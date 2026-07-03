package core

import (
	"fmt"
	"sort"
	"time"
)

// This file defines the shared write contract every state-store backend
// implements — the WriteOp kinds streamed by the worker's writer pool, the
// records a backend serves back, and the StateStore interface itself. The
// contract is specified in specs/stores/overview.spec.md ("The write
// contract"); the per-backend layouts live in the sibling specs under
// specs/stores/.

// ValueStatus is the lifecycle state of a task's value write. Writes land as
// Pending and are settled by a StatusFlip to Committed (task finished
// successfully) or Aborted (task failed). Aborted writes are retained for
// audit but never surface in the current version.
type ValueStatus int

const (
	ValueStatusPending ValueStatus = iota
	ValueStatusCommitted
	ValueStatusAborted
)

func (s ValueStatus) String() string {
	switch s {
	case ValueStatusPending:
		return "pending"
	case ValueStatusCommitted:
		return "committed"
	case ValueStatusAborted:
		return "aborted"
	}
	return fmt.Sprintf("ValueStatus(%d)", int(s))
}

// ParseValueStatus is the inverse of ValueStatus.String. Backends that store
// the status as text use it when reading records back.
func ParseValueStatus(s string) (ValueStatus, error) {
	switch s {
	case "pending":
		return ValueStatusPending, nil
	case "committed":
		return ValueStatusCommitted, nil
	case "aborted":
		return ValueStatusAborted, nil
	}
	return 0, fmt.Errorf("unknown value status %q", s)
}

// WriteOpKind discriminates the three write-op payloads.
type WriteOpKind int

const (
	OpValueWrite WriteOpKind = iota
	OpStatusFlip
	OpHistoryEntry
)

// WriteOp is one unit of the write stream a backend receives. Exactly one of
// the payload pointers is set, matching Kind.
type WriteOp struct {
	RunID string
	Kind  WriteOpKind

	ValueWrite   *ValueWrite
	StatusFlip   *StatusFlip
	HistoryEntry *HistoryEntry
}

// ValueWrite is one field written by a task. It arrives with status Pending.
// Value carries the Bl value encoded as JSON.
type ValueWrite struct {
	TaskID      string    // node path, e.g. "screen" or "verify-step.check-docs"
	ExecutionID string    // distinct per task execution (loop iterations differ)
	Field       string    // output field name, e.g. "Score"
	Value       []byte    // the Bl value, JSON-encoded
	Timestamp   time.Time // set by the worker when the write was produced
}

// StatusFlip settles every Pending ValueWrite for TaskID in its run,
// atomically, to Committed or Aborted.
type StatusFlip struct {
	TaskID    string
	NewStatus ValueStatus // ValueStatusCommitted or ValueStatusAborted
	Timestamp time.Time
}

// HistoryEntry is one execution-history entry (task scheduled / started /
// finished / failed, path chosen, run started / finished). Payload carries
// the remaining entry fields (error, iteration, ...), JSON-encoded.
type HistoryEntry struct {
	Kind        string // e.g. "NODE_STARTED", "GATEWAY_RESOLVED"
	NodeID      *string
	ExecutionID string
	Payload     []byte
	Timestamp   time.Time
}

// ValueRecord is a stored ValueWrite as a backend serves it back: the write
// plus its settled status and the backend-assigned arrival number.
type ValueRecord struct {
	TaskID      string
	ExecutionID string
	Field       string
	Value       []byte
	Status      ValueStatus
	Timestamp   time.Time
	Seq         uint64 // backend-assigned arrival order; replay tiebreak
}

// HistoryRecord is a stored HistoryEntry plus its arrival number.
type HistoryRecord struct {
	Kind        string
	NodeID      *string
	ExecutionID string
	Payload     []byte
	Timestamp   time.Time
	Seq         uint64
}

// RunMetadata is the run-level metadata written directly by the executor at
// evaluation boundaries via Save (bypassing the writer pool).
type RunMetadata struct {
	RunID           string
	ProcessID       string
	ProcessVersion  string
	Status          string
	PublishedAt     *time.Time
	StartedAt       *time.Time
	CompletedAt     *time.Time
	EvaluationCount int
}

// CurrentVersion is the current version of a run's ProcessState: the latest
// committed value per (task, field), plus the run metadata. Pending and
// Aborted writes never appear here.
type CurrentVersion struct {
	Meta   RunMetadata
	Values map[string]map[string][]byte // taskID -> field -> JSON-encoded value
}

// FullHistory is everything stored for a run — every value write (any
// status) and every history entry, in replay order — for diagnostics and
// audit.
type FullHistory struct {
	Meta    RunMetadata
	Values  []ValueRecord
	History []HistoryRecord
}

// StateStore is implemented by every backend. Implementations must be safe
// for concurrent use: parallel tasks within one run and many runs at once
// all write through the same store.
//
// Reads for an unknown run id return (nil, nil) — not found is not an error.
type StateStore interface {
	// Save persists (upserts) run metadata. Called directly by the executor
	// at evaluation boundaries; also how a fresh run's metadata first lands.
	Save(meta RunMetadata) error

	// WriteBatch applies a batch of write ops. Backends apply the batch
	// atomically where the storage engine supports it (transaction, write
	// batch); engines with no batching primitive apply ops one by one.
	WriteBatch(ops []WriteOp) error

	// Flush is a per-run durability barrier: when it returns, every
	// previously accepted write op for the run is durable.
	Flush(runID string) error

	// CurrentVersion loads the current version of the run's ProcessState.
	CurrentVersion(runID string) (*CurrentVersion, error)

	// FullHistory reads everything stored for the run, in replay order.
	FullHistory(runID string) (*FullHistory, error)

	// Config returns the connection details another process needs to reach
	// the same store, or an error for stores that cannot be shared.
	Config() (map[string]string, error)

	// Close releases the store's resources.
	Close() error
}

// ValidateWriteOp checks the structural invariants of a WriteOp. Backends
// call it at the top of WriteBatch so malformed ops fail identically
// everywhere.
func ValidateWriteOp(op WriteOp) error {
	if op.RunID == "" {
		return fmt.Errorf("write op: empty RunID")
	}
	switch op.Kind {
	case OpValueWrite:
		if op.ValueWrite == nil {
			return fmt.Errorf("write op: Kind OpValueWrite with nil ValueWrite")
		}
		w := op.ValueWrite
		if w.TaskID == "" || w.Field == "" || w.ExecutionID == "" {
			return fmt.Errorf("value write: TaskID, ExecutionID and Field are required")
		}
		if w.Timestamp.IsZero() {
			return fmt.Errorf("value write: zero Timestamp")
		}
	case OpStatusFlip:
		if op.StatusFlip == nil {
			return fmt.Errorf("write op: Kind OpStatusFlip with nil StatusFlip")
		}
		f := op.StatusFlip
		if f.TaskID == "" {
			return fmt.Errorf("status flip: TaskID is required")
		}
		if f.NewStatus != ValueStatusCommitted && f.NewStatus != ValueStatusAborted {
			return fmt.Errorf("status flip: NewStatus must be committed or aborted, got %s", f.NewStatus)
		}
	case OpHistoryEntry:
		if op.HistoryEntry == nil {
			return fmt.Errorf("write op: Kind OpHistoryEntry with nil HistoryEntry")
		}
		h := op.HistoryEntry
		if h.Kind == "" {
			return fmt.Errorf("history entry: Kind is required")
		}
		if h.Timestamp.IsZero() {
			return fmt.Errorf("history entry: zero Timestamp")
		}
	default:
		return fmt.Errorf("write op: unknown kind %d", op.Kind)
	}
	return nil
}

// SortValueRecords sorts records into replay order: (Timestamp, Seq).
// Backends whose storage does not already iterate in replay order use it
// before returning FullHistory.
func SortValueRecords(recs []ValueRecord) {
	sort.SliceStable(recs, func(i, j int) bool {
		if !recs[i].Timestamp.Equal(recs[j].Timestamp) {
			return recs[i].Timestamp.Before(recs[j].Timestamp)
		}
		return recs[i].Seq < recs[j].Seq
	})
}

// SortHistoryRecords sorts records into replay order: (Timestamp, Seq).
func SortHistoryRecords(recs []HistoryRecord) {
	sort.SliceStable(recs, func(i, j int) bool {
		if !recs[i].Timestamp.Equal(recs[j].Timestamp) {
			return recs[i].Timestamp.Before(recs[j].Timestamp)
		}
		return recs[i].Seq < recs[j].Seq
	})
}
