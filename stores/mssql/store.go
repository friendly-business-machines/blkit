// Package mssql is the Microsoft SQL Server state-store backend for blkit: a
// durable, strongly consistent, shareable store keeping each run's
// ProcessState in SQL Server, built on the official microsoft/go-mssqldb
// driver via database/sql. See specs/stores/mssql-state-store.spec.md.
package mssql

import (
	"database/sql"
	"fmt"
	"regexp"
	"sync"
	"time"

	_ "github.com/microsoft/go-mssqldb" // registers the "sqlserver" driver

	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the SQL Server backend.
type Config struct {
	DSN         string // sqlserver:// connection string; required
	TablePrefix string // table-name prefix; defaults to "blkit_"
}

// Store implements bl.StateStore on a SQL Server database.
type Store struct {
	db     *sql.DB
	dsn    string
	prefix string

	schemaOnce sync.Once
	schemaErr  error
}

var _ bl.StateStore = (*Store)(nil)

var prefixPattern = regexp.MustCompile(`^[a-z_][a-z0-9_]*$`)

// New opens the pool (connections are established lazily; the schema is
// created on first use). Following the blkit constructor convention it
// panics on an invalid config.
func New(cfg Config) *Store {
	if cfg.DSN == "" {
		panic(fmt.Errorf("mssql state store: DSN is required"))
	}
	prefix := cfg.TablePrefix
	if prefix == "" {
		prefix = "blkit_"
	}
	if !prefixPattern.MatchString(prefix) {
		panic(fmt.Errorf("mssql state store: invalid TablePrefix %q", cfg.TablePrefix))
	}
	db, err := sql.Open("sqlserver", cfg.DSN)
	if err != nil {
		panic(fmt.Errorf("mssql state store: open: %w", err))
	}
	return &Store{db: db, dsn: cfg.DSN, prefix: prefix}
}

func (s *Store) table(name string) string { return s.prefix + name }

func (s *Store) ensureSchema() error {
	s.schemaOnce.Do(func() {
		stmts := []string{
			fmt.Sprintf(`IF OBJECT_ID(N'%s', N'U') IS NULL
			CREATE TABLE %s (
				run_id           NVARCHAR(255) PRIMARY KEY,
				process_id       NVARCHAR(255) NOT NULL,
				process_version  NVARCHAR(64) NOT NULL,
				status           NVARCHAR(32) NOT NULL,
				published_at     DATETIMEOFFSET(7) NULL,
				started_at       DATETIMEOFFSET(7) NULL,
				completed_at     DATETIMEOFFSET(7) NULL,
				evaluation_count INT NOT NULL DEFAULT 0
			)`, s.table("runs"), s.table("runs")),
			fmt.Sprintf(`IF OBJECT_ID(N'%s', N'U') IS NULL
			CREATE TABLE %s (
				id           BIGINT IDENTITY(1,1) PRIMARY KEY,
				run_id       NVARCHAR(255) NOT NULL,
				task_id      NVARCHAR(255) NOT NULL,
				execution_id NVARCHAR(255) NOT NULL,
				field        NVARCHAR(255) NOT NULL,
				value        NVARCHAR(MAX) NOT NULL CHECK (ISJSON(value) = 1),
				status       NVARCHAR(16) NOT NULL DEFAULT 'pending',
				ts           DATETIMEOFFSET(7) NOT NULL
			)`, s.table("values"), s.table("values")),
			fmt.Sprintf(`IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = N'%s_replay')
			CREATE INDEX %s_replay ON %s (run_id, ts, id)`,
				s.table("values"), s.table("values"), s.table("values")),
			fmt.Sprintf(`IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = N'%s_pending')
			CREATE INDEX %s_pending ON %s (run_id, task_id) WHERE status = 'pending'`,
				s.table("values"), s.table("values"), s.table("values")),
			fmt.Sprintf(`IF OBJECT_ID(N'%s', N'U') IS NULL
			CREATE TABLE %s (
				id           BIGINT IDENTITY(1,1) PRIMARY KEY,
				run_id       NVARCHAR(255) NOT NULL,
				kind         NVARCHAR(64) NOT NULL,
				node_id      NVARCHAR(255) NULL,
				execution_id NVARCHAR(255) NOT NULL,
				payload      NVARCHAR(MAX) NOT NULL CHECK (ISJSON(payload) = 1),
				ts           DATETIMEOFFSET(7) NOT NULL
			)`, s.table("history"), s.table("history")),
			fmt.Sprintf(`IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = N'%s_replay')
			CREATE INDEX %s_replay ON %s (run_id, ts, id)`,
				s.table("history"), s.table("history"), s.table("history")),
		}
		for _, stmt := range stmts {
			if _, err := s.db.Exec(stmt); err != nil {
				s.schemaErr = fmt.Errorf("mssql state store: schema: %w", err)
				return
			}
		}
	})
	return s.schemaErr
}

func utcPtr(t *time.Time) *time.Time {
	if t == nil {
		return nil
	}
	u := t.UTC()
	return &u
}

// Save upserts the run's metadata via MERGE.
func (s *Store) Save(meta bl.RunMetadata) error {
	if meta.RunID == "" {
		return fmt.Errorf("save: empty RunID")
	}
	if err := s.ensureSchema(); err != nil {
		return err
	}
	_, err := s.db.Exec(fmt.Sprintf(`
		MERGE %s AS t
		USING (SELECT @p1 AS run_id) AS src ON t.run_id = src.run_id
		WHEN MATCHED THEN UPDATE SET
			process_id       = @p2,
			process_version  = @p3,
			status           = @p4,
			published_at     = @p5,
			started_at       = @p6,
			completed_at     = @p7,
			evaluation_count = @p8
		WHEN NOT MATCHED THEN INSERT
			(run_id, process_id, process_version, status,
			 published_at, started_at, completed_at, evaluation_count)
			VALUES (@p1, @p2, @p3, @p4, @p5, @p6, @p7, @p8);`, s.table("runs")),
		meta.RunID, meta.ProcessID, meta.ProcessVersion, meta.Status,
		utcPtr(meta.PublishedAt), utcPtr(meta.StartedAt), utcPtr(meta.CompletedAt),
		meta.EvaluationCount,
	)
	return err
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
				INSERT INTO %s (run_id, task_id, execution_id, field, value, status, ts)
				VALUES (@p1, @p2, @p3, @p4, @p5, 'pending', @p6)`, s.table("values")),
				op.RunID, w.TaskID, w.ExecutionID, w.Field, string(w.Value),
				w.Timestamp.UTC(),
			); err != nil {
				return err
			}
		case bl.OpStatusFlip:
			f := op.StatusFlip
			if _, err := tx.Exec(fmt.Sprintf(`
				UPDATE %s SET status = @p1
				WHERE run_id = @p2 AND task_id = @p3 AND status = 'pending'`, s.table("values")),
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
				INSERT INTO %s (run_id, kind, node_id, execution_id, payload, ts)
				VALUES (@p1, @p2, @p3, @p4, @p5, @p6)`, s.table("history")),
				op.RunID, h.Kind, h.NodeID, h.ExecutionID, string(payload),
				h.Timestamp.UTC(),
			); err != nil {
				return err
			}
		}
	}
	return tx.Commit()
}

// Flush is a no-op beyond confirming the transaction committed: writes are
// synchronous once WriteBatch returns.
func (s *Store) Flush(runID string) error { return nil }

func (s *Store) runMeta(runID string) (meta bl.RunMetadata, exists bool, err error) {
	meta = bl.RunMetadata{RunID: runID}
	var published, started, completed sql.NullTime
	row := s.db.QueryRow(fmt.Sprintf(`
		SELECT process_id, process_version, status,
		       published_at, started_at, completed_at, evaluation_count
		FROM %s WHERE run_id = @p1`, s.table("runs")), runID)
	scanErr := row.Scan(&meta.ProcessID, &meta.ProcessVersion, &meta.Status,
		&published, &started, &completed, &meta.EvaluationCount)
	switch scanErr {
	case nil:
		meta.PublishedAt = nullTimePtr(published)
		meta.StartedAt = nullTimePtr(started)
		meta.CompletedAt = nullTimePtr(completed)
		return meta, true, nil
	case sql.ErrNoRows:
		var n int
		if err := s.db.QueryRow(fmt.Sprintf(`
			SELECT (SELECT COUNT(*) FROM %s WHERE run_id = @p1)
			     + (SELECT COUNT(*) FROM %s WHERE run_id = @p1)`,
			s.table("values"), s.table("history")), runID).Scan(&n); err != nil {
			return meta, false, err
		}
		return meta, n > 0, nil
	default:
		return meta, false, scanErr
	}
}

func nullTimePtr(t sql.NullTime) *time.Time {
	if !t.Valid {
		return nil
	}
	u := t.Time.UTC()
	return &u
}

// CurrentVersion returns the latest committed value per (task, field) via a
// ROW_NUMBER window. Returns (nil, nil) for an unknown run.
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
		SELECT task_id, field, value FROM (
			SELECT task_id, field, value,
			       ROW_NUMBER() OVER (
			           PARTITION BY task_id, field
			           ORDER BY ts DESC, id DESC
			       ) AS rn
			FROM %s
			WHERE run_id = @p1 AND status = 'committed'
		) latest WHERE rn = 1`, s.table("values")), runID)
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
		FROM %s WHERE run_id = @p1 ORDER BY ts, id`, s.table("values")), runID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var (
			rec    bl.ValueRecord
			value  string
			status string
			ts     time.Time
		)
		if err := rows.Scan(&rec.Seq, &rec.TaskID, &rec.ExecutionID,
			&rec.Field, &value, &status, &ts); err != nil {
			return nil, err
		}
		rec.Value = []byte(value)
		rec.Timestamp = ts.UTC()
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
		FROM %s WHERE run_id = @p1 ORDER BY ts, id`, s.table("history")), runID)
	if err != nil {
		return nil, err
	}
	defer hrows.Close()
	for hrows.Next() {
		var (
			rec     bl.HistoryRecord
			nodeID  sql.NullString
			payload string
			ts      time.Time
		)
		if err := hrows.Scan(&rec.Seq, &rec.Kind, &nodeID,
			&rec.ExecutionID, &payload, &ts); err != nil {
			return nil, err
		}
		if nodeID.Valid {
			rec.NodeID = &nodeID.String
		}
		rec.Payload = []byte(payload)
		rec.Timestamp = ts.UTC()
		fh.History = append(fh.History, rec)
	}
	return fh, hrows.Err()
}

// Config returns the connection details another process needs to reach the
// same store.
func (s *Store) Config() (map[string]string, error) {
	return map[string]string{"dsn": s.dsn, "table_prefix": s.prefix}, nil
}

// Close closes the connection pool.
func (s *Store) Close() error { return s.db.Close() }
