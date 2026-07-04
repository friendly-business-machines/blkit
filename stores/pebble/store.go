// Package pebble is the Pebble state-store backend for blkit: a durable,
// embedded store keeping each run's ProcessState in local files, with
// balanced read-and-write performance. It shares the Badger backend's key
// scheme. See specs/state-stores/pebble-state-store.spec.md.
package pebble

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"sync"
	"time"

	pebbledb "github.com/cockroachdb/pebble"
	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the Pebble backend.
type Config struct {
	Path string // directory for Pebble's files; required
}

// Store implements bl.StateStore on a Pebble store. Write batches are
// serialised by a store-level mutex so the persisted arrival counter can
// never regress; reads are lock-free.
type Store struct {
	db *pebbledb.DB

	mu  sync.Mutex
	seq uint64
}

var _ bl.StateStore = (*Store)(nil)

var seqKey = []byte("!seq")

// New opens (creating if needed) the Pebble store and seeds the arrival
// counter from its persisted value. Following the blkit constructor
// convention it panics on an invalid config or an unopenable store.
func New(cfg Config) *Store {
	if cfg.Path == "" {
		panic(fmt.Errorf("pebble state store: Path is required"))
	}
	db, err := pebbledb.Open(cfg.Path, &pebbledb.Options{})
	if err != nil {
		panic(fmt.Errorf("pebble state store: open %s: %w", cfg.Path, err))
	}
	s := &Store{db: db}
	if raw, closer, err := db.Get(seqKey); err == nil {
		s.seq = binary.BigEndian.Uint64(raw)
		closer.Close()
	} else if err != pebbledb.ErrNotFound {
		db.Close()
		panic(fmt.Errorf("pebble state store: seed sequence: %w", err))
	}
	return s
}

// Records and keys are identical to the Badger backend's.
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

// upperBound returns the smallest key greater than every key with the given
// prefix, for iterator bounds.
func upperBound(prefix []byte) []byte {
	ub := append([]byte(nil), prefix...)
	for i := len(ub) - 1; i >= 0; i-- {
		if ub[i] < 0xff {
			ub[i]++
			return ub[:i+1]
		}
	}
	return nil // prefix is all 0xff — no upper bound
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
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.db.Set(metaKey(meta.RunID), enc, pebbledb.NoSync)
}

// WriteBatch applies the ops as one pebble.Batch (atomic on apply), written
// with NoSync for throughput — Flush provides the durability barrier.
func (s *Store) WriteBatch(ops []bl.WriteOp) error {
	for _, op := range ops {
		if err := bl.ValidateWriteOp(op); err != nil {
			return err
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	b := s.db.NewIndexedBatch()
	defer b.Close()
	for _, op := range ops {
		switch op.Kind {
		case bl.OpValueWrite:
			if err := s.applyValueWrite(b, op.RunID, op.ValueWrite); err != nil {
				return err
			}
		case bl.OpStatusFlip:
			if err := applyStatusFlip(b, op.RunID, op.StatusFlip); err != nil {
				return err
			}
		case bl.OpHistoryEntry:
			if err := s.applyHistoryEntry(b, op.RunID, op.HistoryEntry); err != nil {
				return err
			}
		}
	}
	// Persist the arrival counter with the batch so it survives reopen.
	var seqVal [8]byte
	binary.BigEndian.PutUint64(seqVal[:], s.seq)
	if err := b.Set(seqKey, seqVal[:], nil); err != nil {
		return err
	}
	return s.db.Apply(b, pebbledb.NoSync)
}

func (s *Store) applyValueWrite(b *pebbledb.Batch, runID string, w *bl.ValueWrite) error {
	s.seq++
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
	key := eventKey('v', runID, w.Timestamp.UTC().UnixNano(), s.seq)
	if err := b.Set(key, rec, nil); err != nil {
		return err
	}
	return b.Set(pendingKey(runID, w.TaskID, key), key, nil)
}

func applyStatusFlip(b *pebbledb.Batch, runID string, f *bl.StatusFlip) error {
	prefix := []byte("p|" + runID + "|" + f.TaskID + "|")
	// Collect first, then mutate — the indexed-batch iterator does not see
	// writes made after its creation.
	var pendingKeys, eventKeys [][]byte
	it, err := b.NewIter(&pebbledb.IterOptions{LowerBound: prefix, UpperBound: upperBound(prefix)})
	if err != nil {
		return err
	}
	for it.First(); it.Valid(); it.Next() {
		v, err := it.ValueAndErr()
		if err != nil {
			it.Close()
			return err
		}
		pendingKeys = append(pendingKeys, append([]byte(nil), it.Key()...))
		eventKeys = append(eventKeys, append([]byte(nil), v...))
	}
	if err := it.Close(); err != nil {
		return err
	}
	for i, eventK := range eventKeys {
		raw, closer, err := b.Get(eventK)
		if err != nil {
			return fmt.Errorf("status flip: dangling pending index for task %s: %w", f.TaskID, err)
		}
		var rec valueRec
		uerr := json.Unmarshal(raw, &rec)
		closer.Close()
		if uerr != nil {
			return fmt.Errorf("status flip: decode: %w", uerr)
		}
		rec.Status = f.NewStatus.String()
		enc, err := json.Marshal(rec)
		if err != nil {
			return err
		}
		if err := b.Set(eventK, enc, nil); err != nil {
			return err
		}
		if err := b.Delete(pendingKeys[i], nil); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) applyHistoryEntry(b *pebbledb.Batch, runID string, h *bl.HistoryEntry) error {
	s.seq++
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
	return b.Set(eventKey('h', runID, h.Timestamp.UTC().UnixNano(), s.seq), rec, nil)
}

// Flush applies an empty batch carrying a LogData marker with Sync — the
// standard Pebble WAL sync barrier: when it returns, every previously
// applied batch is durable.
func (s *Store) Flush(runID string) error {
	b := s.db.NewBatch()
	defer b.Close()
	if err := b.LogData([]byte("blkit-flush"), nil); err != nil {
		return err
	}
	return s.db.Apply(b, pebbledb.Sync)
}

func (s *Store) runMeta(runID string) (meta bl.RunMetadata, exists bool, err error) {
	meta = bl.RunMetadata{RunID: runID}
	raw, closer, gerr := s.db.Get(metaKey(runID))
	switch gerr {
	case nil:
		uerr := json.Unmarshal(raw, &meta)
		closer.Close()
		if uerr != nil {
			return meta, false, fmt.Errorf("decode metadata: %w", uerr)
		}
		return meta, true, nil
	case pebbledb.ErrNotFound:
		for _, kind := range []byte{'v', 'h'} {
			prefix := []byte(string(kind) + "|" + runID + "|")
			it, err := s.db.NewIter(&pebbledb.IterOptions{LowerBound: prefix, UpperBound: upperBound(prefix)})
			if err != nil {
				return meta, false, err
			}
			valid := it.First()
			if err := it.Close(); err != nil {
				return meta, false, err
			}
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
	meta, exists, err := s.runMeta(runID)
	if err != nil || !exists {
		return nil, err
	}
	cv := &bl.CurrentVersion{Meta: meta, Values: map[string]map[string][]byte{}}
	prefix := []byte("v|" + runID + "|")
	it, err := s.db.NewIter(&pebbledb.IterOptions{LowerBound: prefix, UpperBound: upperBound(prefix)})
	if err != nil {
		return nil, err
	}
	defer it.Close()
	for it.First(); it.Valid(); it.Next() {
		raw, err := it.ValueAndErr()
		if err != nil {
			return nil, err
		}
		var rec valueRec
		if err := json.Unmarshal(raw, &rec); err != nil {
			return nil, fmt.Errorf("decode value record: %w", err)
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
	return cv, nil
}

// FullHistory iterates the v| and h| prefixes in key (= replay) order.
// Returns (nil, nil) for an unknown run.
func (s *Store) FullHistory(runID string) (*bl.FullHistory, error) {
	meta, exists, err := s.runMeta(runID)
	if err != nil || !exists {
		return nil, err
	}
	fh := &bl.FullHistory{Meta: meta}

	vPrefix := []byte("v|" + runID + "|")
	it, err := s.db.NewIter(&pebbledb.IterOptions{LowerBound: vPrefix, UpperBound: upperBound(vPrefix)})
	if err != nil {
		return nil, err
	}
	for it.First(); it.Valid(); it.Next() {
		raw, verr := it.ValueAndErr()
		if verr != nil {
			it.Close()
			return nil, verr
		}
		var rec valueRec
		if err := json.Unmarshal(raw, &rec); err != nil {
			it.Close()
			return nil, fmt.Errorf("decode value record: %w", err)
		}
		status, err := bl.ParseValueStatus(rec.Status)
		if err != nil {
			it.Close()
			return nil, err
		}
		tsNanos, seq := splitEventKey(it.Key())
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
	if err := it.Close(); err != nil {
		return nil, err
	}

	hPrefix := []byte("h|" + runID + "|")
	hit, err := s.db.NewIter(&pebbledb.IterOptions{LowerBound: hPrefix, UpperBound: upperBound(hPrefix)})
	if err != nil {
		return nil, err
	}
	defer hit.Close()
	for hit.First(); hit.Valid(); hit.Next() {
		raw, verr := hit.ValueAndErr()
		if verr != nil {
			return nil, verr
		}
		var rec historyRec
		if err := json.Unmarshal(raw, &rec); err != nil {
			return nil, fmt.Errorf("decode history record: %w", err)
		}
		tsNanos, seq := splitEventKey(hit.Key())
		fh.History = append(fh.History, bl.HistoryRecord{
			Kind:        rec.Kind,
			NodeID:      rec.NodeID,
			ExecutionID: rec.ExecutionID,
			Payload:     append([]byte(nil), rec.Payload...),
			Timestamp:   time.Unix(0, tsNanos).UTC(),
			Seq:         seq,
		})
	}
	return fh, nil
}

// Config returns an error: a Pebble store is local to one process on one
// machine and cannot be shared.
func (s *Store) Config() (map[string]string, error) {
	return nil, fmt.Errorf("pebble state store cannot be shared across processes")
}

// Close closes the store.
func (s *Store) Close() error { return s.db.Close() }
