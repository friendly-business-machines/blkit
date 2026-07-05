package mssql_test

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"testing"
	"time"

	tcmssql "github.com/testcontainers/testcontainers-go/modules/mssql"

	bl "github.com/friendly-business-machines/blkit/core"
	mssqlstore "github.com/friendly-business-machines/blkit/stores/mssql"
)

// The suite runs against a real SQL Server instance. By default it starts a
// throwaway container via testcontainers-go; set BLKIT_TEST_MSSQL_DSN to point
// it at an already-running server instead. It skips only when neither a DSN nor
// a reachable Docker daemon is available. Each subtest gets its own table
// prefix so runs are isolated and repeatable; the tables are dropped
// afterwards.
func TestMssqlStateStoreConformance(t *testing.T) {
	dsn := mssqlDSN(t)
	var n int
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		n++
		prefix := fmt.Sprintf("blkit_t%d_%d_", time.Now().Unix(), n)
		t.Cleanup(func() { dropTables(t, dsn, prefix) })
		open := func() bl.StateStore {
			return mssqlstore.New(mssqlstore.Config{DSN: dsn, TablePrefix: prefix})
		}
		return open(), open
	})
}

// mssqlDSN returns a DSN for the conformance suite. It prefers
// BLKIT_TEST_MSSQL_DSN; when that is unset it starts a throwaway SQL Server
// container and terminates it when the test ends.
func mssqlDSN(t *testing.T) string {
	t.Helper()
	if dsn := os.Getenv("BLKIT_TEST_MSSQL_DSN"); dsn != "" {
		return dsn
	}
	ctx := context.Background()
	ctr, err := tcmssql.Run(ctx, "mcr.microsoft.com/mssql/server:2022-latest",
		tcmssql.WithAcceptEULA(),
		tcmssql.WithPassword("Str0ng!Passw0rd"),
	)
	if err != nil {
		t.Skipf("start mssql container (set BLKIT_TEST_MSSQL_DSN to use an existing server): %v", err)
	}
	t.Cleanup(func() { _ = ctr.Terminate(context.Background()) })
	dsn, err := ctr.ConnectionString(ctx)
	if err != nil {
		t.Fatalf("mssql connection string: %v", err)
	}
	return dsn
}

func dropTables(t *testing.T, dsn, prefix string) {
	t.Helper()
	db, err := sql.Open("sqlserver", dsn)
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
