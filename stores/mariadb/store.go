// Package mariadb is the MariaDB state-store backend for blkit: a durable,
// shareable store keeping each run's ProcessState in MariaDB. It shares the
// go-sql-driver/mysql driver with the MySQL backend but is tuned for
// MariaDB's own features — INSERT ... RETURNING (10.5+) and LONGTEXT with a
// JSON_VALID check in place of a native JSON type.
// See specs/state-stores/mariadb-state-store.spec.md.
package mariadb

import (
	"database/sql"
	"fmt"
	"regexp"
	"sync"
	"time"

	gomysql "github.com/go-sql-driver/mysql"

	bl "github.com/friendly-business-machines/blkit/core"
)

// Config holds the construction parameters for the MariaDB backend.
type Config struct {
	DSN         string // go-sql-driver DSN, e.g. "user:pass@tcp(host:3306)/blkit"; required
	TablePrefix string // table-name prefix; defaults to "blkit_"
}

// Store implements bl.StateStore on a MariaDB database.
type Store struct {
	db     *sql.DB
	dsn    string
	prefix string

	schemaOnce sync.Once
	schemaErr  error
}

var _ bl.StateStore = (*Store)(nil)

var prefixPattern = regexp.MustCompile(`^[a-z_][a-z0-9_]*$`)

// New parses the DSN (forcing ParseTime and UTC so DATETIME(6) columns
// round-trip as UTC time.Time values) and opens the pool lazily. Following
// the blkit constructor convention it panics on an invalid config.
func New(cfg Config) *Store {
	if cfg.DSN == "" {
		panic(fmt.Errorf("mariadb state store: DSN is required"))
	}
	prefix := cfg.TablePrefix
	if prefix == "" {
		prefix = "blkit_"
	}
	if !prefixPattern.MatchString(prefix) {
		panic(fmt.Errorf("mariadb state store: invalid TablePrefix %q", cfg.TablePrefix))
	}
	parsed, err := gomysql.ParseDSN(cfg.DSN)
	if err != nil {
		panic(fmt.Errorf("mariadb state store: parse DSN: %w", err))
	}
	parsed.ParseTime = true
	parsed.Loc = time.UTC
	db, err := sql.Open("mysql", parsed.FormatDSN())
	if err != nil {
		panic(fmt.Errorf("mariadb state store: open: %w", err))
	}
	return &Store{db: db, dsn: cfg.DSN, prefix: prefix}
}

func (s *Store) table(name string) string { return s.prefix + name }

func (s *Store) ensureSchema() error {
	s.schemaOnce.Do(func() {
		stmts := []string{
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				run_id           VARCHAR(255) PRIMARY KEY,
				process_id       VARCHAR(255) NOT NULL,
				process_version  VARCHAR(64) NOT NULL,
				status           VARCHAR(32) NOT NULL,
				published_at     DATETIME(6) NULL,
				started_at       DATETIME(6) NULL,
				completed_at     DATETIME(6) NULL,
				evaluation_count INT NOT NULL DEFAULT 0
			)`, s.table("runs")),
			// MariaDB's JSON type is an alias for LONGTEXT; the CHECK makes
			// the JSON contract explicit.
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				id           BIGINT AUTO_INCREMENT PRIMARY KEY,
				run_id       VARCHAR(255) NOT NULL,
				task_id      VARCHAR(255) NOT NULL,
				execution_id VARCHAR(255) NOT NULL,
				field        VARCHAR(255) NOT NULL,
				value        LONGTEXT NOT NULL CHECK (JSON_VALID(value)),
				status       VARCHAR(16) NOT NULL DEFAULT 'pending',
				ts           DATETIME(6) NOT NULL,
				INDEX %s_replay (run_id, ts, id),
				INDEX %s_pending (run_id, task_id, status)
			)`, s.table("values"), s.table("values"), s.table("values")),
			fmt.Sprintf(`CREATE TABLE IF NOT EXISTS %s (
				id           BIGINT AUTO_INCREMENT PRIMARY KEY,
				run_id       VARCHAR(255) NOT NULL,
				kind         VARCHAR(64) NOT NULL,
				node_id      VARCHAR(255) NULL,
				execution_id VARCHAR(255) NOT NULL,
				payload      LONGTEXT NOT NULL CHECK (JSON_VALID(payload)),
				ts           DATETIME(6) NOT NULL,
				INDEX %s_replay (run_id, ts, id)
			)`, s.table("history"), s.table("history")),
		}
		for _, stmt := range stmts {
			if _, err := s.db.Exec(stmt); err != nil {
				s.schemaErr = fmt.Errorf("mariadb state store: schema: %w", err)
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

// Save upserts the run's metadata.
func (s *Store) Save(meta bl.RunMetadata) error {
	if meta.RunID == "" {
		return fmt.Errorf("save: empty RunID")
	}
	if err := s.ensureSchema(); err != nil {
		return err
	}
	_, err := s.db.Exec(fmt.Sprintf(`
		INSERT INTO %s
			(run_id, process_id, process_version, status,
			 published_at, started_at, completed_at, evaluation_count)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?)
		ON DUPLICATE KEY UPDATE
			process_id       = VALUES(process_id),
			process_version  = VALUES(process_version),
			status           = VALUES(status),
			published_at     = VALUES(published_at),
			started_at       = VALUES(started_at),
			completed_at     = VALUES(completed_at),
			evaluation_count = VALUES(evaluation_count)`, s.table("runs")),
		meta.RunID, meta.ProcessID, meta.ProcessVersion, meta.Status,
		utcPtr(meta.PublishedAt), utcPtr(meta.StartedAt), utcPtr(meta.CompletedAt),
		meta.EvaluationCount,
	)
	return err
}

// WriteBatch applies the ops in a single transaction. Inserts use MariaDB's
// INSERT ... RETURNING (10.5+) so the stored row id is confirmed in the same
// round-trip.
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
			var id int64
			if err := tx.QueryRow(fmt.Sprintf(`
				INSERT INTO %s (run_id, task_id, execution_id, field, value, status, ts)
				VALUES (?, ?, ?, ?, ?, 'pending', ?)
				RETURNING id`, s.table("values")),
				op.RunID, w.TaskID, w.ExecutionID, w.Field, string(w.Value),
				w.Timestamp.UTC(),
			).Scan(&id); err != nil {
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
			var id int64
			if err := tx.QueryRow(fmt.Sprintf(`
				INSERT INTO %s (run_id, kind, node_id, execution_id, payload, ts)
				VALUES (?, ?, ?, ?, ?, ?)
				RETURNING id`, s.table("history")),
				op.RunID, h.Kind, h.NodeID, h.ExecutionID, string(payload),
				h.Timestamp.UTC(),
			).Scan(&id); err != nil {
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
		FROM %s WHERE run_id = ?`, s.table("runs")), runID)
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

func nullTimePtr(t sql.NullTime) *time.Time {
	if !t.Valid {
		return nil
	}
	u := t.Time.UTC()
	return &u
}

// CurrentVersion returns the latest committed value per (task, field) via a
// ROW_NUMBER window (MariaDB 10.2+). Returns (nil, nil) for an unknown run.
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
			WHERE run_id = ? AND status = 'committed'
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
