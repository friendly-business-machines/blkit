package bbolt_test

import (
	"path/filepath"
	"testing"

	bl "github.com/friendly-business-machines/blkit/core"
	bboltstore "github.com/friendly-business-machines/blkit/stores/bbolt"
)

// The suite runs against a store opened in a temporary directory that is
// removed when the test finishes — no external system. Reopen verifies the
// data survives a close/open cycle (durability).
func TestBboltStateStoreConformance(t *testing.T) {
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		path := filepath.Join(t.TempDir(), "state.db")
		open := func() bl.StateStore { return bboltstore.New(bboltstore.Config{Path: path}) }
		return open(), open
	})
}

func TestConfigNotShareable(t *testing.T) {
	s := bboltstore.New(bboltstore.Config{Path: filepath.Join(t.TempDir(), "state.db")})
	defer s.Close()
	if _, err := s.Config(); err == nil {
		t.Fatal("Config() must error for the bbolt store")
	}
}
