package pebble_test

import (
	"testing"

	bl "github.com/friendly-business-machines/blkit/core"
	pebblestore "github.com/friendly-business-machines/blkit/stores/pebble"
)

// The suite runs against a store opened in a temporary directory that is
// removed when the test finishes — no external system. Reopen verifies the
// data survives a close/open cycle (durability).
func TestPebbleStateStoreConformance(t *testing.T) {
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		path := t.TempDir()
		open := func() bl.StateStore { return pebblestore.New(pebblestore.Config{Path: path}) }
		return open(), open
	})
}

func TestConfigNotShareable(t *testing.T) {
	s := pebblestore.New(pebblestore.Config{Path: t.TempDir()})
	defer s.Close()
	if _, err := s.Config(); err == nil {
		t.Fatal("Config() must error for the pebble store")
	}
}
