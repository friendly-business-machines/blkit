package gpubsub_test

import (
	"context"
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"cloud.google.com/go/pubsub"
	tcfirestore "github.com/testcontainers/testcontainers-go/modules/gcloud/firestore"
	tcpubsub "github.com/testcontainers/testcontainers-go/modules/gcloud/pubsub"
	"google.golang.org/api/option"
	"google.golang.org/grpc"
	insecurecreds "google.golang.org/grpc/credentials/insecure"

	gpubsub "github.com/friendly-business-machines/blkit/brokers/google-pubsub"
	bl "github.com/friendly-business-machines/blkit/core"
)

// The suite runs against the gcloud Pub/Sub and Firestore emulators. By
// default it starts one throwaway container per emulator (shared by the whole
// test binary) via testcontainers-go's gcloud module; set
// BLKIT_TEST_GPUBSUB_ENDPOINT and BLKIT_TEST_FIRESTORE_ENDPOINT to point it
// at already-running emulators instead (BLKIT_TEST_GPUBSUB_PROJECT overrides
// the project id). Tests skip only when neither an endpoint nor a reachable
// Docker daemon is available. Each open(t) uses a fresh entity/collection
// prefix, so subtests are isolated on the shared emulators.

const emulatorImage = "gcr.io/google.com/cloudsdktool/cloud-sdk:513.0.0-emulators"

// registrationTTL must be long enough that a registration outlives a
// subtest's chain of emulator round trips, and short enough that the
// heartbeat-loss subtest sees the TTL lapse within the conformance suite's
// 8s wait budget.
const registrationTTL = 4 * time.Second

// inFlightTimeout is Pub/Sub's minimum ack deadline. The redelivery subtest
// does not wait this long: a fetcher whose context dies nacks its unsettled
// jobs, so redelivery to another worker is prompt. The deadline only governs
// hard worker crashes (process death), where lease extension stops.
const inFlightTimeout = 10 * time.Second

var (
	emuMu       sync.Mutex
	terminators []func()

	pubsubStarted bool
	pubsubURI     string
	pubsubFail    error

	firestoreStarted bool
	firestoreURI     string
	firestoreFail    error
)

func TestMain(m *testing.M) {
	code := m.Run()
	emuMu.Lock()
	for _, terminate := range terminators {
		terminate()
	}
	emuMu.Unlock()
	os.Exit(code)
}

func testProjectID() string {
	if p := os.Getenv("BLKIT_TEST_GPUBSUB_PROJECT"); p != "" {
		return p
	}
	return "blkit-test"
}

// pubsubEndpoint returns the Pub/Sub emulator endpoint, starting the shared
// container on first use.
func pubsubEndpoint(t *testing.T) string {
	t.Helper()
	if v := os.Getenv("BLKIT_TEST_GPUBSUB_ENDPOINT"); v != "" {
		return v
	}
	emuMu.Lock()
	defer emuMu.Unlock()
	if !pubsubStarted {
		pubsubStarted = true
		ctr, err := tcpubsub.Run(context.Background(), emulatorImage, tcpubsub.WithProjectID(testProjectID()))
		if err != nil {
			pubsubFail = err
		} else {
			pubsubURI = ctr.URI()
			terminators = append(terminators, func() { _ = ctr.Terminate(context.Background()) })
		}
	}
	if pubsubFail != nil {
		t.Skipf("start pubsub emulator container (set BLKIT_TEST_GPUBSUB_ENDPOINT to use an existing emulator): %v", pubsubFail)
	}
	return pubsubURI
}

// firestoreEndpoint returns the Firestore emulator endpoint, starting the
// shared container on first use.
func firestoreEndpoint(t *testing.T) string {
	t.Helper()
	if v := os.Getenv("BLKIT_TEST_FIRESTORE_ENDPOINT"); v != "" {
		return v
	}
	emuMu.Lock()
	defer emuMu.Unlock()
	if !firestoreStarted {
		firestoreStarted = true
		ctr, err := tcfirestore.Run(context.Background(), emulatorImage, tcfirestore.WithProjectID(testProjectID()))
		if err != nil {
			firestoreFail = err
		} else {
			firestoreURI = ctr.URI()
			terminators = append(terminators, func() { _ = ctr.Terminate(context.Background()) })
		}
	}
	if firestoreFail != nil {
		t.Skipf("start firestore emulator container (set BLKIT_TEST_FIRESTORE_ENDPOINT to use an existing emulator): %v", firestoreFail)
	}
	return firestoreURI
}

// emulatorClientOptions dials an emulator endpoint without auth or TLS.
func emulatorClientOptions(endpoint string) []option.ClientOption {
	return []option.ClientOption{
		option.WithEndpoint(endpoint),
		option.WithoutAuthentication(),
		option.WithGRPCDialOption(grpc.WithTransportCredentials(insecurecreds.NewCredentials())),
	}
}

var openCount atomic.Int64

// openBroker returns a fresh broker (and its Firestore registry store) on an
// isolating entity/collection prefix.
func openBroker(t *testing.T, cipher bl.PayloadCipher) bl.MessageBroker {
	t.Helper()
	psURI := pubsubEndpoint(t)
	fsURI := firestoreEndpoint(t)
	prefix := fmt.Sprintf("blkit-%d-%d", os.Getpid(), openCount.Add(1))
	store, err := gpubsub.NewFirestoreRegistryStore(gpubsub.FirestoreConfig{
		ProjectID:        testProjectID(),
		CollectionPrefix: prefix,
		Endpoint:         fsURI,
		Cipher:           cipher,
		PollInterval:     100 * time.Millisecond,
	})
	if err != nil {
		t.Fatalf("firestore registry store: %v", err)
	}
	t.Cleanup(func() { _ = store.Close() })
	b, err := gpubsub.New(gpubsub.Config{
		ProjectID:       testProjectID(),
		EntityPrefix:    prefix,
		Registry:        store,
		Endpoint:        psURI,
		Cipher:          cipher,
		RegistrationTTL: registrationTTL,
		InFlightTimeout: inFlightTimeout,
	})
	if err != nil {
		t.Fatalf("new broker: %v", err)
	}
	return b
}

func TestGooglePubSubMessageBrokerConformance(t *testing.T) {
	bl.RunMessageBrokerConformance(t,
		func(t *testing.T) bl.MessageBroker { return openBroker(t, nil) },
		bl.BrokerConformanceOptions{
			// Pub/Sub has no message deletion: Cancel always takes the
			// JobCancel route.
			SupportsQueueRemoval: false,
			RegistrationTTL:      registrationTTL,
			// A cancelled fetcher nacks its unsettled jobs, so the
			// redelivery subtest passes promptly; only a hard process
			// crash waits out the 10s ack deadline.
			InFlightTimeout: inFlightTimeout,
			OpenWithCipher: func(t *testing.T) bl.MessageBroker {
				key := make([]byte, 32)
				for i := range key {
					key[i] = byte(i * 7)
				}
				cipher, err := bl.NewAESGCMPayloadCipher("conformance-key", key)
				if err != nil {
					t.Fatalf("cipher: %v", err)
				}
				return openBroker(t, cipher)
			},
		})
}

// TestSubscribeToUnknownInstance checks the INSTANCE_NOT_FOUND first event
// for a subscription to an instance the broker has no record of.
func TestSubscribeToUnknownInstance(t *testing.T) {
	b := openBroker(t, nil)
	defer func() { _ = b.Close() }()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	events, err := b.SubscribeToInstance(ctx, "no-such-instance")
	if err != nil {
		t.Fatalf("subscribe: %v", err)
	}
	select {
	case evt := <-events:
		if evt.Kind != bl.InstanceEventError || evt.Error == nil || evt.Error.Code != bl.ErrCodeInstanceNotFound {
			t.Fatalf("want INSTANCE_NOT_FOUND first event, got %+v", evt)
		}
	case <-time.After(8 * time.Second):
		t.Fatal("timed out waiting for the INSTANCE_NOT_FOUND event")
	}
}

// TestEmulatorSubscriptionFilterProbe documents whether the emulator under
// test accepts and enforces server-side subscription filters. The broker
// works either way (it always also filters client-side); this probe exists
// so emulator behavior changes are visible in test output.
func TestEmulatorSubscriptionFilterProbe(t *testing.T) {
	endpoint := pubsubEndpoint(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	client, err := pubsub.NewClient(ctx, testProjectID(), emulatorClientOptions(endpoint)...)
	if err != nil {
		t.Fatalf("pubsub client: %v", err)
	}
	defer func() { _ = client.Close() }()

	suffix := fmt.Sprintf("%d-%d", os.Getpid(), openCount.Add(1))
	topic, err := client.CreateTopic(ctx, "blkit-probe-topic-"+suffix)
	if err != nil {
		t.Fatalf("create topic: %v", err)
	}
	defer topic.Stop()
	sub, err := client.CreateSubscription(ctx, "blkit-probe-sub-"+suffix, pubsub.SubscriptionConfig{
		Topic:  topic,
		Filter: `attributes.x = "keep"`,
	})
	if err != nil {
		t.Logf("emulator rejects subscription filters at creation: %v", err)
		return
	}
	for _, x := range []string{"keep", "drop"} {
		res := topic.Publish(ctx, &pubsub.Message{Data: []byte(x), Attributes: map[string]string{"x": x}})
		if _, err := res.Get(ctx); err != nil {
			t.Fatalf("publish %s: %v", x, err)
		}
	}
	rctx, rcancel := context.WithTimeout(ctx, 3*time.Second)
	defer rcancel()
	var mu sync.Mutex
	got := map[string]bool{}
	_ = sub.Receive(rctx, func(_ context.Context, msg *pubsub.Message) {
		mu.Lock()
		got[string(msg.Data)] = true
		mu.Unlock()
		msg.Ack()
	})
	switch {
	case got["keep"] && !got["drop"]:
		t.Log("emulator accepts AND enforces subscription filters")
	case got["keep"] && got["drop"]:
		t.Log("emulator accepts subscription filters but does NOT enforce them (client-side filtering covers this)")
	default:
		t.Logf("probe inconclusive: received %v", got)
	}
}
