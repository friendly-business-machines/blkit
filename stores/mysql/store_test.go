package mysql_test

import (
	"database/sql"
	"fmt"
	"os"
	"testing"
	"time"

	bl "github.com/friendly-business-machines/blkit/core"
	mysqlstore "github.com/friendly-business-machines/blkit/stores/mysql"
)

// The suite runs against a real MySQL server. Point
// BLKIT_TEST_MYSQL_DSN at one (CI runs it in a container); the test skips
// when the variable is unset. Each subtest gets its own table prefix so runs
// are isolated and repeatable; the tables are dropped afterwards.
func TestMysqlStateStoreConformance(t *testing.T) {
	dsn := os.Getenv("BLKIT_TEST_MYSQL_DSN")
	if dsn == "" {
		t.Skip("BLKIT_TEST_MYSQL_DSN not set; skipping MySQL conformance test")
	}
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
