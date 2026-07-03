package sqlite_test

import (
	"path/filepath"
	"testing"

	bl "github.com/friendly-business-machines/blkit/core"
	sqlitestore "github.com/friendly-business-machines/blkit/stores/sqlite"
)

// The suite runs against a database file in a temporary directory that is
// removed when the test finishes — no external system. Reopen verifies the
// data survives a close/open cycle (durability).
func TestSqliteStateStoreConformance(t *testing.T) {
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		path := filepath.Join(t.TempDir(), "state.db")
		open := func() bl.StateStore { return sqlitestore.New(sqlitestore.Config{Path: path}) }
		return open(), open
	})
}

func TestConfigNotShareable(t *testing.T) {
	s := sqlitestore.New(sqlitestore.Config{Path: filepath.Join(t.TempDir(), "state.db")})
	defer s.Close()
	if _, err := s.Config(); err == nil {
		t.Fatal("Config() must error for the sqlite store")
	}
}
