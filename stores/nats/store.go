// Package nats is the NATS JetStream state-store backend for blkit: a
// durable, shareable store keeping each run's ProcessState in a JetStream
// key-value bucket — a natural fit when NATS is already the message broker.
// See specs/state-stores/nats-state-store.spec.md.
package nats

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"sync/atomic"
	"time"

	natsgo "github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"

	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the NATS backend.
type Config struct {
	URL    string // NATS server URL(s); required
	Bucket string // JetStream key-value bucket name; required
}

// Store implements bl.StateStore on a JetStream key-value bucket.
type Store struct {
	nc     *natsgo.Conn
	kv     jetstream.KeyValue
	url    string
	bucket string
	n      atomic.Uint64 // per-open counter: keeps same-nanosecond keys distinct
}

var _ bl.StateStore = (*Store)(nil)

const opTimeout = 10 * time.Second

// New connects to NATS and creates (or binds to) the bucket, configured with
// history depth 1 — old revisions of a flipped entry are not needed; the
// audit story lives in the records themselves. Following the blkit
// constructor convention it panics on an invalid config or a failed connect.
func New(cfg Config) *Store {
	if cfg.URL == "" || cfg.Bucket == "" {
		panic(fmt.Errorf("nats state store: URL and Bucket are required"))
	}
	nc, err := natsgo.Connect(cfg.URL)
	if err != nil {
		panic(fmt.Errorf("nats state store: connect %s: %w", cfg.URL, err))
	}
	js, err := jetstream.New(nc)
	if err != nil {
		nc.Close()
		panic(fmt.Errorf("nats state store: jetstream: %w", err))
	}
	ctx, cancel := context.WithTimeout(context.Background(), opTimeout)
	defer cancel()
	kv, err := js.CreateOrUpdateKeyValue(ctx, jetstream.KeyValueConfig{
		Bucket:  cfg.Bucket,
		History: 1,
	})
	if err != nil {
		nc.Close()
		panic(fmt.Errorf("nats state store: bucket %s: %w", cfg.Bucket, err))
	}
	return &Store{nc: nc, kv: kv, url: cfg.URL, bucket: cfg.Bucket}
}

// Records share the JSON shape used by the embedded backends.
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

// Keys: {runID}.meta, {runID}.v.{ts}.{n}, {runID}.h.{ts}.{n}. The fixed-width
// decimal ts keeps keys unique and readable; the authoritative arrival
// tiebreak on replay is the entry's JetStream revision (stream sequence).
func (s *Store) eventKey(kind, runID string, tsNanos int64) string {
	return fmt.Sprintf("%s.%s.%020d.%012d", runID, kind, tsNanos, s.n.Add(1))
}

func metaSuffix(runID string) string { return runID + ".meta" }

func opCtx() (context.Context, context.CancelFunc) {
	return context.WithTimeout(context.Background(), opTimeout)
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
	ctx, cancel := opCtx()
	defer cancel()
	_, err = s.kv.Put(ctx, metaSuffix(meta.RunID), enc)
	return err
}

// WriteBatch applies the ops one by one — the KV store has no cross-key
// batching primitive, which the write contract allows. Each Put waits for
// the JetStream publish acknowledgement, so an accepted write is already
// quorum-accepted and durable — which is why Flush is a no-op.
func (s *Store) WriteBatch(ops []bl.WriteOp) error {
	for _, op := range ops {
		if err := bl.ValidateWriteOp(op); err != nil {
			return err
		}
	}
	for _, op := range ops {
		var err error
		switch op.Kind {
		case bl.OpValueWrite:
			err = s.applyValueWrite(op.RunID, op.ValueWrite)
		case bl.OpStatusFlip:
			err = s.applyStatusFlip(op.RunID, op.StatusFlip)
		case bl.OpHistoryEntry:
			err = s.applyHistoryEntry(op.RunID, op.HistoryEntry)
		}
		if err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) applyValueWrite(runID string, w *bl.ValueWrite) error {
	enc, err := json.Marshal(valueRec{
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
	ctx, cancel := opCtx()
	defer cancel()
	_, err = s.kv.Put(ctx, s.eventKey("v", runID, w.Timestamp.UTC().UnixNano()), enc)
	return err
}

// applyStatusFlip lists the run's value keys and rewrites the task's pending
// records on their own keys — a flip is simply a new revision of each entry.
func (s *Store) applyStatusFlip(runID string, f *bl.StatusFlip) error {
	keys, err := s.runKeys(runID + ".v.")
	if err != nil {
		return err
	}
	for _, key := range keys {
		ctx, cancel := opCtx()
		entry, err := s.kv.Get(ctx, key)
		cancel()
		if err != nil {
			return err
		}
		var rec valueRec
		if err := json.Unmarshal(entry.Value(), &rec); err != nil {
			return fmt.Errorf("status flip: decode %s: %w", key, err)
		}
		if rec.TaskID != f.TaskID || rec.Status != bl.ValueStatusPending.String() {
			continue
		}
		rec.Status = f.NewStatus.String()
		enc, err := json.Marshal(rec)
		if err != nil {
			return err
		}
		pctx, pcancel := opCtx()
		_, err = s.kv.Put(pctx, key, enc)
		pcancel()
		if err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) applyHistoryEntry(runID string, h *bl.HistoryEntry) error {
	payload := h.Payload
	if len(payload) == 0 {
		payload = []byte("null")
	}
	enc, err := json.Marshal(historyRec{
		Kind:        h.Kind,
		NodeID:      h.NodeID,
		ExecutionID: h.ExecutionID,
		Payload:     json.RawMessage(payload),
		TS:          h.Timestamp.UTC().UnixNano(),
	})
	if err != nil {
		return fmt.Errorf("history entry: encode: %w", err)
	}
	ctx, cancel := opCtx()
	defer cancel()
	_, err = s.kv.Put(ctx, s.eventKey("h", runID, h.Timestamp.UTC().UnixNano()), enc)
	return err
}

// Flush is a no-op: every Put has already been acknowledged by JetStream.
func (s *Store) Flush(runID string) error { return nil }

// runKeys lists the bucket's keys filtered to the given prefix.
func (s *Store) runKeys(prefix string) ([]string, error) {
	ctx, cancel := opCtx()
	defer cancel()
	lister, err := s.kv.ListKeys(ctx)
	if err != nil {
		return nil, err
	}
	var keys []string
	for key := range lister.Keys() {
		if strings.HasPrefix(key, prefix) {
			keys = append(keys, key)
		}
	}
	return keys, nil
}

func (s *Store) runMeta(runID string) (meta bl.RunMetadata, exists bool, err error) {
	meta = bl.RunMetadata{RunID: runID}
	ctx, cancel := opCtx()
	entry, gerr := s.kv.Get(ctx, metaSuffix(runID))
	cancel()
	switch {
	case gerr == nil:
		if err := json.Unmarshal(entry.Value(), &meta); err != nil {
			return meta, false, fmt.Errorf("decode metadata: %w", err)
		}
		return meta, true, nil
	case gerr == jetstream.ErrKeyNotFound:
		keys, err := s.runKeys(runID + ".")
		if err != nil {
			return meta, false, err
		}
		return meta, len(keys) > 0, nil
	default:
		return meta, false, gerr
	}
}

func (s *Store) loadValues(runID string) ([]bl.ValueRecord, error) {
	keys, err := s.runKeys(runID + ".v.")
	if err != nil {
		return nil, err
	}
	var recs []bl.ValueRecord
	for _, key := range keys {
		ctx, cancel := opCtx()
		entry, err := s.kv.Get(ctx, key)
		cancel()
		if err != nil {
			return nil, err
		}
		var rec valueRec
		if err := json.Unmarshal(entry.Value(), &rec); err != nil {
			return nil, fmt.Errorf("decode value record %s: %w", key, err)
		}
		status, err := bl.ParseValueStatus(rec.Status)
		if err != nil {
			return nil, err
		}
		recs = append(recs, bl.ValueRecord{
			TaskID:      rec.TaskID,
			ExecutionID: rec.ExecutionID,
			Field:       rec.Field,
			Value:       append([]byte(nil), rec.Value...),
			Status:      status,
			Timestamp:   time.Unix(0, rec.TS).UTC(),
			Seq:         entry.Revision(),
		})
	}
	bl.SortValueRecords(recs)
	return recs, nil
}

// CurrentVersion folds the run's value records into the latest committed
// value per (task, field). Returns (nil, nil) for an unknown run.
func (s *Store) CurrentVersion(runID string) (*bl.CurrentVersion, error) {
	meta, exists, err := s.runMeta(runID)
	if err != nil || !exists {
		return nil, err
	}
	values, err := s.loadValues(runID)
	if err != nil {
		return nil, err
	}
	cv := &bl.CurrentVersion{Meta: meta, Values: map[string]map[string][]byte{}}
	for _, rec := range values {
		if rec.Status != bl.ValueStatusCommitted {
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

// FullHistory reads the run's v. and h. entries and sorts them into replay
// order (Timestamp, revision). Returns (nil, nil) for an unknown run.
func (s *Store) FullHistory(runID string) (*bl.FullHistory, error) {
	meta, exists, err := s.runMeta(runID)
	if err != nil || !exists {
		return nil, err
	}
	fh := &bl.FullHistory{Meta: meta}
	if fh.Values, err = s.loadValues(runID); err != nil {
		return nil, err
	}
	keys, err := s.runKeys(runID + ".h.")
	if err != nil {
		return nil, err
	}
	for _, key := range keys {
		ctx, cancel := opCtx()
		entry, err := s.kv.Get(ctx, key)
		cancel()
		if err != nil {
			return nil, err
		}
		var rec historyRec
		if err := json.Unmarshal(entry.Value(), &rec); err != nil {
			return nil, fmt.Errorf("decode history record %s: %w", key, err)
		}
		fh.History = append(fh.History, bl.HistoryRecord{
			Kind:        rec.Kind,
			NodeID:      rec.NodeID,
			ExecutionID: rec.ExecutionID,
			Payload:     append([]byte(nil), rec.Payload...),
			Timestamp:   time.Unix(0, rec.TS).UTC(),
			Seq:         entry.Revision(),
		})
	}
	bl.SortHistoryRecords(fh.History)
	return fh, nil
}

// Config returns the connection details another process needs to reach the
// same store.
func (s *Store) Config() (map[string]string, error) {
	return map[string]string{"url": s.url, "bucket": s.bucket}, nil
}

// Close closes the NATS connection.
func (s *Store) Close() error {
	s.nc.Close()
	return nil
}
