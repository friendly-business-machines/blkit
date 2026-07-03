package postgres_test

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	bl "github.com/friendly-business-machines/blkit/core"
	pgstore "github.com/friendly-business-machines/blkit/stores/postgres"
)

// The suite runs against a real PostgreSQL server. Point
// BLKIT_TEST_POSTGRES_DSN at one (CI runs it in a container); the test skips
// when the variable is unset. Each subtest gets its own table prefix so runs
// are isolated and repeatable; the tables are dropped afterwards.
func TestPostgresStateStoreConformance(t *testing.T) {
	dsn := os.Getenv("BLKIT_TEST_POSTGRES_DSN")
	if dsn == "" {
		t.Skip("BLKIT_TEST_POSTGRES_DSN not set; skipping PostgreSQL conformance test")
	}
	var n int
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		n++
		prefix := fmt.Sprintf("blkit_t%d_%d_", time.Now().Unix(), n)
		t.Cleanup(func() { dropTables(t, dsn, prefix) })
		open := func() bl.StateStore {
			return pgstore.New(pgstore.Config{DSN: dsn, TablePrefix: prefix})
		}
		return open(), open
	})
}

func dropTables(t *testing.T, dsn, prefix string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Logf("cleanup: %v", err)
		return
	}
	defer pool.Close()
	for _, table := range []string{"runs", "values", "history"} {
		if _, err := pool.Exec(ctx, "DROP TABLE IF EXISTS "+prefix+table); err != nil {
			t.Logf("cleanup %s%s: %v", prefix, table, err)
		}
	}
}
