package core

import "testing"

// The in-memory backend runs the shared conformance suite in-process — no
// setup, no cleanup, no reopen (memory does not survive a handle).
func TestInMemoryStateStoreConformance(t *testing.T) {
	RunStateStoreConformance(t, func(t *testing.T) (StateStore, func() StateStore) {
		return NewInMemoryStateStore(), nil
	})
}

func TestInMemoryStateStoreConfigNotShareable(t *testing.T) {
	s := NewInMemoryStateStore()
	if _, err := s.Config(); err == nil {
		t.Fatal("Config() must error for the in-memory store")
	}
}
