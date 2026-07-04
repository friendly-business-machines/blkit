package turso_test

import (
	"path/filepath"
	"testing"

	bl "github.com/friendly-business-machines/blkit/core"
	tursostore "github.com/friendly-business-machines/blkit/stores/turso"
)

// The suite runs against the real Turso Database engine, in-process, on a
// database file in a temporary directory that is removed when the test
// finishes — no external system. Reopen verifies the data survives a
// close/open cycle (durability of the SQLite-compatible file format).
func TestTursoStateStoreConformance(t *testing.T) {
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		path := filepath.Join(t.TempDir(), "state.db")
		open := func() bl.StateStore { return tursostore.New(tursostore.Config{Path: path}) }
		return open(), open
	})
}

func TestConfigNotShareable(t *testing.T) {
	s := tursostore.New(tursostore.Config{Path: filepath.Join(t.TempDir(), "state.db")})
	defer s.Close()
	if _, err := s.Config(); err == nil {
		t.Fatal("Config() must error for the turso store")
	}
}
