package mariadb_test

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"testing"
	"time"

	tcmariadb "github.com/testcontainers/testcontainers-go/modules/mariadb"

	bl "github.com/friendly-business-machines/blkit/core"
	mariadbstore "github.com/friendly-business-machines/blkit/stores/mariadb"
)

// The suite runs against a real MariaDB server. By default it starts a
// throwaway container via testcontainers-go; set BLKIT_TEST_MARIADB_DSN to
// point it at an already-running server instead. It skips only when neither a
// DSN nor a reachable Docker daemon is available. Each subtest gets its own
// table prefix so runs are isolated and repeatable; the tables are dropped
// afterwards.
func TestMariadbStateStoreConformance(t *testing.T) {
	dsn := mariadbDSN(t)
	var n int
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		n++
		prefix := fmt.Sprintf("blkit_t%d_%d_", time.Now().Unix(), n)
		t.Cleanup(func() { dropTables(t, dsn, prefix) })
		open := func() bl.StateStore {
			return mariadbstore.New(mariadbstore.Config{DSN: dsn, TablePrefix: prefix})
		}
		return open(), open
	})
}

// mariadbDSN returns a DSN for the conformance suite. It prefers
// BLKIT_TEST_MARIADB_DSN; when that is unset it starts a throwaway MariaDB
// container and terminates it when the test ends.
func mariadbDSN(t *testing.T) string {
	t.Helper()
	if dsn := os.Getenv("BLKIT_TEST_MARIADB_DSN"); dsn != "" {
		return dsn
	}
	ctx := context.Background()
	ctr, err := tcmariadb.Run(ctx, "mariadb:11",
		tcmariadb.WithDatabase("blkit"),
		tcmariadb.WithUsername("blkit"),
		tcmariadb.WithPassword("blkit"),
	)
	if err != nil {
		t.Skipf("start mariadb container (set BLKIT_TEST_MARIADB_DSN to use an existing server): %v", err)
	}
	t.Cleanup(func() { _ = ctr.Terminate(context.Background()) })
	dsn, err := ctr.ConnectionString(ctx)
	if err != nil {
		t.Fatalf("mariadb connection string: %v", err)
	}
	return dsn
}

func dropTables(t *testing.T, dsn, prefix string) {
	t.Helper()
	db, err := sql.Open("mysql", dsn)
	if err != nil {
		t.Logf("cleanup: %v", err)
		return
	}
	defer db.Close()
	for _, table := range []string{"runs", "values", "history"} {
		if _, err := db.Exec("DROP TABLE IF EXISTS " + prefix + table); err != nil {
			t.Logf("cleanup %s%s: %v", prefix, table, err)
		}
	}
}
