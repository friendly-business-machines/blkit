package nats_test

import (
	"fmt"
	"os"
	"sync/atomic"
	"testing"
	"time"

	natsserver "github.com/nats-io/nats-server/v2/server"

	natsbroker "github.com/friendly-business-machines/blkit/brokers/nats"
	bl "github.com/friendly-business-machines/blkit/core"
)

// The suite runs against a real NATS server with JetStream enabled, embedded
// in the test process — the genuine engine, no external system.
// BLKIT_TEST_NATS_URL points it at an external server instead.
var natsURL string

func TestMain(m *testing.M) {
	if url := os.Getenv("BLKIT_TEST_NATS_URL"); url != "" {
		natsURL = url
		os.Exit(m.Run())
	}
	dir, err := os.MkdirTemp("", "blkit-nats-broker")
	if err != nil {
		fmt.Fprintf(os.Stderr, "temp dir: %v\n", err)
		os.Exit(1)
	}
	srv, err := natsserver.NewServer(&natsserver.Options{
		Port:      -1, // random port
		JetStream: true,
		StoreDir:  dir,
	})
	if err != nil {
		fmt.Fprintf(os.Stderr, "nats server: %v\n", err)
		os.Exit(1)
	}
	go srv.Start()
	if !srv.ReadyForConnections(10 * time.Second) {
		fmt.Fprintln(os.Stderr, "nats server not ready")
		os.Exit(1)
	}
	natsURL = srv.ClientURL()
	code := m.Run()
	srv.Shutdown()
	_ = os.RemoveAll(dir)
	os.Exit(code)
}

// Short TTL / timeout values keep the heartbeat-loss and redelivery subtests
// fast while staying well inside the conformance helpers' 8s waits.
const (
	testRegistrationTTL = 1500 * time.Millisecond
	testInFlightTimeout = 2 * time.Second
)

var prefixN atomic.Uint64

// openBroker opens a fresh broker on a unique subject prefix, so every
// subtest gets isolated streams and buckets on the shared server.
func openBroker(t *testing.T, cipher bl.PayloadCipher) bl.MessageBroker {
	t.Helper()
	b, err := natsbroker.New(natsbroker.Config{
		URL:             natsURL,
		SubjectPrefix:   fmt.Sprintf("cb%d", prefixN.Add(1)),
		Cipher:          cipher,
		RegistrationTTL: testRegistrationTTL,
		InFlightTimeout: testInFlightTimeout,
	})
	if err != nil {
		t.Fatalf("open broker: %v", err)
	}
	t.Cleanup(func() { _ = b.Close() })
	return b
}

func TestNatsMessageBrokerConformance(t *testing.T) {
	bl.RunMessageBrokerConformance(t, func(t *testing.T) bl.MessageBroker {
		return openBroker(t, nil)
	}, bl.BrokerConformanceOptions{
		SupportsQueueRemoval: true,
		RegistrationTTL:      testRegistrationTTL,
		InFlightTimeout:      testInFlightTimeout,
		OpenWithCipher: func(t *testing.T) bl.MessageBroker {
			key := make([]byte, 32)
			for i := range key {
				key[i] = byte(i * 7)
			}
			cipher, err := bl.NewAESGCMPayloadCipher("test-key", key)
			if err != nil {
				t.Fatalf("cipher: %v", err)
			}
			return openBroker(t, cipher)
		},
	})
}
