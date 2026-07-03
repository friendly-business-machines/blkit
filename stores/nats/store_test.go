package nats_test

import (
	"fmt"
	"testing"
	"time"

	natsserver "github.com/nats-io/nats-server/v2/server"

	bl "github.com/friendly-business-machines/blkit/core"
	natsstore "github.com/friendly-business-machines/blkit/stores/nats"
)

// The suite runs against a real NATS server with JetStream enabled, embedded
// in the test process — the genuine engine, no external system. Reopen
// verifies a fresh connection sees everything (read-your-writes across
// handles).
func TestNatsStateStoreConformance(t *testing.T) {
	var bucketN int
	bl.RunStateStoreConformance(t, func(t *testing.T) (bl.StateStore, func() bl.StateStore) {
		srv := startServer(t)
		bucketN++
		cfg := natsstore.Config{
			URL:    srv.ClientURL(),
			Bucket: fmt.Sprintf("blkit-state-%d", bucketN),
		}
		open := func() bl.StateStore { return natsstore.New(cfg) }
		return open(), open
	})
}

func TestConfigShareable(t *testing.T) {
	srv := startServer(t)
	s := natsstore.New(natsstore.Config{URL: srv.ClientURL(), Bucket: "blkit-cfg"})
	defer s.Close()
	cfg, err := s.Config()
	if err != nil {
		t.Fatalf("Config: %v", err)
	}
	if cfg["url"] == "" || cfg["bucket"] != "blkit-cfg" {
		t.Fatalf("Config incomplete: %v", cfg)
	}
}

func startServer(t *testing.T) *natsserver.Server {
	t.Helper()
	srv, err := natsserver.NewServer(&natsserver.Options{
		Port:      -1, // random port
		JetStream: true,
		StoreDir:  t.TempDir(),
	})
	if err != nil {
		t.Fatalf("nats server: %v", err)
	}
	go srv.Start()
	if !srv.ReadyForConnections(10 * time.Second) {
		t.Fatal("nats server not ready")
	}
	t.Cleanup(srv.Shutdown)
	return srv
}
