---
name: AzureServiceBusMessageBroker
description: Azure Service Bus message-broker backend — peek-lock queues for jobs, a per-handle pump subscription on one events topic, native scheduled messages for timers, and a RegistryStore (Table Storage / Cosmos DB) for the registry and last-event records. Its own module under brokers/azure-service-bus.
status: implemented
code:
  - brokers/azure-service-bus/
implements: specs/message-brokers/overview.spec.md
---

# Azure Service Bus Message Broker

The Azure Service Bus backend implements [MessageBroker](overview.spec.md)
against Service Bus queues and topics. Service Bus brings durable queues,
peek-lock delivery, and — uniquely among the supported backends — **native
scheduled messages**, the best suspend-resume timer story. Service Bus has no
KV store, so the worker registry and last-event records live in a pluggable
[RegistryStore](overview.spec.md#the-registrystore), typically Azure Table
Storage or Cosmos DB.

```go
import azsbbroker "github.com/friendly-business-machines/blkit/brokers/azure-service-bus"

broker, err := azsbbroker.New(azsbbroker.Config{
    Namespace:  "myns.servicebus.windows.net",
    Credential: cred, // azcore.TokenCredential (Managed Identity in production)
    Registry:   tableRegistryStore,
})
```

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one Service Bus **queue per
   `ProcessKey`** (`<prefix>-jobs.<ns>.<proc>.<ver>`, injectively escaped —
   queue-per-key maps onto Service Bus's native scaling unit, at the cost
   of entity count for many-process deployments), created on demand via the
   admin client. Delivery is **peek-lock** with a lock-renewal goroutine
   while in-flight; lock duration maps from `InFlightTimeout` with the
   Service Bus 5s minimum. Terminal lifecycle reports **and**
   `ReportSuspended` issue `Complete()` — there is no `Defer()` /
   `ReceiveDeferredMessage` dance: external-input waits resume via a fresh
   `JobRespondToInput` job and timer waits via a scheduled `JobResume`,
   which is simpler and equivalent under at-least-once. A crashed worker's
   lock expires and Service Bus redelivers (a dying fetch context just
   stops lock renewal); repeated expiry dead-letters at `MaxDeliveryCount`
   (see notes). `Close` abandons unsettled in-flight messages for prompt
   redelivery.
2. **Selective consumption** — workers receive only from their registered
   keys' queues.
3. **Registry — RegistryStore** — `TableRegistryStore` on Azure **Table
   Storage** (Azurite-compatible) implements the core `RegistryStore`
   interface: one entity per worker with envelope-encoded registrations and
   a deadline; a client-side sweeper expires lapsed workers (with a short
   grace so pollers observe the expiry before deletion — `Snapshot`
   includes entities until the sweeper deletes them, `Watch` emits
   `HeartbeatLost` promptly off the strict deadline). `Watch` is a
   **~200ms polling diff loop** — there is no `<prefix>-reg-feed` topic.
   Timer records use ETag-claimed optimistic concurrency. `Submit` reads
   the store synchronously (authoritative), so there is no cold-start
   snapshot wait.
4. **Per-instance events / fan-out / replay** — one Service Bus **Topic**
   (`<prefix>-inst-events`) with **one Subscription per broker handle**
   (created at construction, `AutoDeleteOnIdle` PT5M) and a pump goroutine
   that dispatches in-process to the handle's subscribers by instance id —
   a deliberate bounded-entity redesign: subscriber counts never create
   Service Bus entities, unlike per-subscriber SQL-filtered subscriptions.
   Because a Subscription only receives messages published after it
   exists, late-subscriber replay comes from **last-event records** (latest
   lifecycle + terminal envelopes + process key + correlation key) kept via
   the module's `InstanceRecordStore` extension interface, which
   `TableRegistryStore` implements; a custom RegistryStore lacking it falls
   back to a per-handle in-memory record store (replay then only works
   within one handle). Terminal sequences carry a `blkitFinal` application
   property so every handle closes subscriber channels exactly once.
5. **Delayed delivery** — native `ScheduledEnqueueTime` on the `JobResume`;
   scheduled resumes are cancelled best-effort (`CancelScheduledMessages`)
   when the instance finishes.
6. **Cancel of queued jobs** — not supported: Service Bus cannot delete an
   arbitrary queued message, so `Cancel` always takes the opt-in-checked
   `JobCancel` route (`SupportsQueueRemoval: false`).
7. **TLS** — always on (AMQPS/HTTPS through the Azure SDK). The emulator
   requires a connection string and a development-only insecure knob
   (`UseEmulator`), which never applies to a real namespace.
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       Namespace        string                 // "myns.servicebus.windows.net"
       ConnectionString string                 // SAS auth; or set Credential
       Credential       azcore.TokenCredential // Azure AD / Managed Identity (recommended)
       Client           *azservicebus.Client   // optional: share one AMQP connection across handles (never closed by the broker)
       EntityPrefix     string                 // default "blkit"; isolates deployments sharing a namespace
       Registry         bl.RegistryStore       // required; should also implement this module's InstanceRecordStore (TableRegistryStore does)
       PumpSubscription string                 // optional predeclared pump subscription; required with UseEmulator
       UseEmulator      bool                   // development only; permits the emulator's non-TLS endpoint
       Cipher           bl.PayloadCipher       // optional end-to-end payload encryption; default nil

       RegistrationTTL time.Duration // default 90s
       InFlightTimeout time.Duration // default 150s; lock duration (Service Bus minimum 5s)
       EventRetention  time.Duration // default 1h; last-event record lifetime
   }
   ```

9. **Local testing** — the conformance suite is **gated on a live
   endpoint**: `BLKIT_TEST_AZURESB_CONNECTION` (real namespace; dynamic
   entity creation under a per-open `EntityPrefix`, best-effort cleanup)
   plus `BLKIT_TEST_AZURE_TABLES_CONNECTION` (or auto-started Azurite),
   skipping cleanly otherwise. Microsoft's Service Bus **emulator proved
   unusable for automated conformance** in the reference environment — it
   has no runtime entity management (entities must be predeclared in its
   config JSON), and it exhibited fatal instability (gateway
   `NotImplementedException`, Winfab load-report failures, AMQP accept
   wedges). An emulator route is retained opt-in behind
   `BLKIT_TEST_AZURESB_EMULATOR=1` (generated config predeclaring the
   conformance entities + a pump-subscription pool) for healthier Docker
   hosts. The `TableRegistryStore` is always tested locally against
   **Azurite** (registrations/TTL/watch, ETag-claimed timers, instance
   records + retention).

## Notes

- **Sessions** (FIFO per `instanceID`) are opt-in via queue configuration:
  they keep an instance's `JobCancel`/`JobResume` ordered behind its
  `JobStart` and pinned to one consumer, at the cost of per-instance
  throughput. Default off; the at-least-once + state-store-check model does
  not require them.
- **Dead-lettering**: jobs that exhaust `MaxDeliveryCount` (default 10) land
  in the built-in DLQ as the fallback for repeated worker crashes;
  `ReportFailed` is the normal failure path and happens before DLQ ever
  applies.
- All payloads are [CBOR envelopes](overview.spec.md#wire-format) in the
  message body; application properties carry the cleartext routing metadata.
- Backpressure and fan-out follow the overview defaults.
- `Close` never closes a caller-supplied `Client` or the caller-owned
  `Registry`.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
