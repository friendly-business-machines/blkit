package core

import (
	"fmt"
	"sync"
)

// InMemoryStateStore keeps each run's ProcessState in memory. It is the
// built-in, zero-dependency backend used for tests, examples, and local
// single-process runs. Not durable, cannot be shared across processes.
// See specs/state-stores/in-memory-state-store.spec.md.
type InMemoryStateStore struct {
	mu   sync.RWMutex
	runs map[string]*inMemoryRun
}

type inMemoryRun struct {
	meta    RunMetadata
	hasMeta bool
	values  []ValueRecord
	history []HistoryRecord
	seq     uint64
}

// NewInMemoryStateStore returns an empty in-memory state store.
func NewInMemoryStateStore() *InMemoryStateStore {
	return &InMemoryStateStore{runs: map[string]*inMemoryRun{}}
}

func (s *InMemoryStateStore) run(runID string) *inMemoryRun {
	r, ok := s.runs[runID]
	if !ok {
		r = &inMemoryRun{meta: RunMetadata{RunID: runID}}
		s.runs[runID] = r
	}
	return r
}

// Save upserts the run's metadata.
func (s *InMemoryStateStore) Save(meta RunMetadata) error {
	if meta.RunID == "" {
		return fmt.Errorf("save: empty RunID")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	r := s.run(meta.RunID)
	r.meta = meta
	r.hasMeta = true
	return nil
}

// WriteBatch applies the ops synchronously under the store lock, so the
// batch is atomic with respect to readers.
func (s *InMemoryStateStore) WriteBatch(ops []WriteOp) error {
	for _, op := range ops {
		if err := ValidateWriteOp(op); err != nil {
			return err
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, op := range ops {
		r := s.run(op.RunID)
		switch op.Kind {
		case OpValueWrite:
			w := op.ValueWrite
			r.seq++
			r.values = append(r.values, ValueRecord{
				TaskID:      w.TaskID,
				ExecutionID: w.ExecutionID,
				Field:       w.Field,
				Value:       append([]byte(nil), w.Value...),
				Status:      ValueStatusPending,
				Timestamp:   w.Timestamp,
				Seq:         r.seq,
			})
		case OpStatusFlip:
			f := op.StatusFlip
			for i := range r.values {
				if r.values[i].TaskID == f.TaskID && r.values[i].Status == ValueStatusPending {
					r.values[i].Status = f.NewStatus
				}
			}
		case OpHistoryEntry:
			h := op.HistoryEntry
			r.seq++
			r.history = append(r.history, HistoryRecord{
				Kind:        h.Kind,
				NodeID:      h.NodeID,
				ExecutionID: h.ExecutionID,
				Payload:     append([]byte(nil), h.Payload...),
				Timestamp:   h.Timestamp,
				Seq:         r.seq,
			})
		}
	}
	return nil
}

// Flush is a no-op: writes are applied synchronously in memory.
func (s *InMemoryStateStore) Flush(runID string) error { return nil }

// CurrentVersion folds the run's value records into the latest committed
// value per (task, field). Returns (nil, nil) for an unknown run.
func (s *InMemoryStateStore) CurrentVersion(runID string) (*CurrentVersion, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	r, ok := s.runs[runID]
	if !ok {
		return nil, nil
	}
	cv := &CurrentVersion{Meta: r.meta, Values: map[string]map[string][]byte{}}
	// Fold in replay order so the latest committed write per field wins.
	recs := append([]ValueRecord(nil), r.values...)
	SortValueRecords(recs)
	for _, rec := range recs {
		if rec.Status != ValueStatusCommitted {
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

// FullHistory returns every stored record for the run — pending and aborted
// included — in replay order. Returns (nil, nil) for an unknown run.
func (s *InMemoryStateStore) FullHistory(runID string) (*FullHistory, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	r, ok := s.runs[runID]
	if !ok {
		return nil, nil
	}
	fh := &FullHistory{
		Meta:    r.meta,
		Values:  append([]ValueRecord(nil), r.values...),
		History: append([]HistoryRecord(nil), r.history...),
	}
	SortValueRecords(fh.Values)
	SortHistoryRecords(fh.History)
	return fh, nil
}

// Config returns an error: in-memory state cannot be reached from another
// process.
func (s *InMemoryStateStore) Config() (map[string]string, error) {
	return nil, fmt.Errorf("in-memory state store cannot be shared across processes")
}

// Close releases nothing; it exists to satisfy StateStore.
func (s *InMemoryStateStore) Close() error { return nil }

var _ StateStore = (*InMemoryStateStore)(nil)
