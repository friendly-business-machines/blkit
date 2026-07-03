// Package badger is the BadgerDB state-store backend for blkit: a durable,
// embedded store keeping each run's ProcessState in local files, tuned for
// write throughput. See specs/stores/badger-state-store.spec.md.
package badger

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"time"

	badgerdb "github.com/dgraph-io/badger/v4"
	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the Badger backend.
type Config struct {
	Path string // directory for Badger's files; required
}

// Store implements bl.StateStore on a BadgerDB store.
type Store struct {
	db  *badgerdb.DB
	seq *badgerdb.Sequence
}

var _ bl.StateStore = (*Store)(nil)

// New opens (creating if needed) the Badger store. Following the blkit
// constructor convention it panics on an invalid config or an unopenable
// store. The store runs with SyncWrites off for throughput; Flush provides
// the durability barrier via DB.Sync.
func New(cfg Config) *Store {
	if cfg.Path == "" {
		panic(fmt.Errorf("badger state store: Path is required"))
	}
	opts := badgerdb.DefaultOptions(cfg.Path).
		WithLogger(nil).
		WithSyncWrites(false)
	db, err := badgerdb.Open(opts)
	if err != nil {
		panic(fmt.Errorf("badger state store: open %s: %w", cfg.Path, err))
	}
	// Durable monotonic arrival counter — survives reopen.
	seq, err := db.GetSequence([]byte("!seq"), 256)
	if err != nil {
		db.Close()
		panic(fmt.Errorf("badger state store: sequence: %w", err))
	}
	return &Store{db: db, seq: seq}
}

// Records share the JSON shape used by the other embedded backends.
type valueRec struct {
	TaskID      string          `json:"task_id"`
	ExecutionID string          `json:"execution_id"`
	Field       string          `json:"field"`
	Value       json.RawMessage `json:"value"`
	Status      string          `json:"status"`
	TS          int64           `json:"ts"`
}

type historyRec struct {
	Kind        string          `json:"kind"`
	NodeID      *string         `json:"node_id,omitempty"`
	ExecutionID string          `json:"execution_id"`
	Payload     json.RawMessage `json:"payload"`
	TS          int64           `json:"ts"`
}

// Keys: m|{runID}, v|{runID}|{ts8}{seq8}, h|{runID}|{ts8}{seq8}, and the flip
// index p|{runID}|{taskID}|{ts8}{seq8} -> value key. Fixed-width big-endian
// suffixes make lexicographic key order the replay order.
func metaKey(runID string) []byte { return []byte("m|" + runID) }

func eventKey(kind byte, runID string, tsNanos int64, seq uint64) []byte {
	k := make([]byte, 0, 3+len(runID)+16)
	k = append(k, kind, '|')
	k = append(k, runID...)
	k = append(k, '|')
	var suffix [16]byte
	binary.BigEndian.PutUint64(suffix[:8], uint64(tsNanos))
	binary.BigEndian.PutUint64(suffix[8:], seq)
	return append(k, suffix[:]...)
}

func pendingKey(runID, taskID string, eventK []byte) []byte {
	k := make([]byte, 0, 3+len(runID)+len(taskID)+1+16)
	k = append(k, 'p', '|')
	k = append(k, runID...)
	k = append(k, '|')
	k = append(k, taskID...)
	k = append(k, '|')
	return append(k, eventK[len(eventK)-16:]...)
}

func splitEventKey(k []byte) (tsNanos int64, seq uint64) {
	suffix := k[len(k)-16:]
	return int64(binary.BigEndian.Uint64(suffix[:8])), binary.BigEndian.Uint64(suffix[8:])
}

// Save upserts the run's metadata.
func (s *Store) Save(meta bl.RunMetadata) error {
	if meta.RunID == "" {
		return fmt.Errorf("save: empty RunID")
	}
	enc, err := json.Marshal(meta)
	if err != nil {
		return fmt.Errorf("save: encode metadata: %w", err)
	}
	return s.db.Update(func(txn *badgerdb.Txn) error {
		return txn.Set(metaKey(meta.RunID), enc)
	})
}

// WriteBatch applies the ops in a single Badger transaction, so a task's
// outputs land together.
func (s *Store) WriteBatch(ops []bl.WriteOp) error {
	for _, op := range ops {
		if err := bl.ValidateWriteOp(op); err != nil {
			return err
		}
	}
	return s.db.Update(func(txn *badgerdb.Txn) error {
		for _, op := range ops {
			switch op.Kind {
			case bl.OpValueWrite:
				if err := s.applyValueWrite(txn, op.RunID, op.ValueWrite); err != nil {
					return err
				}
			case bl.OpStatusFlip:
				if err := applyStatusFlip(txn, op.RunID, op.StatusFlip); err != nil {
					return err
				}
			case bl.OpHistoryEntry:
				if err := s.applyHistoryEntry(txn, op.RunID, op.HistoryEntry); err != nil {
					return err
				}
			}
		}
		return nil
	})
}

func (s *Store) applyValueWrite(txn *badgerdb.Txn, runID string, w *bl.ValueWrite) error {
	seq, err := s.seq.Next()
	if err != nil {
		return err
	}
	rec, err := json.Marshal(valueRec{
		TaskID:      w.TaskID,
		ExecutionID: w.ExecutionID,
		Field:       w.Field,
		Value:       json.RawMessage(w.Value),
		Status:      bl.ValueStatusPending.String(),
		TS:          w.Timestamp.UTC().UnixNano(),
	})
	if err != nil {
		return fmt.Errorf("value write: encode: %w", err)
	}
	key := eventKey('v', runID, w.Timestamp.UTC().UnixNano(), seq)
	if err := txn.Set(key, rec); err != nil {
		return err
	}
	return txn.Set(pendingKey(runID, w.TaskID, key), key)
}

func applyStatusFlip(txn *badgerdb.Txn, runID string, f *bl.StatusFlip) error {
	prefix := []byte("p|" + runID + "|" + f.TaskID + "|")
	// Collect first, then mutate — no writes while the iterator is open.
	var pendingKeys, eventKeys [][]byte
	it := txn.NewIterator(badgerdb.IteratorOptions{Prefix: prefix})
	for it.Rewind(); it.Valid(); it.Next() {
		item := it.Item()
		eventK, err := item.ValueCopy(nil)
		if err != nil {
			it.Close()
			return err
		}
		pendingKeys = append(pendingKeys, item.KeyCopy(nil))
		eventKeys = append(eventKeys, eventK)
	}
	it.Close()
	for i, eventK := range eventKeys {
		item, err := txn.Get(eventK)
		if err != nil {
			return fmt.Errorf("status flip: dangling pending index for task %s: %w", f.TaskID, err)
		}
		raw, err := item.ValueCopy(nil)
		if err != nil {
			return err
		}
		var rec valueRec
		if err := json.Unmarshal(raw, &rec); err != nil {
			return fmt.Errorf("status flip: decode: %w", err)
		}
		rec.Status = f.NewStatus.String()
		enc, err := json.Marshal(rec)
		if err != nil {
			return err
		}
		if err := txn.Set(eventK, enc); err != nil {
			return err
		}
		if err := txn.Delete(pendingKeys[i]); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) applyHistoryEntry(txn *badgerdb.Txn, runID string, h *bl.HistoryEntry) error {
	seq, err := s.seq.Next()
	if err != nil {
		return err
	}
	payload := h.Payload
	if len(payload) == 0 {
		payload = []byte("null")
	}
	rec, err := json.Marshal(historyRec{
		Kind:        h.Kind,
		NodeID:      h.NodeID,
		ExecutionID: h.ExecutionID,
		Payload:     json.RawMessage(payload),
		TS:          h.Timestamp.UTC().UnixNano(),
	})
	if err != nil {
		return fmt.Errorf("history entry: encode: %w", err)
	}
	return txn.Set(eventKey('h', runID, h.Timestamp.UTC().UnixNano(), seq), rec)
}

// Flush syncs Badger's WAL to disk — the durability barrier that pairs with
// running SyncWrites off.
func (s *Store) Flush(runID string) error { return s.db.Sync() }

func (s *Store) runMeta(txn *badgerdb.Txn, runID string) (meta bl.RunMetadata, exists bool, err error) {
	meta = bl.RunMetadata{RunID: runID}
	item, gerr := txn.Get(metaKey(runID))
	switch gerr {
	case nil:
		raw, err := item.ValueCopy(nil)
		if err != nil {
			return meta, false, err
		}
		if err := json.Unmarshal(raw, &meta); err != nil {
			return meta, false, fmt.Errorf("decode metadata: %w", err)
		}
		return meta, true, nil
	case badgerdb.ErrKeyNotFound:
		for _, kind := range []byte{'v', 'h'} {
			prefix := []byte(string(kind) + "|" + runID + "|")
			it := txn.NewIterator(badgerdb.IteratorOptions{Prefix: prefix})
			it.Rewind()
			valid := it.Valid()
			it.Close()
			if valid {
				return meta, true, nil
			}
		}
		return meta, false, nil
	default:
		return meta, false, gerr
	}
}

// CurrentVersion folds the run's value records into the latest committed
// value per (task, field); iteration order is replay order. Returns
// (nil, nil) for an unknown run.
func (s *Store) CurrentVersion(runID string) (*bl.CurrentVersion, error) {
	var cv *bl.CurrentVersion
	err := s.db.View(func(txn *badgerdb.Txn) error {
		meta, exists, err := s.runMeta(txn, runID)
		if err != nil || !exists {
			return err
		}
		cv = &bl.CurrentVersion{Meta: meta, Values: map[string]map[string][]byte{}}
		prefix := []byte("v|" + runID + "|")
		it := txn.NewIterator(badgerdb.IteratorOptions{Prefix: prefix, PrefetchValues: true})
		defer it.Close()
		for it.Rewind(); it.Valid(); it.Next() {
			raw, err := it.Item().ValueCopy(nil)
			if err != nil {
				return err
			}
			var rec valueRec
			if err := json.Unmarshal(raw, &rec); err != nil {
				return fmt.Errorf("decode value record: %w", err)
			}
			if rec.Status != bl.ValueStatusCommitted.String() {
				continue
			}
			task, ok := cv.Values[rec.TaskID]
			if !ok {
				task = map[string][]byte{}
				cv.Values[rec.TaskID] = task
			}
			task[rec.Field] = append([]byte(nil), rec.Value...)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return cv, nil
}

// FullHistory iterates the v| and h| prefixes in key (= replay) order.
// Returns (nil, nil) for an unknown run.
func (s *Store) FullHistory(runID string) (*bl.FullHistory, error) {
	var fh *bl.FullHistory
	err := s.db.View(func(txn *badgerdb.Txn) error {
		meta, exists, err := s.runMeta(txn, runID)
		if err != nil || !exists {
			return err
		}
		fh = &bl.FullHistory{Meta: meta}

		vPrefix := []byte("v|" + runID + "|")
		it := txn.NewIterator(badgerdb.IteratorOptions{Prefix: vPrefix, PrefetchValues: true})
		for it.Rewind(); it.Valid(); it.Next() {
			item := it.Item()
			raw, err := item.ValueCopy(nil)
			if err != nil {
				it.Close()
				return err
			}
			var rec valueRec
			if err := json.Unmarshal(raw, &rec); err != nil {
				it.Close()
				return fmt.Errorf("decode value record: %w", err)
			}
			status, err := bl.ParseValueStatus(rec.Status)
			if err != nil {
				it.Close()
				return err
			}
			tsNanos, seq := splitEventKey(item.Key())
			fh.Values = append(fh.Values, bl.ValueRecord{
				TaskID:      rec.TaskID,
				ExecutionID: rec.ExecutionID,
				Field:       rec.Field,
				Value:       append([]byte(nil), rec.Value...),
				Status:      status,
				Timestamp:   time.Unix(0, tsNanos).UTC(),
				Seq:         seq,
			})
		}
		it.Close()

		hPrefix := []byte("h|" + runID + "|")
		hit := txn.NewIterator(badgerdb.IteratorOptions{Prefix: hPrefix, PrefetchValues: true})
		defer hit.Close()
		for hit.Rewind(); hit.Valid(); hit.Next() {
			item := hit.Item()
			raw, err := item.ValueCopy(nil)
			if err != nil {
				return err
			}
			var rec historyRec
			if err := json.Unmarshal(raw, &rec); err != nil {
				return fmt.Errorf("decode history record: %w", err)
			}
			tsNanos, seq := splitEventKey(item.Key())
			fh.History = append(fh.History, bl.HistoryRecord{
				Kind:        rec.Kind,
				NodeID:      rec.NodeID,
				ExecutionID: rec.ExecutionID,
				Payload:     append([]byte(nil), rec.Payload...),
				Timestamp:   time.Unix(0, tsNanos).UTC(),
				Seq:         seq,
			})
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	if fh != nil {
		// Badger sequences lease in bands, so arrival numbers are monotonic
		// but keys from different bands can interleave within a timestamp;
		// normalise with the shared replay sort.
		bl.SortValueRecords(fh.Values)
		bl.SortHistoryRecords(fh.History)
	}
	return fh, nil
}

// Config returns an error: a Badger store is local to one process on one
// machine and cannot be shared.
func (s *Store) Config() (map[string]string, error) {
	return nil, fmt.Errorf("badger state store cannot be shared across processes")
}

// Close releases the sequence lease and closes the store.
func (s *Store) Close() error {
	if err := s.seq.Release(); err != nil {
		s.db.Close()
		return err
	}
	return s.db.Close()
}
