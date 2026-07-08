package azuresb_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Azure/azure-sdk-for-go/sdk/azcore/to"
	"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"
	"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus/admin"
	"github.com/testcontainers/testcontainers-go"
	tcnetwork "github.com/testcontainers/testcontainers-go/network"
	"github.com/testcontainers/testcontainers-go/wait"

	azuresb "github.com/friendly-business-machines/blkit/brokers/azure-service-bus"
	bl "github.com/friendly-business-machines/blkit/core"
)

// The conformance suite is gated on BLKIT_TEST_AZURESB_CONNECTION (a real
// Service Bus namespace; entities are created dynamically via the admin
// client under a per-open prefix) plus BLKIT_TEST_AZURE_TABLES_CONNECTION
// (or auto-started Azurite) for the RegistryStore. The TableRegistryStore
// tests below always run locally against Azurite.
//
// An emulator route also exists behind BLKIT_TEST_AZURESB_EMULATOR=1: it
// starts Microsoft's Service Bus emulator (with its companion SQL Server
// container) via testcontainers-go, predeclares the conformance job queues,
// the instance-events topic, and a pump-subscription pool in the emulator's
// config JSON (the emulator cannot create entities at runtime), claims pump
// subscriptions round-robin, drains the shared queues per open, and shares
// one AMQP connection across opens. It is OFF by default because the
// emulator proved unstable under this suite: its gateway logs
// System.NotImplementedException / "failed to report load to Winfab
// runtime" and stops accepting AMQP connections (new TCP connects hang),
// wedging the run — despite reporting "Emulator Service is Successfully
// Up!". Try it on your own Docker host; a real namespace is the reliable
// route.

const (
	testPrefix   = "blkit"
	pumpPoolSize = 20
	sqlPassword  = "Str0ng!Passw0rd"
	testTTL      = 1500 * time.Millisecond
	testInFlight = 5 * time.Second // Service Bus minimum lock duration
)

// conformanceKeys are the fixed process keys the shared conformance suite
// submits against (core/message_broker_conformance.go).
var conformanceKeys = []bl.ProcessKey{
	{Namespace: "example.com/conformance", ProcessID: "proc-a", Version: "1.0"},
	{Namespace: "example.com/conformance", ProcessID: "proc-b", Version: "1.0"},
}

type testEnv struct {
	sbConn     string
	tablesConn string
	emulator   bool
	drain      *azservicebus.Client // shared across opens to limit connection churn
}

func TestAzureServiceBusBrokerConformance(t *testing.T) {
	env := serviceBusEnv(t)
	var n atomic.Int64
	open := func(t *testing.T, cipher bl.PayloadCipher) bl.MessageBroker {
		i := n.Add(1)
		store := openTableStore(t, env.tablesConn, fmt.Sprintf("conf%dn%d", time.Now().Unix(), i), cipher)
		cfg := azuresb.Config{
			ConnectionString: env.sbConn,
			EntityPrefix:     testPrefix,
			Registry:         store,
			Cipher:           cipher,
			RegistrationTTL:  testTTL,
			InFlightTimeout:  testInFlight,
			EventRetention:   time.Hour,
		}
		if env.emulator {
			cfg.UseEmulator = true
			cfg.PumpSubscription = fmt.Sprintf("pump-%02d", i%pumpPoolSize)
			// Share one AMQP connection across every open: the emulator
			// wedges its accept loop under repeated connection open/close
			// churn (new TCP connects eventually hang in SYN).
			cfg.Client = env.drain
			drainQueues(t, env.drain, conformanceQueues()...)
		} else {
			cfg.EntityPrefix = fmt.Sprintf("%s-t%d-%d", testPrefix, time.Now().Unix(), i)
			t.Cleanup(func() { cleanupNamespaceEntities(t, env.sbConn, cfg.EntityPrefix) })
		}
		b, err := azuresb.New(cfg)
		if err != nil {
			t.Fatalf("open broker: %v", err)
		}
		return b
	}
	bl.RunMessageBrokerConformance(t, func(t *testing.T) bl.MessageBroker {
		return open(t, nil)
	}, bl.BrokerConformanceOptions{
		SupportsQueueRemoval: false, // Service Bus cannot remove an arbitrary queued message
		RegistrationTTL:      testTTL,
		InFlightTimeout:      testInFlight,
		OpenWithCipher: func(t *testing.T) bl.MessageBroker {
			cipher, err := bl.NewAESGCMPayloadCipher("k1", make([]byte, 32))
			if err != nil {
				t.Fatalf("cipher: %v", err)
			}
			return open(t, cipher)
		},
	})
}

// ===== TableRegistryStore unit tests (Azurite-only live coverage) =====

func TestTableRegistryStoreRegistrations(t *testing.T) {
	conn := tablesConn(t)
	s := openTableStore(t, conn, fmt.Sprintf("regs%d", time.Now().UnixNano()%1e9), nil)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	regA := testRegistration("proc-a", "w1")
	regB := testRegistration("proc-b", "w1")
	if err := s.PutRegistrations(ctx, "w1", []bl.ProcessRegistration{regA, regB}, 10*time.Second); err != nil {
		t.Fatalf("put: %v", err)
	}
	snap, err := s.Snapshot(ctx)
	if err != nil {
		t.Fatalf("snapshot: %v", err)
	}
	if len(snap) != 2 {
		t.Fatalf("want 2 registrations, got %d", len(snap))
	}
	for _, r := range snap {
		if r.WorkerID != "w1" || len(r.StartEvents) == 0 || r.StartEvents[0].InputContract == nil {
			t.Fatalf("registration did not round trip intact: %+v", r)
		}
	}

	// RegisteredAt is preserved per key across re-registration.
	first := snap[0].RegisteredAt
	time.Sleep(20 * time.Millisecond)
	regA.RegisteredAt = time.Now()
	if err := s.PutRegistrations(ctx, "w1", []bl.ProcessRegistration{regA}, 10*time.Second); err != nil {
		t.Fatalf("re-put: %v", err)
	}
	snap, err = s.Snapshot(ctx)
	if err != nil {
		t.Fatalf("snapshot 2: %v", err)
	}
	if len(snap) != 1 {
		t.Fatalf("re-registration must replace the set, got %d", len(snap))
	}
	if !snap[0].RegisteredAt.Equal(first) {
		t.Fatalf("RegisteredAt not preserved: want %v, got %v", first, snap[0].RegisteredAt)
	}

	if err := s.Touch(ctx, "w1", 10*time.Second); err != nil {
		t.Fatalf("touch: %v", err)
	}
	if err := s.Touch(ctx, "nobody", 10*time.Second); !errors.Is(err, bl.ErrUnknownWorker) {
		t.Fatalf("touch unknown: want ErrUnknownWorker, got %v", err)
	}
	if err := s.Delete(ctx, "w1"); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if err := s.Delete(ctx, "w1"); !errors.Is(err, bl.ErrUnknownWorker) {
		t.Fatalf("delete twice: want ErrUnknownWorker, got %v", err)
	}
}

func TestTableRegistryStoreWatch(t *testing.T) {
	conn := tablesConn(t)
	s := openTableStore(t, conn, fmt.Sprintf("watch%d", time.Now().UnixNano()%1e9), nil)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	ch, err := s.Watch(ctx)
	if err != nil {
		t.Fatalf("watch: %v", err)
	}
	if err := s.PutRegistrations(ctx, "w1", []bl.ProcessRegistration{testRegistration("proc-a", "w1")}, 600*time.Millisecond); err != nil {
		t.Fatalf("put: %v", err)
	}
	if u := nextUpdate(t, ch); u.Kind != bl.RegistryUpdateAdded || u.Registration.ProcessID != "proc-a" {
		t.Fatalf("want Added proc-a, got %+v", u)
	}
	// No touch: the TTL lapses and the loss is emitted.
	if u := nextUpdate(t, ch); u.Kind != bl.RegistryUpdateHeartbeatLost {
		t.Fatalf("want HeartbeatLost, got %+v", u)
	}
	// Re-register, then unregister: Added then Removed.
	if err := s.PutRegistrations(ctx, "w1", []bl.ProcessRegistration{testRegistration("proc-a", "w1")}, 10*time.Second); err != nil {
		t.Fatalf("re-put: %v", err)
	}
	if u := nextUpdate(t, ch); u.Kind != bl.RegistryUpdateAdded {
		t.Fatalf("want Added after revive, got %+v", u)
	}
	if err := s.Delete(ctx, "w1"); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if u := nextUpdate(t, ch); u.Kind != bl.RegistryUpdateRemoved {
		t.Fatalf("want Removed, got %+v", u)
	}
}

func TestTableRegistryStoreTimers(t *testing.T) {
	conn := tablesConn(t)
	s := openTableStore(t, conn, fmt.Sprintf("timer%d", time.Now().UnixNano()%1e9), nil)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	now := time.Now()
	if err := s.PutTimer(ctx, "inst-due", now.Add(-time.Second)); err != nil {
		t.Fatalf("put due: %v", err)
	}
	if err := s.PutTimer(ctx, "inst-future", now.Add(time.Hour)); err != nil {
		t.Fatalf("put future: %v", err)
	}
	due, err := s.DueTimers(ctx, now)
	if err != nil {
		t.Fatalf("due: %v", err)
	}
	if len(due) != 1 || due[0] != "inst-due" {
		t.Fatalf("want [inst-due], got %v", due)
	}
	// Claimed atomically: a second call finds nothing due.
	due, err = s.DueTimers(ctx, now)
	if err != nil {
		t.Fatalf("due 2: %v", err)
	}
	if len(due) != 0 {
		t.Fatalf("timer must be claimed exactly once, got %v", due)
	}
	if err := s.DeleteTimer(ctx, "inst-future"); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if err := s.DeleteTimer(ctx, "inst-future"); err != nil {
		t.Fatalf("delete absent must be a no-op, got %v", err)
	}
}

func TestTableRegistryStoreInstanceRecords(t *testing.T) {
	conn := tablesConn(t)
	s := openTableStore(t, conn, fmt.Sprintf("inst%d", time.Now().UnixNano()%1e9), nil)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	corr := "corr-1"
	rec := azuresb.InstanceRecord{
		Key:            conformanceKeys[0],
		CorrelationKey: &corr,
		Latest:         []byte{1, 2, 3},
	}
	if err := s.PutInstanceRecord(ctx, "inst-1", rec); err != nil {
		t.Fatalf("put: %v", err)
	}
	got, err := s.GetInstanceRecord(ctx, "inst-1")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if got == nil || got.Key != rec.Key || *got.CorrelationKey != corr || len(got.Latest) != 3 {
		t.Fatalf("record round trip: %+v", got)
	}
	if got, err := s.GetInstanceRecord(ctx, "nope"); err != nil || got != nil {
		t.Fatalf("absent record: want (nil, nil), got (%+v, %v)", got, err)
	}
	// Finished past retention reads as absent.
	rec.Finished = true
	rec.ExpiresAt = time.Now().Add(-time.Minute)
	if err := s.PutInstanceRecord(ctx, "inst-1", rec); err != nil {
		t.Fatalf("put expired: %v", err)
	}
	if got, err := s.GetInstanceRecord(ctx, "inst-1"); err != nil || got != nil {
		t.Fatalf("expired record: want (nil, nil), got (%+v, %v)", got, err)
	}
}

// ===== helpers =====

func testRegistration(processID, workerID string) bl.ProcessRegistration {
	contract, err := bl.NewInputContract(bl.RequiredField("amount", bl.TypeNumber))
	if err != nil {
		panic(err)
	}
	return bl.ProcessRegistration{
		Namespace:    "example.com/conformance",
		ProcessID:    processID,
		Version:      "1.0",
		StartEvents:  []bl.StartEventInfo{{Id: "start", InputContract: contract}},
		WorkerID:     workerID,
		RegisteredAt: time.Now(),
	}
}

func nextUpdate(t *testing.T, ch <-chan bl.RegistryUpdate) bl.RegistryUpdate {
	t.Helper()
	select {
	case u, ok := <-ch:
		if !ok {
			t.Fatal("watch channel closed unexpectedly")
		}
		return u
	case <-time.After(8 * time.Second):
		t.Fatal("timed out waiting for a registry update")
		return bl.RegistryUpdate{}
	}
}

func openTableStore(t *testing.T, conn, table string, cipher bl.PayloadCipher) *azuresb.TableRegistryStore {
	t.Helper()
	s, err := azuresb.NewTableRegistryStore(azuresb.TableRegistryStoreConfig{
		ConnectionString: conn,
		TableName:        table,
		Cipher:           cipher,
	})
	if err != nil {
		t.Fatalf("open table store: %v", err)
	}
	t.Cleanup(func() { _ = s.Close() })
	return s
}

func conformanceQueues() []string {
	queues := make([]string, len(conformanceKeys))
	for i, key := range conformanceKeys {
		queues[i] = azuresb.JobsQueueName(testPrefix, key)
	}
	return queues
}

// drainQueues removes leftover messages from the shared emulator queues so
// each open starts clean. Messages still locked by a previous (closed)
// broker become receivable when their lock expires, so the loop waits out
// active-but-unreceivable messages up to a bounded deadline.
func drainQueues(t *testing.T, client *azservicebus.Client, queues ...string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	for _, q := range queues {
		receiver, err := client.NewReceiverForQueue(q, &azservicebus.ReceiverOptions{
			ReceiveMode: azservicebus.ReceiveModeReceiveAndDelete,
		})
		if err != nil {
			t.Fatalf("drain receiver %s: %v", q, err)
		}
		deadline := time.Now().Add(2*testInFlight + 2*time.Second)
		for {
			rctx, rcancel := context.WithTimeout(ctx, 400*time.Millisecond)
			msgs, _ := receiver.ReceiveMessages(rctx, 32, nil)
			rcancel()
			if len(msgs) > 0 {
				continue
			}
			pctx, pcancel := context.WithTimeout(ctx, 3*time.Second)
			peeked, _ := receiver.PeekMessages(pctx, 32, &azservicebus.PeekMessagesOptions{
				FromSequenceNumber: to.Ptr(int64(0)),
			})
			pcancel()
			active := 0
			for _, p := range peeked {
				if p.State == azservicebus.MessageStateActive {
					active++
				}
			}
			if active == 0 {
				break
			}
			if time.Now().After(deadline) {
				t.Fatalf("queue %s still holds %d active messages after drain deadline", q, active)
			}
			time.Sleep(400 * time.Millisecond)
		}
		_ = receiver.Close(ctx)
	}
}

// cleanupNamespaceEntities best-effort deletes a real namespace run's
// entities (the pump subscription is AutoDeleteOnIdle already).
func cleanupNamespaceEntities(t *testing.T, conn, prefix string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	adm, err := admin.NewClientFromConnectionString(conn, nil)
	if err != nil {
		return
	}
	for _, key := range conformanceKeys {
		_, _ = adm.DeleteQueue(ctx, azuresb.JobsQueueName(prefix, key), nil)
	}
	_, _ = adm.DeleteTopic(ctx, azuresb.InstanceEventsTopicName(prefix), nil)
}

// ===== environment setup =====

// serviceBusEnv resolves the Service Bus endpoint the conformance suite runs
// against: BLKIT_TEST_AZURESB_CONNECTION (a real namespace, the reliable
// route), or — opt-in via BLKIT_TEST_AZURESB_EMULATOR=1 — a locally started
// Service Bus emulator. With neither set the suite skips (the emulator
// proved unstable under this suite; see the file comment).
func serviceBusEnv(t *testing.T) testEnv {
	t.Helper()
	if conn := os.Getenv("BLKIT_TEST_AZURESB_CONNECTION"); conn != "" {
		return testEnv{sbConn: conn, tablesConn: tablesConn(t), emulator: false}
	}
	if os.Getenv("BLKIT_TEST_AZURESB_EMULATOR") == "" {
		t.Skip("set BLKIT_TEST_AZURESB_CONNECTION to a Service Bus namespace connection string to run the broker conformance suite " +
			"(or BLKIT_TEST_AZURESB_EMULATOR=1 to attempt the local emulator, which is known to be unstable under this suite)")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Minute)
	defer cancel()

	net, err := tcnetwork.New(ctx)
	if err != nil {
		t.Skipf("create docker network (set BLKIT_TEST_AZURESB_CONNECTION to use a real namespace): %v", err)
	}
	t.Cleanup(func() { _ = net.Remove(context.Background()) })

	sqlC, err := testcontainers.GenericContainer(ctx, testcontainers.GenericContainerRequest{
		ContainerRequest: testcontainers.ContainerRequest{
			Image:          "mcr.microsoft.com/mssql/server:2022-latest",
			Env:            map[string]string{"ACCEPT_EULA": "Y", "MSSQL_SA_PASSWORD": sqlPassword},
			Networks:       []string{net.Name},
			NetworkAliases: map[string][]string{net.Name: {"sbemulator-sql"}},
			WaitingFor:     wait.ForLog("Recovery is complete").WithStartupTimeout(3 * time.Minute),
		},
		Started: true,
	})
	if err != nil {
		t.Skipf("start SQL container for the Service Bus emulator: %v", err)
	}
	t.Cleanup(func() { _ = sqlC.Terminate(context.Background()) })

	cfgPath := filepath.Join(t.TempDir(), "Config.json")
	if err := os.WriteFile(cfgPath, emulatorConfigJSON(t), 0o644); err != nil {
		t.Fatalf("write emulator config: %v", err)
	}
	sbC, err := testcontainers.GenericContainer(ctx, testcontainers.GenericContainerRequest{
		ContainerRequest: testcontainers.ContainerRequest{
			Image: "mcr.microsoft.com/azure-messaging/servicebus-emulator:latest",
			Env: map[string]string{
				"ACCEPT_EULA":       "Y",
				"SQL_SERVER":        "sbemulator-sql",
				"MSSQL_SA_PASSWORD": sqlPassword,
			},
			Networks:     []string{net.Name},
			ExposedPorts: []string{"5672/tcp"},
			Files: []testcontainers.ContainerFile{{
				HostFilePath:      cfgPath,
				ContainerFilePath: "/ServiceBus_Emulator/ConfigFiles/Config.json",
				FileMode:          0o444,
			}},
			WaitingFor: wait.ForLog("Emulator Service is Successfully Up!").WithStartupTimeout(3 * time.Minute),
		},
		Started: true,
	})
	if err != nil {
		t.Skipf("start Service Bus emulator: %v", err)
	}
	t.Cleanup(func() { _ = sbC.Terminate(context.Background()) })
	host, err := sbC.Host(ctx)
	if err != nil {
		t.Fatalf("emulator host: %v", err)
	}
	port, err := sbC.MappedPort(ctx, "5672/tcp")
	if err != nil {
		t.Fatalf("emulator port: %v", err)
	}
	sbConn := fmt.Sprintf("Endpoint=sb://%s:%s;SharedAccessKeyName=RootManageSharedAccessKey;SharedAccessKey=SAS_KEY_VALUE;UseDevelopmentEmulator=true;", host, port.Port())
	drain, err := azservicebus.NewClientFromConnectionString(sbConn, nil)
	if err != nil {
		t.Fatalf("drain client: %v", err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = drain.Close(ctx)
	})
	return testEnv{sbConn: sbConn, tablesConn: tablesConn(t), emulator: true, drain: drain}
}

// tablesConn prefers BLKIT_TEST_AZURE_TABLES_CONNECTION; otherwise it starts
// Azurite's table service.
func tablesConn(t *testing.T) string {
	t.Helper()
	if conn := os.Getenv("BLKIT_TEST_AZURE_TABLES_CONNECTION"); conn != "" {
		return conn
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()
	azuriteC, err := testcontainers.GenericContainer(ctx, testcontainers.GenericContainerRequest{
		ContainerRequest: testcontainers.ContainerRequest{
			Image:        "mcr.microsoft.com/azure-storage/azurite:latest",
			Cmd:          []string{"azurite-table", "--tableHost", "0.0.0.0", "--tablePort", "10002", "--skipApiVersionCheck"},
			ExposedPorts: []string{"10002/tcp"},
			WaitingFor:   wait.ForListeningPort("10002/tcp").WithStartupTimeout(2 * time.Minute),
		},
		Started: true,
	})
	if err != nil {
		t.Skipf("start Azurite (set BLKIT_TEST_AZURE_TABLES_CONNECTION to use an existing table service): %v", err)
	}
	t.Cleanup(func() { _ = azuriteC.Terminate(context.Background()) })
	host, err := azuriteC.Host(ctx)
	if err != nil {
		t.Fatalf("azurite host: %v", err)
	}
	port, err := azuriteC.MappedPort(ctx, "10002/tcp")
	if err != nil {
		t.Fatalf("azurite port: %v", err)
	}
	return fmt.Sprintf("DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;TableEndpoint=http://%s:%s/devstoreaccount1;", host, port.Port())
}

// emulatorConfigJSON declares every entity the conformance run needs: the
// fixed process keys' job queues (5s lock ↔ InFlightTimeout), the
// instance-events topic, and the pump-subscription pool broker handles claim
// round-robin (the emulator does not support runtime entity management).
func emulatorConfigJSON(t *testing.T) []byte {
	t.Helper()
	type props map[string]any
	queueProps := props{
		"DeadLetteringOnMessageExpiration":    false,
		"DefaultMessageTimeToLive":            "PT1H",
		"DuplicateDetectionHistoryTimeWindow": "PT20S",
		"ForwardDeadLetteredMessagesTo":       "",
		"ForwardTo":                           "",
		"LockDuration":                        "PT5S",
		"MaxDeliveryCount":                    10,
		"RequiresDuplicateDetection":          false,
		"RequiresSession":                     false,
	}
	var queues []props
	for _, q := range conformanceQueues() {
		queues = append(queues, props{"Name": q, "Properties": queueProps})
	}
	var subs []props
	for i := range pumpPoolSize {
		subs = append(subs, props{
			"Name": fmt.Sprintf("pump-%02d", i),
			"Properties": props{
				"DeadLetteringOnMessageExpiration": false,
				"DefaultMessageTimeToLive":         "PT1H",
				"LockDuration":                     "PT30S",
				"MaxDeliveryCount":                 10,
				"ForwardDeadLetteredMessagesTo":    "",
				"ForwardTo":                        "",
				"RequiresSession":                  false,
			},
		})
	}
	cfg := props{
		"UserConfig": props{
			"Namespaces": []props{{
				"Name":   "sbemulatorns",
				"Queues": queues,
				"Topics": []props{{
					"Name": azuresb.InstanceEventsTopicName(testPrefix),
					"Properties": props{
						"DefaultMessageTimeToLive":            "PT1H",
						"DuplicateDetectionHistoryTimeWindow": "PT20S",
						"RequiresDuplicateDetection":          false,
					},
					"Subscriptions": subs,
				}},
			}},
			"Logging": props{"Type": "Console"},
		},
	}
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		t.Fatalf("marshal emulator config: %v", err)
	}
	return data
}
