// Package postgres is the PostgreSQL state-store backend for blkit: a
// durable, strongly consistent, shareable store keeping each run's
// ProcessState in PostgreSQL, built on pgx/v5 with pgx.Batch pipelining.
// See specs/state-stores/postgres-state-store.spec.md.
package postgres

import (
	"context"
	"fmt"
	"regexp"
	"sync"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the PostgreSQL backend.
type Config struct {
	DSN         string // PostgreSQL connection string; required
	TablePrefix string // table-name prefix; defaults to "blkit_"
}

// Store implements bl.StateStore on a PostgreSQL database.
type Store struct {
	pool   *pgxpool.Pool
	dsn    string
	prefix string

	schemaOnce sync.Once
	schemaErr  error
}

var _ bl.StateStore = (*Store)(nil)

var prefixPattern = regexp.MustCompile(`^[a-z_][a-z0-9_]*$`)

const opTimeout = 30 * time.Second

// New parses the config and creates the connection pool (connections are
// established lazily; the schema is created on first use). Following the
// blkit constructor convention it panics on an invalid config.
func New(cfg Config) *Store {
	if cfg.DSN == "" {
		panic(fmt.Errorf("postgres state store: DSN is required"))
	}
	prefix := cfg.TablePrefix
	if prefix == "" {
		prefix = "blkit_"
	}
	if !prefixPattern.MatchString(prefix) {
		panic(fmt.Errorf("postgres state store: invalid TablePrefix %q", cfg.TablePrefix))
	}
	poolCfg, err := pgxpool.ParseConfig(cfg.DSN)
	if err != nil {
		panic(fmt.Errorf("postgres state store: parse DSN: %w", err))
	}
	pool, err := pgxpool.NewWithConfig(context.Background(), poolCfg)
	if err != nil {
		panic(fmt.Errorf("postgres state store: pool: %w", err))
	}
	return &Store{pool: pool, dsn: cfg.DSN, prefix: prefix}
}

func (s *Store) table(name string) string { return s.prefix + name }

func (s *Store) ensureSchema(ctx context.Context) error {
	s.schemaOnce.Do(func() {
		stmts := []string{
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				run_id           TEXT PRIMARY KEY,
				process_id       TEXT NOT NULL,
				process_version  TEXT NOT NULL,
				status           TEXT NOT NULL,
				published_at     TIMESTAMPTZ,
				started_at       TIMESTAMPTZ,
				completed_at     TIMESTAMPTZ,
				evaluation_count INTEGER NOT NULL DEFAULT 0
			)`, s.table("runs")),
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				id           BIGSERIAL PRIMARY KEY,
				run_id       TEXT NOT NULL,
				task_id      TEXT NOT NULL,
				execution_id TEXT NOT NULL,
				field        TEXT NOT NULL,
				value        JSONB NOT NULL,
				status       TEXT NOT NULL DEFAULT 'pending',
				ts           TIMESTAMPTZ NOT NULL
			)`, s.table("values")),
			fmt.Sprintf(`CREATE INDEX IF NOT EXISTS %s_replay ON %s (run_id, ts, id)`,
				s.table("values"), s.table("values")),
			fmt.Sprintf(`CREATE INDEX IF NOT EXISTS %s_pending ON %s (run_id, task_id) WHERE status = 'pending'`,
				s.table("values"), s.table("values")),
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				id           BIGSERIAL PRIMARY KEY,
				run_id       TEXT NOT NULL,
				kind         TEXT NOT NULL,
				node_id      TEXT,
				execution_id TEXT NOT NULL,
				payload      JSONB NOT NULL,
				ts           TIMESTAMPTZ NOT NULL
			)`, s.table("history")),
			fmt.Sprintf(`CREATE INDEX IF NOT EXISTS %s_replay ON %s (run_id, ts, id)`,
				s.table("history"), s.table("history")),
		}
		for _, stmt := range stmts {
			if _, err := s.pool.Exec(ctx, stmt); err != nil {
				s.schemaErr = fmt.Errorf("postgres state store: schema: %w", err)
				return
			}
		}
	})
	return s.schemaErr
}

// Save upserts the run's metadata.
func (s *Store) Save(meta bl.RunMetadata) error {
	if meta.RunID == "" {
		return fmt.Errorf("save: empty RunID")
	}
	ctx, cancel := context.WithTimeout(context.Background(), opTimeout)
	defer cancel()
	if err := s.ensureSchema(ctx); err != nil {
		return err
	}
	_, err := s.pool.Exec(ctx, fmt.Sprintf(`
		INSERT INTO %s
			(run_id, process_id, process_version, status,
			 published_at, started_at, completed_at, evaluation_count)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
		ON CONFLICT (run_id) DO UPDATE SET
			process_id       = EXCLUDED.process_id,
			process_version  = EXCLUDED.process_version,
			status           = EXCLUDED.status,
			published_at     = EXCLUDED.published_at,
			started_at       = EXCLUDED.started_at,
			completed_at     = EXCLUDED.completed_at,
			evaluation_count = EXCLUDED.evaluation_count`, s.table("runs")),
		meta.RunID, meta.ProcessID, meta.ProcessVersion, meta.Status,
		meta.PublishedAt, meta.StartedAt, meta.CompletedAt, meta.EvaluationCount,
	)
	return err
}

// WriteBatch sends the whole batch as one pipelined round-trip inside a
// single transaction via pgx.Batch — exactly the shape the worker's writer
// pool produces.
func (s *Store) WriteBatch(ops []bl.WriteOp) error {
	for _, op := range ops {
		if err := bl.ValidateWriteOp(op); err != nil {
			return err
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), opTimeout)
	defer cancel()
	if err := s.ensureSchema(ctx); err != nil {
		return err
	}
	b := &pgx.Batch{}
	for _, op := range ops {
		switch op.Kind {
		case bl.OpValueWrite:
			w := op.ValueWrite
			b.Queue(fmt.Sprintf(`
				INSERT INTO %s (run_id, task_id, execution_id, field, value, status, ts)
				VALUES ($1, $2, $3, $4, $5::jsonb, 'pending', $6)`, s.table("values")),
				op.RunID, w.TaskID, w.ExecutionID, w.Field, string(w.Value), w.Timestamp.UTC())
		case bl.OpStatusFlip:
			f := op.StatusFlip
			b.Queue(fmt.Sprintf(`
				UPDATE %s SET status = $1
				WHERE run_id = $2 AND task_id = $3 AND status = 'pending'`, s.table("values")),
				f.NewStatus.String(), op.RunID, f.TaskID)
		case bl.OpHistoryEntry:
			h := op.HistoryEntry
			payload := h.Payload
			if len(payload) == 0 {
				payload = []byte("null")
			}
			b.Queue(fmt.Sprintf(`
				INSERT INTO %s (run_id, kind, node_id, execution_id, payload, ts)
				VALUES ($1, $2, $3, $4, $5::jsonb, $6)`, s.table("history")),
				op.RunID, h.Kind, h.NodeID, h.ExecutionID, string(payload), h.Timestamp.UTC())
		}
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)
	br := tx.SendBatch(ctx, b)
	for range ops {
		if _, err := br.Exec(); err != nil {
			br.Close()
			return err
		}
	}
	if err := br.Close(); err != nil {
		return err
	}
	return tx.Commit(ctx)
}

// Flush is a no-op beyond confirming the transaction committed: writes are
// synchronous once WriteBatch returns.
func (s *Store) Flush(runID string) error { return nil }

func (s *Store) runMeta(ctx context.Context, runID string) (meta bl.RunMetadata, exists bool, err error) {
	meta = bl.RunMetadata{RunID: runID}
	row := s.pool.QueryRow(ctx, fmt.Sprintf(`
		SELECT process_id, process_version, status,
		       published_at, started_at, completed_at, evaluation_count
		FROM %s WHERE run_id = $1`, s.table("runs")), runID)
	scanErr := row.Scan(&meta.ProcessID, &meta.ProcessVersion, &meta.Status,
		&meta.PublishedAt, &meta.StartedAt, &meta.CompletedAt, &meta.EvaluationCount)
	switch scanErr {
	case nil:
		normalizeMetaTimes(&meta)
		return meta, true, nil
	case pgx.ErrNoRows:
		var n int
		if err := s.pool.QueryRow(ctx, fmt.Sprintf(`
			SELECT (SELECT COUNT(*) FROM %s WHERE run_id = $1)
			     + (SELECT COUNT(*) FROM %s WHERE run_id = $1)`,
			s.table("values"), s.table("history")), runID).Scan(&n); err != nil {
			return meta, false, err
		}
		return meta, n > 0, nil
	default:
		return meta, false, scanErr
	}
}

func normalizeMetaTimes(meta *bl.RunMetadata) {
	for _, t := range []**time.Time{&meta.PublishedAt, &meta.StartedAt, &meta.CompletedAt} {
		if *t != nil {
			utc := (**t).UTC()
			*t = &utc
		}
	}
}

// CurrentVersion returns the latest committed value per (task, field) plus
// the run metadata, via DISTINCT ON. Returns (nil, nil) for an unknown run.
func (s *Store) CurrentVersion(runID string) (*bl.CurrentVersion, error) {
	ctx, cancel := context.WithTimeout(context.Background(), opTimeout)
	defer cancel()
	if err := s.ensureSchema(ctx); err != nil {
		return nil, err
	}
	meta, exists, err := s.runMeta(ctx, runID)
	if err != nil || !exists {
		return nil, err
	}
	cv := &bl.CurrentVersion{Meta: meta, Values: map[string]map[string][]byte{}}
	rows, err := s.pool.Query(ctx, fmt.Sprintf(`
		SELECT DISTINCT ON (task_id, field) task_id, field, value::text
		FROM %s
		WHERE run_id = $1 AND status = 'committed'
		ORDER BY task_id, field, ts DESC, id DESC`, s.table("values")), runID)
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
	ctx, cancel := context.WithTimeout(context.Background(), opTimeout)
	defer cancel()
	if err := s.ensureSchema(ctx); err != nil {
		return nil, err
	}
	meta, exists, err := s.runMeta(ctx, runID)
	if err != nil || !exists {
		return nil, err
	}
	fh := &bl.FullHistory{Meta: meta}

	rows, err := s.pool.Query(ctx, fmt.Sprintf(`
		SELECT id, task_id, execution_id, field, value::text, status, ts
		FROM %s WHERE run_id = $1 ORDER BY ts, id`, s.table("values")), runID)
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

	hrows, err := s.pool.Query(ctx, fmt.Sprintf(`
		SELECT id, kind, node_id, execution_id, payload::text, ts
		FROM %s WHERE run_id = $1 ORDER BY ts, id`, s.table("history")), runID)
	if err != nil {
		return nil, err
	}
	defer hrows.Close()
	for hrows.Next() {
		var (
			rec     bl.HistoryRecord
			payload string
			ts      time.Time
		)
		if err := hrows.Scan(&rec.Seq, &rec.Kind, &rec.NodeID,
			&rec.ExecutionID, &payload, &ts); err != nil {
			return nil, err
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
func (s *Store) Close() error {
	s.pool.Close()
	return nil
}
