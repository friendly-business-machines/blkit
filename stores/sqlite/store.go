// Package sqlite is the SQLite state-store backend for blkit: a durable,
// embedded store keeping each run's ProcessState in a single SQL-queryable
// file on local disk, built on the pure-Go modernc.org/sqlite driver so
// CGO_ENABLED=0 static builds keep working.
// See specs/state-stores/sqlite-state-store.spec.md.
package sqlite

import (
	"database/sql"
	"fmt"
	"time"

	bl "github.com/friendly-business-machines/blkit/core"
	_ "modernc.org/sqlite" // registers the "sqlite" database/sql driver
)

// Config holds the construction parameters for the SQLite backend.
type Config struct {
	Path string // path to the database file; required
}

// Store implements bl.StateStore on a SQLite database file. Writes are
// funnelled through a single-connection handle (SQLite permits one writer at
// a time); reads use a separate pooled handle, and WAL mode keeps them
// flowing while a write is in progress.
type Store struct {
	write *sql.DB
	read  *sql.DB
}

var _ bl.StateStore = (*Store)(nil)

// New opens (creating if needed) the database file, applies the PRAGMAs, and
// creates the schema. Following the blkit constructor convention it panics on
// an invalid config or an unopenable file.
func New(cfg Config) *Store {
	if cfg.Path == "" {
		panic(fmt.Errorf("sqlite state store: Path is required"))
	}
	dsn := "file:" + cfg.Path +
		"?_pragma=journal_mode(WAL)&_pragma=busy_timeout(5000)&_pragma=synchronous(FULL)"
	write, err := sql.Open("sqlite", dsn)
	if err != nil {
		panic(fmt.Errorf("sqlite state store: open %s: %w", cfg.Path, err))
	}
	write.SetMaxOpenConns(1) // the single writer connection
	read, err := sql.Open("sqlite", dsn)
	if err != nil {
		write.Close()
		panic(fmt.Errorf("sqlite state store: open %s: %w", cfg.Path, err))
	}
	s := &Store{write: write, read: read}
	if err := s.ensureSchema(); err != nil {
		s.Close()
		panic(fmt.Errorf("sqlite state store: schema: %w", err))
	}
	return s
}

func (s *Store) ensureSchema() error {
	stmts := []string{
		`CREATE TABLE IF NOT EXISTS blkit_runs (
			run_id           TEXT PRIMARY KEY,
			process_id       TEXT NOT NULL,
			process_version  TEXT NOT NULL,
			status           TEXT NOT NULL,
			published_at     INTEGER,
			started_at       INTEGER,
			completed_at     INTEGER,
			evaluation_count INTEGER NOT NULL DEFAULT 0
		)`,
		`CREATE TABLE IF NOT EXISTS blkit_values (
			id           INTEGER PRIMARY KEY,
			run_id       TEXT NOT NULL,
			task_id      TEXT NOT NULL,
			execution_id TEXT NOT NULL,
			field        TEXT NOT NULL,
			value        TEXT NOT NULL,
			status       TEXT NOT NULL DEFAULT 'pending',
			ts           INTEGER NOT NULL
		)`,
		`CREATE INDEX IF NOT EXISTS blkit_values_replay
			ON blkit_values (run_id, ts, id)`,
		`CREATE INDEX IF NOT EXISTS blkit_values_pending
			ON blkit_values (run_id, task_id) WHERE status = 'pending'`,
		`CREATE TABLE IF NOT EXISTS blkit_history (
			id           INTEGER PRIMARY KEY,
			run_id       TEXT NOT NULL,
			kind         TEXT NOT NULL,
			node_id      TEXT,
			execution_id TEXT NOT NULL,
			payload      TEXT NOT NULL,
			ts           INTEGER NOT NULL
		)`,
		`CREATE INDEX IF NOT EXISTS blkit_history_replay
			ON blkit_history (run_id, ts, id)`,
	}
	for _, stmt := range stmts {
		if _, err := s.write.Exec(stmt); err != nil {
			return err
		}
	}
	return nil
}

func timePtrToNanos(t *time.Time) any {
	if t == nil {
		return nil
	}
	return t.UTC().UnixNano()
}

func nanosToTimePtr(n sql.NullInt64) *time.Time {
	if !n.Valid {
		return nil
	}
	t := time.Unix(0, n.Int64).UTC()
	return &t
}

// Save upserts the run's metadata.
func (s *Store) Save(meta bl.RunMetadata) error {
	if meta.RunID == "" {
		return fmt.Errorf("save: empty RunID")
	}
	_, err := s.write.Exec(`
		INSERT INTO blkit_runs
			(run_id, process_id, process_version, status,
			 published_at, started_at, completed_at, evaluation_count)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT (run_id) DO UPDATE SET
			process_id       = excluded.process_id,
			process_version  = excluded.process_version,
			status           = excluded.status,
			published_at     = excluded.published_at,
			started_at       = excluded.started_at,
			completed_at     = excluded.completed_at,
			evaluation_count = excluded.evaluation_count`,
		meta.RunID, meta.ProcessID, meta.ProcessVersion, meta.Status,
		timePtrToNanos(meta.PublishedAt), timePtrToNanos(meta.StartedAt),
		timePtrToNanos(meta.CompletedAt), meta.EvaluationCount,
	)
	return err
}

// WriteBatch applies the ops in a single transaction on the writer
// connection, so a task's outputs land together.
func (s *Store) WriteBatch(ops []bl.WriteOp) error {
	for _, op := range ops {
		if err := bl.ValidateWriteOp(op); err != nil {
			return err
		}
	}
	tx, err := s.write.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	for _, op := range ops {
		switch op.Kind {
		case bl.OpValueWrite:
			w := op.ValueWrite
			if _, err := tx.Exec(`
				INSERT INTO blkit_values
					(run_id, task_id, execution_id, field, value, status, ts)
				VALUES (?, ?, ?, ?, ?, 'pending', ?)`,
				op.RunID, w.TaskID, w.ExecutionID, w.Field, string(w.Value),
				w.Timestamp.UTC().UnixNano(),
			); err != nil {
				return err
			}
		case bl.OpStatusFlip:
			f := op.StatusFlip
			if _, err := tx.Exec(`
				UPDATE blkit_values SET status = ?
				WHERE run_id = ? AND task_id = ? AND status = 'pending'`,
				f.NewStatus.String(), op.RunID, f.TaskID,
			); err != nil {
				return err
			}
		case bl.OpHistoryEntry:
			h := op.HistoryEntry
			payload := h.Payload
			if len(payload) == 0 {
				payload = []byte("null")
			}
			if _, err := tx.Exec(`
				INSERT INTO blkit_history
					(run_id, kind, node_id, execution_id, payload, ts)
				VALUES (?, ?, ?, ?, ?, ?)`,
				op.RunID, h.Kind, h.NodeID, h.ExecutionID, string(payload),
				h.Timestamp.UTC().UnixNano(),
			); err != nil {
				return err
			}
		}
	}
	return tx.Commit()
}

// Flush is a no-op: the store runs with synchronous(FULL), so every committed
// transaction is already durable.
func (s *Store) Flush(runID string) error { return nil }

// runExists reports whether the store has ever seen the run — via Save or via
// its first WriteBatch.
func (s *Store) runMeta(runID string) (meta bl.RunMetadata, exists bool, err error) {
	meta = bl.RunMetadata{RunID: runID}
	var published, started, completed sql.NullInt64
	row := s.read.QueryRow(`
		SELECT process_id, process_version, status,
		       published_at, started_at, completed_at, evaluation_count
		FROM blkit_runs WHERE run_id = ?`, runID)
	scanErr := row.Scan(&meta.ProcessID, &meta.ProcessVersion, &meta.Status,
		&published, &started, &completed, &meta.EvaluationCount)
	switch scanErr {
	case nil:
		meta.PublishedAt = nanosToTimePtr(published)
		meta.StartedAt = nanosToTimePtr(started)
		meta.CompletedAt = nanosToTimePtr(completed)
		return meta, true, nil
	case sql.ErrNoRows:
		var n int
		if err := s.read.QueryRow(`
			SELECT (SELECT COUNT(*) FROM blkit_values WHERE run_id = ?)
			     + (SELECT COUNT(*) FROM blkit_history WHERE run_id = ?)`,
			runID, runID).Scan(&n); err != nil {
			return meta, false, err
		}
		return meta, n > 0, nil
	default:
		return meta, false, scanErr
	}
}

// CurrentVersion returns the latest committed value per (task, field) plus
// the run metadata. Returns (nil, nil) for an unknown run.
func (s *Store) CurrentVersion(runID string) (*bl.CurrentVersion, error) {
	meta, exists, err := s.runMeta(runID)
	if err != nil {
		return nil, err
	}
	if !exists {
		return nil, nil
	}
	cv := &bl.CurrentVersion{Meta: meta, Values: map[string]map[string][]byte{}}
	rows, err := s.read.Query(`
		SELECT task_id, field, value FROM (
			SELECT task_id, field, value,
			       ROW_NUMBER() OVER (
			           PARTITION BY task_id, field
			           ORDER BY ts DESC, id DESC
			       ) AS rn
			FROM blkit_values
			WHERE run_id = ? AND status = 'committed'
		) WHERE rn = 1`, runID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var taskID, field, value string
		if err := rows.Scan(&taskID, &field, &value); err != nil {
			return nil, err
		}
		task, ok := cv.Values[taskID]
		if !ok {
			task = map[string][]byte{}
			cv.Values[taskID] = task
		}
		task[field] = []byte(value)
	}
	return cv, rows.Err()
}

// FullHistory returns every stored record for the run in replay order
// (ts, id). Returns (nil, nil) for an unknown run.
func (s *Store) FullHistory(runID string) (*bl.FullHistory, error) {
	meta, exists, err := s.runMeta(runID)
	if err != nil {
		return nil, err
	}
	if !exists {
		return nil, nil
	}
	fh := &bl.FullHistory{Meta: meta}

	rows, err := s.read.Query(`
		SELECT id, task_id, execution_id, field, value, status, ts
		FROM blkit_values WHERE run_id = ? ORDER BY ts, id`, runID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var (
			rec    bl.ValueRecord
			value  string
			status string
			nanos  int64
		)
		if err := rows.Scan(&rec.Seq, &rec.TaskID, &rec.ExecutionID,
			&rec.Field, &value, &status, &nanos); err != nil {
			return nil, err
		}
		rec.Value = []byte(value)
		rec.Timestamp = time.Unix(0, nanos).UTC()
		if rec.Status, err = bl.ParseValueStatus(status); err != nil {
			return nil, err
		}
		fh.Values = append(fh.Values, rec)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	hrows, err := s.read.Query(`
		SELECT id, kind, node_id, execution_id, payload, ts
		FROM blkit_history WHERE run_id = ? ORDER BY ts, id`, runID)
	if err != nil {
		return nil, err
	}
	defer hrows.Close()
	for hrows.Next() {
		var (
			rec     bl.HistoryRecord
			nodeID  sql.NullString
			payload string
			nanos   int64
		)
		if err := hrows.Scan(&rec.Seq, &rec.Kind, &nodeID,
			&rec.ExecutionID, &payload, &nanos); err != nil {
			return nil, err
		}
		if nodeID.Valid {
			rec.NodeID = &nodeID.String
		}
		rec.Payload = []byte(payload)
		rec.Timestamp = time.Unix(0, nanos).UTC()
		fh.History = append(fh.History, rec)
	}
	return fh, hrows.Err()
}

// Config returns an error: a SQLite file is local to one process on one
// machine and cannot be shared.
func (s *Store) Config() (map[string]string, error) {
	return nil, fmt.Errorf("sqlite state store cannot be shared across processes")
}

// Close closes both database handles.
func (s *Store) Close() error {
	rerr := s.read.Close()
	werr := s.write.Close()
	if werr != nil {
		return werr
	}
	return rerr
}
