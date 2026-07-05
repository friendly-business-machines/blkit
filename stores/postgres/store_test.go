package postgres_test

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"

	"github.com/jackc/pgx/v5/pgxpool"

	bl "github.com/friendly-business-machines/blkit/core"
	pgstore "github.com/friendly-business-machines/blkit/stores/postgres"
)

// The suite runs against a real PostgreSQL server. By default it starts a
// throwaway container via testcontainers-go; set BLKIT_TEST_POSTGRES_DSN to
// point it at an already-running server instead. It skips only when neither a
// DSN nor a reachable Docker daemon is available. Each subtest gets its own
// table prefix so runs are isolated and repeatable; the tables are dropped
// afterwards.
func TestPostgresStateStoreConformance(t *testing.T) {
	dsn := postgresDSN(t)
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

// postgresDSN returns a DSN for the conformance suite. It prefers
// BLKIT_TEST_POSTGRES_DSN; when that is unset it starts a throwaway PostgreSQL
// container and terminates it when the test ends.
func postgresDSN(t *testing.T) string {
	t.Helper()
	if dsn := os.Getenv("BLKIT_TEST_POSTGRES_DSN"); dsn != "" {
		return dsn
	}
	ctx := context.Background()
	ctr, err := tcpostgres.Run(ctx, "postgres:16-alpine",
		tcpostgres.WithDatabase("blkit"),
		tcpostgres.WithUsername("blkit"),
		tcpostgres.WithPassword("blkit"),
		tcpostgres.BasicWaitStrategies(),
	)
	if err != nil {
		t.Skipf("start postgres container (set BLKIT_TEST_POSTGRES_DSN to use an existing server): %v", err)
	}
	t.Cleanup(func() { _ = ctr.Terminate(context.Background()) })
	dsn, err := ctr.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("postgres connection string: %v", err)
	}
	return dsn
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
