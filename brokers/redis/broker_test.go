package redis_test

import (
	"context"
	"fmt"
	"os"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	goredis "github.com/redis/go-redis/v9"
	"github.com/testcontainers/testcontainers-go"
	"github.com/testcontainers/testcontainers-go/wait"

	redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"
	bl "github.com/friendly-business-machines/blkit/core"
)

// Short timing knobs so the heartbeat-loss and redelivery subtests are fast;
// the opened brokers are configured with these same values and they are
// declared to the conformance suite.
const (
	testRegistrationTTL = 1500 * time.Millisecond
	testInFlightTimeout = time.Second
)

var (
	testAddr     string
	testUsername string
	testPassword string
	skipReason   string
)

// TestMain provides one Redis server per test binary: BLKIT_TEST_REDIS_URL
// points at an already-running instance; otherwise a throwaway container is
// started via testcontainers-go. When neither is available the tests skip
// with skipReason.
func TestMain(m *testing.M) { os.Exit(testMain(m)) }

func testMain(m *testing.M) int {
	if url := os.Getenv("BLKIT_TEST_REDIS_URL"); url != "" {
		if strings.Contains(url, "://") {
			opt, err := goredis.ParseURL(url)
			if err != nil {
				fmt.Fprintf(os.Stderr, "invalid BLKIT_TEST_REDIS_URL %q: %v\n", url, err)
				return 1
			}
			testAddr, testUsername, testPassword = opt.Addr, opt.Username, opt.Password
		} else {
			testAddr = url
		}
		return m.Run()
	}
	ctx := context.Background()
	ctr, err := testcontainers.GenericContainer(ctx, testcontainers.GenericContainerRequest{
		ContainerRequest: testcontainers.ContainerRequest{
			Image:        "docker.io/redis:7-alpine",
			ExposedPorts: []string{"6379/tcp"},
			WaitingFor:   wait.ForLog("Ready to accept connections").WithStartupTimeout(60 * time.Second),
		},
		Started: true,
	})
	if err != nil {
		skipReason = fmt.Sprintf("start redis container (set BLKIT_TEST_REDIS_URL to use an existing server): %v", err)
		return m.Run()
	}
	defer func() { _ = testcontainers.TerminateContainer(ctr) }()
	endpoint, err := ctr.Endpoint(ctx, "")
	if err != nil {
		skipReason = fmt.Sprintf("redis container endpoint: %v", err)
		return m.Run()
	}
	testAddr = endpoint
	return m.Run()
}

// TestRedisMessageBrokerConformance runs the shared message-broker
// conformance suite against the Redis backend. Every open gets a fresh
// KeyPrefix so subtests are isolated on the shared server.
func TestRedisMessageBrokerConformance(t *testing.T) {
	if skipReason != "" {
		t.Skip(skipReason)
	}
	var n atomic.Int64
	open := func(t *testing.T, cipher bl.PayloadCipher) bl.MessageBroker {
		t.Helper()
		prefix := fmt.Sprintf("blkit-t%d-%d", time.Now().UnixNano(), n.Add(1))
		b, err := redisbroker.New(redisbroker.Config{
			Addr:            testAddr,
			Username:        testUsername,
			Password:        testPassword,
			KeyPrefix:       prefix,
			Cipher:          cipher,
			RegistrationTTL: testRegistrationTTL,
			InFlightTimeout: testInFlightTimeout,
		})
		if err != nil {
			t.Fatalf("open redis broker: %v", err)
		}
		return b
	}
	bl.RunMessageBrokerConformance(t,
		func(t *testing.T) bl.MessageBroker { return open(t, nil) },
		bl.BrokerConformanceOptions{
			SupportsQueueRemoval: true,
			RegistrationTTL:      testRegistrationTTL,
			InFlightTimeout:      testInFlightTimeout,
			OpenWithCipher: func(t *testing.T) bl.MessageBroker {
				t.Helper()
				cipher, err := bl.NewAESGCMPayloadCipher("conformance-key", []byte("0123456789abcdef0123456789abcdef"))
				if err != nil {
					t.Fatalf("cipher: %v", err)
				}
				return open(t, cipher)
			},
		})
}
