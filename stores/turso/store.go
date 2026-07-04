// Package turso is the Turso Database state-store backend for blkit: a
// durable, embedded store keeping each run's ProcessState in a Turso
// database file on local disk. Turso Database is the ground-up rewrite of
// SQLite in Rust (formerly "Limbo") — in-process, SQLite-compatible in query
// language and file format, and currently in BETA. The driver is pure Go
// (purego + a bundled platform library; no CGO at build time).
// See specs/stores/turso-state-store.spec.md.
package turso

import (
	"database/sql"
	"fmt"
	"regexp"
	"sync"
	"time"

	_ "turso.tech/database/tursogo" // registers the "turso" database/sql driver

	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the Turso backend.
type Config struct {
	Path string // path to the database file; required

	// TablePrefix is the table-name prefix; defaults to "blkit_".
	TablePrefix string
}

// Store implements bl.StateStore on a Turso database file. All access is
// funnelled through a single connection — the engine is in beta, and
// serialising access through one connection keeps its concurrency surface
// out of play.
type Store struct {
	db     *sql.DB
	prefix string

	schemaOnce sync.Once
	schemaErr  error
}

var _ bl.StateStore = (*Store)(nil)

var prefixPattern = regexp.MustCompile(`^[a-z_][a-z0-9_]*$`)

// New opens (creating if needed) the database file and returns the store.
// Following the blkit constructor convention it panics on an invalid config
// or an unopenable file.
func New(cfg Config) *Store {
	if cfg.Path == "" {
		panic(fmt.Errorf("turso state store: Path is required"))
	}
	prefix := cfg.TablePrefix
	if prefix == "" {
		prefix = "blkit_"
	}
	if !prefixPattern.MatchString(prefix) {
		panic(fmt.Errorf("turso state store: invalid TablePrefix %q", cfg.TablePrefix))
	}
	db, err := sql.Open("turso", cfg.Path)
	if err != nil {
		panic(fmt.Errorf("turso state store: open %s: %w", cfg.Path, err))
	}
	db.SetMaxOpenConns(1) // serialise all access through one connection
	return &Store{db: db, prefix: prefix}
}

func (s *Store) table(name string) string { return s.prefix + name }

// ensureSchema creates the schema on first use. The DDL is deliberately
// conservative — plain composite indexes, no partial indexes — to stay well
// inside the beta engine's supported SQLite surface.
func (s *Store) ensureSchema() error {
	s.schemaOnce.Do(func() {
		stmts := []string{
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				run_id           TEXT PRIMARY KEY,
				process_id       TEXT NOT NULL,
				process_version  TEXT NOT NULL,
				status           TEXT NOT NULL,
				published_at     INTEGER,
				started_at       INTEGER,
				completed_at     INTEGER,
				evaluation_count INTEGER NOT NULL DEFAULT 0
			)`, s.table("runs")),
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				id           INTEGER PRIMARY KEY,
				run_id       TEXT NOT NULL,
				task_id      TEXT NOT NULL,
				execution_id TEXT NOT NULL,
				field        TEXT NOT NULL,
				value        TEXT NOT NULL,
				status       TEXT NOT NULL DEFAULT 'pending',
				ts           INTEGER NOT NULL
			)`, s.table("values")),
			fmt.Sprintf(`CREATE INDEX IF NOT EXISTS %s_replay
				ON %s (run_id, ts, id)`, s.table("values"), s.table("values")),
			fmt.Sprintf(`CREATE INDEX IF NOT EXISTS %s_pending
				ON %s (run_id, task_id, status)`, s.table("values"), s.table("values")),
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				id           INTEGER PRIMARY KEY,
				run_id       TEXT NOT NULL,
				kind         TEXT NOT NULL,
				node_id      TEXT,
				execution_id TEXT NOT NULL,
				payload      TEXT NOT NULL,
				ts           INTEGER NOT NULL
			)`, s.table("history")),
			fmt.Sprintf(`CREATE INDEX IF NOT EXISTS %s_replay
				ON %s (run_id, ts, id)`, s.table("history"), s.table("history")),
		}
		for _, stmt := range stmts {
			if _, err := s.db.Exec(stmt); err != nil {
				s.schemaErr = fmt.Errorf("turso state store: schema: %w", err)
				return
			}
		}
	})
	return s.schemaErr
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

// Save upserts the run's metadata. The upsert is UPDATE-then-INSERT inside a
// transaction rather than ON CONFLICT — plain UPDATE/INSERT is the most
// conservative SQLite surface, and the single connection makes it raceless.
func (s *Store) Save(meta bl.RunMetadata) error {
	if meta.RunID == "" {
		return fmt.Errorf("save: empty RunID")
	}
	if err := s.ensureSchema(); err != nil {
		return err
	}
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	res, err := tx.Exec(fmt.Sprintf(`
		UPDATE %s SET
			process_id       = ?,
			process_version  = ?,
			status           = ?,
			published_at     = ?,
			started_at       = ?,
			completed_at     = ?,
			evaluation_count = ?
		WHERE run_id = ?`, s.table("runs")),
		meta.ProcessID, meta.ProcessVersion, meta.Status,
		timePtrToNanos(meta.PublishedAt), timePtrToNanos(meta.StartedAt),
		timePtrToNanos(meta.CompletedAt), meta.EvaluationCount, meta.RunID,
	)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err != nil {
		return err
	} else if n == 0 {
		if _, err := tx.Exec(fmt.Sprintf(`
			INSERT INTO %s
				(run_id, process_id, process_version, status,
				 published_at, started_at, completed_at, evaluation_count)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?)`, s.table("runs")),
			meta.RunID, meta.ProcessID, meta.ProcessVersion, meta.Status,
			timePtrToNanos(meta.PublishedAt), timePtrToNanos(meta.StartedAt),
			timePtrToNanos(meta.CompletedAt), meta.EvaluationCount,
		); err != nil {
			return err
		}
	}
	return tx.Commit()
}

// WriteBatch applies the ops in a single transaction, so a task's outputs
// land together.
func (s *Store) WriteBatch(ops []bl.WriteOp) error {
	for _, op := range ops {
		if err := bl.ValidateWriteOp(op); err != nil {
			return err
		}
	}
	if err := s.ensureSchema(); err != nil {
		return err
	}
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	for _, op := range ops {
		switch op.Kind {
		case bl.OpValueWrite:
			w := op.ValueWrite
			if _, err := tx.Exec(fmt.Sprintf(`
				INSERT INTO %s
					(run_id, task_id, execution_id, field, value, status, ts)
				VALUES (?, ?, ?, ?, ?, 'pending', ?)`, s.table("values")),
				op.RunID, w.TaskID, w.ExecutionID, w.Field, string(w.Value),
				w.Timestamp.UTC().UnixNano(),
			); err != nil {
				return err
			}
		case bl.OpStatusFlip:
			f := op.StatusFlip
			if _, err := tx.Exec(fmt.Sprintf(`
				UPDATE %s SET status = ?
				WHERE run_id = ? AND task_id = ? AND status = 'pending'`, s.table("values")),
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
			if _, err := tx.Exec(fmt.Sprintf(`
				INSERT INTO %s
					(run_id, kind, node_id, execution_id, payload, ts)
				VALUES (?, ?, ?, ?, ?, ?)`, s.table("history")),
				op.RunID, h.Kind, h.NodeID, h.ExecutionID, string(payload),
				h.Timestamp.UTC().UnixNano(),
			); err != nil {
				return err
			}
		}
	}
	return tx.Commit()
}

// Flush is a no-op: writes are synchronous once the transaction commits.
func (s *Store) Flush(runID string) error { return nil }

func (s *Store) runMeta(runID string) (meta bl.RunMetadata, exists bool, err error) {
	meta = bl.RunMetadata{RunID: runID}
	var published, started, completed sql.NullInt64
	row := s.db.QueryRow(fmt.Sprintf(`
		SELECT process_id, process_version, status,
		       published_at, started_at, completed_at, evaluation_count
		FROM %s WHERE run_id = ?`, s.table("runs")), runID)
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
		if err := s.db.QueryRow(fmt.Sprintf(`
			SELECT (SELECT COUNT(*) FROM %s WHERE run_id = ?)
			     + (SELECT COUNT(*) FROM %s WHERE run_id = ?)`,
			s.table("values"), s.table("history")), runID, runID).Scan(&n); err != nil {
			return meta, false, err
		}
		return meta, n > 0, nil
	default:
		return meta, false, scanErr
	}
}

// CurrentVersion returns the latest committed value per (task, field). The
// fold is done in Go over a replay-ordered scan rather than with a window
// function, staying inside the beta engine's supported SQL surface. Returns
// (nil, nil) for an unknown run.
func (s *Store) CurrentVersion(runID string) (*bl.CurrentVersion, error) {
	if err := s.ensureSchema(); err != nil {
		return nil, err
	}
	meta, exists, err := s.runMeta(runID)
	if err != nil || !exists {
		return nil, err
	}
	cv := &bl.CurrentVersion{Meta: meta, Values: map[string]map[string][]byte{}}
	rows, err := s.db.Query(fmt.Sprintf(`
		SELECT task_id, field, value
		FROM %s
		WHERE run_id = ? AND status = 'committed'
		ORDER BY ts, id`, s.table("values")), runID)
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
		task[field] = []byte(value) // replay order: the last write per field wins
	}
	return cv, rows.Err()
}

// FullHistory returns every stored record for the run in replay order
// (ts, id). Returns (nil, nil) for an unknown run.
func (s *Store) FullHistory(runID string) (*bl.FullHistory, error) {
	if err := s.ensureSchema(); err != nil {
		return nil, err
	}
	meta, exists, err := s.runMeta(runID)
	if err != nil || !exists {
		return nil, err
	}
	fh := &bl.FullHistory{Meta: meta}

	rows, err := s.db.Query(fmt.Sprintf(`
		SELECT id, task_id, execution_id, field, value, status, ts
		FROM %s WHERE run_id = ? ORDER BY ts, id`, s.table("values")), runID)
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

	hrows, err := s.db.Query(fmt.Sprintf(`
		SELECT id, kind, node_id, execution_id, payload, ts
		FROM %s WHERE run_id = ? ORDER BY ts, id`, s.table("history")), runID)
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

// Config returns an error: a Turso database file is local to one process on
// one machine and cannot be shared.
func (s *Store) Config() (map[string]string, error) {
	return nil, fmt.Errorf("turso state store cannot be shared across processes")
}

// Close closes the database handle.
func (s *Store) Close() error { return s.db.Close() }
