// Package bbolt is the bbolt state-store backend for blkit: a durable,
// embedded store keeping each run's ProcessState in a single file on local
// disk. See specs/state-stores/bbolt-state-store.spec.md.
package bbolt

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"time"

	bl "github.com/friendly-business-machines/blkit/core"
	bolt "go.etcd.io/bbolt"
)

func nanosToTime(n int64) time.Time { return time.Unix(0, n).UTC() }

// Config holds the construction parameters for the bbolt backend.
type Config struct {
	Path string // path to the database file; required
}

// Store implements bl.StateStore on a bbolt database file.
type Store struct {
	db *bolt.DB
}

var _ bl.StateStore = (*Store)(nil)

var (
	bucketRuns    = []byte("runs")
	bucketValues  = []byte("values")
	bucketHistory = []byte("history")
	bucketPending = []byte("pending")
	keyMeta       = []byte("meta")
)

// New opens (creating if needed) the database file and returns the store.
// Following the blkit constructor convention it panics on an invalid config
// or an unopenable file.
func New(cfg Config) *Store {
	if cfg.Path == "" {
		panic(fmt.Errorf("bbolt state store: Path is required"))
	}
	db, err := bolt.Open(cfg.Path, 0o600, nil)
	if err != nil {
		panic(fmt.Errorf("bbolt state store: open %s: %w", cfg.Path, err))
	}
	if err := db.Update(func(tx *bolt.Tx) error {
		_, err := tx.CreateBucketIfNotExists(bucketRuns)
		return err
	}); err != nil {
		db.Close()
		panic(fmt.Errorf("bbolt state store: init %s: %w", cfg.Path, err))
	}
	return &Store{db: db}
}

// Records are stored as JSON so the file stays inspectable with the bbolt
// CLI. Value payloads are JSON-encoded Bl values and embed verbatim.
type valueRec struct {
	TaskID      string          `json:"task_id"`
	ExecutionID string          `json:"execution_id"`
	Field       string          `json:"field"`
	Value       json.RawMessage `json:"value"`
	Status      string          `json:"status"`
	TS          int64           `json:"ts"` // Unix nanoseconds, UTC
}

type historyRec struct {
	Kind        string          `json:"kind"`
	NodeID      *string         `json:"node_id,omitempty"`
	ExecutionID string          `json:"execution_id"`
	Payload     json.RawMessage `json:"payload"`
	TS          int64           `json:"ts"`
}

// eventKey is 8 bytes of big-endian Unix nanoseconds followed by 8 bytes of
// big-endian sequence number — so bbolt's natural key order IS the replay
// order (Timestamp, arrival).
func eventKey(tsNanos int64, seq uint64) []byte {
	k := make([]byte, 16)
	binary.BigEndian.PutUint64(k[:8], uint64(tsNanos))
	binary.BigEndian.PutUint64(k[8:], seq)
	return k
}

func splitEventKey(k []byte) (tsNanos int64, seq uint64) {
	return int64(binary.BigEndian.Uint64(k[:8])), binary.BigEndian.Uint64(k[8:])
}

// pendingKey prefixes the event key with the task id (NUL-separated) so a
// status flip is a single prefix scan.
func pendingKey(taskID string, event []byte) []byte {
	k := make([]byte, 0, len(taskID)+1+len(event))
	k = append(k, taskID...)
	k = append(k, 0)
	return append(k, event...)
}

func runBucket(tx *bolt.Tx, runID string, create bool) (*bolt.Bucket, error) {
	runs := tx.Bucket(bucketRuns)
	if create {
		rb, err := runs.CreateBucketIfNotExists([]byte(runID))
		if err != nil {
			return nil, err
		}
		for _, b := range [][]byte{bucketValues, bucketHistory, bucketPending} {
			if _, err := rb.CreateBucketIfNotExists(b); err != nil {
				return nil, err
			}
		}
		return rb, nil
	}
	return runs.Bucket([]byte(runID)), nil
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
	return s.db.Update(func(tx *bolt.Tx) error {
		rb, err := runBucket(tx, meta.RunID, true)
		if err != nil {
			return err
		}
		return rb.Put(keyMeta, enc)
	})
}

// WriteBatch applies the ops in one bbolt Update transaction — atomic, and
// durable when it returns (bbolt fsyncs on commit), which is why Flush is a
// no-op.
func (s *Store) WriteBatch(ops []bl.WriteOp) error {
	for _, op := range ops {
		if err := bl.ValidateWriteOp(op); err != nil {
			return err
		}
	}
	return s.db.Update(func(tx *bolt.Tx) error {
		for _, op := range ops {
			rb, err := runBucket(tx, op.RunID, true)
			if err != nil {
				return err
			}
			switch op.Kind {
			case bl.OpValueWrite:
				if err := applyValueWrite(rb, op.ValueWrite); err != nil {
					return err
				}
			case bl.OpStatusFlip:
				if err := applyStatusFlip(rb, op.StatusFlip); err != nil {
					return err
				}
			case bl.OpHistoryEntry:
				if err := applyHistoryEntry(rb, op.HistoryEntry); err != nil {
					return err
				}
			}
		}
		return nil
	})
}

func applyValueWrite(rb *bolt.Bucket, w *bl.ValueWrite) error {
	values := rb.Bucket(bucketValues)
	seq, err := values.NextSequence()
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
	key := eventKey(w.Timestamp.UTC().UnixNano(), seq)
	if err := values.Put(key, rec); err != nil {
		return err
	}
	return rb.Bucket(bucketPending).Put(pendingKey(w.TaskID, key), key)
}

func applyStatusFlip(rb *bolt.Bucket, f *bl.StatusFlip) error {
	values := rb.Bucket(bucketValues)
	pending := rb.Bucket(bucketPending)
	prefix := append([]byte(f.TaskID), 0)

	var pendingKeys [][]byte
	c := pending.Cursor()
	for k, _ := c.Seek(prefix); k != nil && bytes.HasPrefix(k, prefix); k, _ = c.Next() {
		pendingKeys = append(pendingKeys, append([]byte(nil), k...))
	}
	for _, pk := range pendingKeys {
		eventK := pending.Get(pk)
		raw := values.Get(eventK)
		if raw == nil {
			return fmt.Errorf("status flip: dangling pending index for task %s", f.TaskID)
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
		if err := values.Put(append([]byte(nil), eventK...), enc); err != nil {
			return err
		}
		if err := pending.Delete(pk); err != nil {
			return err
		}
	}
	return nil
}

func applyHistoryEntry(rb *bolt.Bucket, h *bl.HistoryEntry) error {
	history := rb.Bucket(bucketHistory)
	seq, err := history.NextSequence()
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
	return history.Put(eventKey(h.Timestamp.UTC().UnixNano(), seq), rec)
}

// Flush is a no-op: every Update transaction is fsynced on commit.
func (s *Store) Flush(runID string) error { return nil }

func (s *Store) loadMeta(rb *bolt.Bucket, runID string) (bl.RunMetadata, error) {
	meta := bl.RunMetadata{RunID: runID}
	if raw := rb.Get(keyMeta); raw != nil {
		if err := json.Unmarshal(raw, &meta); err != nil {
			return meta, fmt.Errorf("decode metadata: %w", err)
		}
	}
	return meta, nil
}

// CurrentVersion folds the run's value records into the latest committed
// value per (task, field). Key order is replay order, so a single in-order
// walk suffices. Returns (nil, nil) for an unknown run.
func (s *Store) CurrentVersion(runID string) (*bl.CurrentVersion, error) {
	var cv *bl.CurrentVersion
	err := s.db.View(func(tx *bolt.Tx) error {
		rb, _ := runBucket(tx, runID, false)
		if rb == nil {
			return nil
		}
		meta, err := s.loadMeta(rb, runID)
		if err != nil {
			return err
		}
		cv = &bl.CurrentVersion{Meta: meta, Values: map[string]map[string][]byte{}}
		return rb.Bucket(bucketValues).ForEach(func(k, v []byte) error {
			var rec valueRec
			if err := json.Unmarshal(v, &rec); err != nil {
				return fmt.Errorf("decode value record: %w", err)
			}
			if rec.Status != bl.ValueStatusCommitted.String() {
				return nil
			}
			task, ok := cv.Values[rec.TaskID]
			if !ok {
				task = map[string][]byte{}
				cv.Values[rec.TaskID] = task
			}
			task[rec.Field] = append([]byte(nil), rec.Value...)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return cv, nil
}

// FullHistory walks both buckets in key (= replay) order. Returns (nil, nil)
// for an unknown run.
func (s *Store) FullHistory(runID string) (*bl.FullHistory, error) {
	var fh *bl.FullHistory
	err := s.db.View(func(tx *bolt.Tx) error {
		rb, _ := runBucket(tx, runID, false)
		if rb == nil {
			return nil
		}
		meta, err := s.loadMeta(rb, runID)
		if err != nil {
			return err
		}
		fh = &bl.FullHistory{Meta: meta}
		if err := rb.Bucket(bucketValues).ForEach(func(k, v []byte) error {
			var rec valueRec
			if err := json.Unmarshal(v, &rec); err != nil {
				return fmt.Errorf("decode value record: %w", err)
			}
			status, err := bl.ParseValueStatus(rec.Status)
			if err != nil {
				return err
			}
			tsNanos, seq := splitEventKey(k)
			fh.Values = append(fh.Values, bl.ValueRecord{
				TaskID:      rec.TaskID,
				ExecutionID: rec.ExecutionID,
				Field:       rec.Field,
				Value:       append([]byte(nil), rec.Value...),
				Status:      status,
				Timestamp:   nanosToTime(tsNanos),
				Seq:         seq,
			})
			return nil
		}); err != nil {
			return err
		}
		return rb.Bucket(bucketHistory).ForEach(func(k, v []byte) error {
			var rec historyRec
			if err := json.Unmarshal(v, &rec); err != nil {
				return fmt.Errorf("decode history record: %w", err)
			}
			tsNanos, seq := splitEventKey(k)
			fh.History = append(fh.History, bl.HistoryRecord{
				Kind:        rec.Kind,
				NodeID:      rec.NodeID,
				ExecutionID: rec.ExecutionID,
				Payload:     append([]byte(nil), rec.Payload...),
				Timestamp:   nanosToTime(tsNanos),
				Seq:         seq,
			})
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return fh, nil
}

// Config returns an error: a bbolt file is local to one process on one
// machine and cannot be shared.
func (s *Store) Config() (map[string]string, error) {
	return nil, fmt.Errorf("bbolt state store cannot be shared across processes")
}

// Close closes the database file.
func (s *Store) Close() error { return s.db.Close() }
