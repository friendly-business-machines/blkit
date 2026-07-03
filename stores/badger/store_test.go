package badger_test

import (
	"testing"

	bl "github.com/friendly-business-machines/blkit/core"
	badgerstore "github.com/friendly-business-machines/blkit/stores/badger"
)

// The suite runs against a store opened in a temporary directory that is
// removed when the test finishes — no external system. Reopen verifies the
// data survives a close/open cycle (durability).
func TestBadgerStateStoreConformance(t *testing.T) {
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		path := t.TempDir()
		open := func() bl.StateStore { return badgerstore.New(badgerstore.Config{Path: path}) }
		return open(), open
	})
}

func TestConfigNotShareable(t *testing.T) {
	s := badgerstore.New(badgerstore.Config{Path: t.TempDir()})
	defer s.Close()
	if _, err := s.Config(); err == nil {
		t.Fatal("Config() must error for the badger store")
	}
}
