package mysql_test

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"testing"
	"time"

	tcmysql "github.com/testcontainers/testcontainers-go/modules/mysql"

	bl "github.com/friendly-business-machines/blkit/core"
	mysqlstore "github.com/friendly-business-machines/blkit/stores/mysql"
)

// The suite runs against a real MySQL server. By default it starts a throwaway
// container via testcontainers-go; set BLKIT_TEST_MYSQL_DSN to point it at an
// already-running server instead. It skips only when neither a DSN nor a
// reachable Docker daemon is available. Each subtest gets its own table prefix
// so runs are isolated and repeatable; the tables are dropped afterwards.
func TestMysqlStateStoreConformance(t *testing.T) {
	dsn := mysqlDSN(t)
	var n int
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		n++
		prefix := fmt.Sprintf("blkit_t%d_%d_", time.Now().Unix(), n)
		t.Cleanup(func() { dropTables(t, dsn, prefix) })
		open := func() bl.StateStore {
			return mysqlstore.New(mysqlstore.Config{DSN: dsn, TablePrefix: prefix})
		}
		return open(), open
	})
}

// mysqlDSN returns a DSN for the conformance suite. It prefers
// BLKIT_TEST_MYSQL_DSN; when that is unset it starts a throwaway MySQL
// container and terminates it when the test ends.
func mysqlDSN(t *testing.T) string {
	t.Helper()
	if dsn := os.Getenv("BLKIT_TEST_MYSQL_DSN"); dsn != "" {
		return dsn
	}
	ctx := context.Background()
	ctr, err := tcmysql.Run(ctx, "mysql:8.0",
		tcmysql.WithDatabase("blkit"),
		tcmysql.WithUsername("blkit"),
		tcmysql.WithPassword("blkit"),
	)
	if err != nil {
		t.Skipf("start mysql container (set BLKIT_TEST_MYSQL_DSN to use an existing server): %v", err)
	}
	t.Cleanup(func() { _ = ctr.Terminate(context.Background()) })
	dsn, err := ctr.ConnectionString(ctx)
	if err != nil {
		t.Fatalf("mysql connection string: %v", err)
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
