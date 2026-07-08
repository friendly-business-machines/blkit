# Azure Service Bus

> A cloud-managed broker on Azure — peek-lock queues for jobs, one instance-event
> topic with client-side fan-out, native scheduled messages for timers, and a
> RegistryStore in Table Storage or Cosmos DB.

The Azure Service Bus backend implements [MessageBroker](overview.md) against
Service Bus queues and topics. Service Bus brings durable queues, peek-lock
delivery, and — uniquely among the supported backends — **native scheduled
messages**, which give it the cleanest suspend-resume timer story. It is a good
fit for a deployment already on Azure, and especially for workflows that lean on
timer-driven waits.

Service Bus has no key-value store, so the worker registry, timer records, and
the last-event records that seed late subscribers live in a required
**RegistryStore** side-store — typically **Azure Table Storage** (Azurite-
compatible in tests) or Cosmos DB. You construct it and pass it in; the broker
never opens it for you.

```go
import (
    "log"

    "github.com/Azure/azure-sdk-for-go/sdk/azidentity"
    azsbbroker "github.com/friendly-business-machines/blkit/brokers/azure-service-bus"
)

cred, err := azidentity.NewDefaultAzureCredential(nil)
if err != nil {
    log.Fatalf("azure credential: %v", err)
}

// Table Storage holds the worker registry, timer records, and the last-event
// records that let late subscribers replay. NewTableRegistryStore creates the
// table if absent and starts the TTL/retention sweeper.
registry, err := azsbbroker.NewTableRegistryStore(azsbbroker.TableRegistryStoreConfig{
    ServiceURL: "https://myacct.table.core.windows.net",
    Credential: cred,
    TableName:  "blkitregistry",
})
if err != nil {
    log.Fatalf("connect table registry: %v", err)
}
defer registry.Close()

// New connects to the namespace, ensures the instance-events topic and this
// handle's pump subscription exist, and starts the event pump; the returned
// *Broker is a bl.MessageBroker like any other backend. It never closes the
// registry — the caller owns it.
broker, err := azsbbroker.New(azsbbroker.Config{
    Namespace:  "myns.servicebus.windows.net",
    Credential: cred, // azcore.TokenCredential; Managed Identity in production
    Registry:   registry,
})
if err != nil {
    log.Fatalf("connect azure broker: %v", err)
}
defer broker.Close()
```

`Credential` is any `azcore.TokenCredential` — a Managed Identity in production,
`DefaultAzureCredential` in development. A SAS `ConnectionString` works instead
for both the broker and the table store. Several broker handles can share one
AMQP connection by passing a pre-built `Client`, which the broker then never
closes.

## What it's good for

- **Deployments already on Azure** that want a fully managed broker.
- **Workflows with suspend-resume timers**, which map onto native scheduled
  messages more cleanly than on any other backend — no scheduler poll loop.
- Teams comfortable pairing Service Bus with **Table Storage** or Cosmos DB for
  the registry side-store.

## Provisioning

There is no server to run: Service Bus and Table Storage are managed Azure
services. What you provide is a namespace, a storage account (or Cosmos DB), and
a credential — a Managed Identity in production. On a real namespace the broker
creates the entities it needs on demand through the admin client: a jobs queue
per process key, the instance-events topic, and this handle's pump subscription.

For local development the Service Bus **emulator** container plus **Azurite** (the
Table Storage emulator) can stand in, reached through `UseEmulator` and a
`ConnectionString`. One caveat matters: the emulator **cannot create entities at
runtime** — every queue and subscription must be predeclared in its config JSON,
and you must name a predeclared subscription in `PumpSubscription`. The
RegistryStore against Azurite has no such limitation.

## How it works

Each broker duty maps onto Service Bus primitives as follows. Every entity name
carries the configured `EntityPrefix` (default `blkit`).

- **Job queue, ack, and redelivery** — one Service Bus **queue per process key**
  (`<prefix>-jobs.<ns>.<proc>.<ver>`, with each component injectively escaped),
  created on demand via the admin client. Delivery is **peek-lock**: a delivered
  job stays locked while a renewal goroutine keeps the lock alive, and a terminal
  lifecycle report or `ReportSuspended` `Complete()`s it. There is deliberately no
  `Defer()`/`ReceiveDeferredMessage` dance — an input wait resumes via a fresh
  `JobRespondToInput` and a timer wait via a scheduled `JobResume`, which is
  simpler and equivalent under at-least-once. A worker that dies stops renewing,
  the lock expires, and Service Bus redelivers; `Close` abandons unsettled
  in-flight messages so they redeliver promptly. Repeated expiry dead-letters at
  `MaxDeliveryCount` into the queue's built-in DLQ.
- **Selective consumption** — a worker receives only from the queues for the
  process keys it registered.
- **Registry** — the TableRegistryStore keeps one **entity per worker** on Azure
  Table Storage, holding the envelope-encoded registration set and a **deadline**.
  A client-side **sweeper** enforces the TTL, but only after a short grace past
  the deadline, so every `Watch` poller observes the expiry before the entity
  disappears — that is what lets an expiry surface as `HeartbeatLost` rather than
  looking like a plain `Removed`. `Watch` is a **~200 ms polling diff loop**;
  there is no change-feed topic. Timer records are claimed with ETag-conditioned
  deletes so concurrent claimers never double-fire.
- **Per-instance events, fan-out, and replay** — one Service Bus **topic**
  (`<prefix>-inst-events`) with **one subscription per broker handle**, created at
  construction (with `AutoDeleteOnIdle` for cleanup) and drained by a single pump
  goroutine that dispatches events **in-process** to that handle's subscribers by
  instance id. This is a deliberate bounded-entity design: however many
  subscribers a handle has, it never creates more Service Bus entities. Because a
  subscription only receives messages published after it exists, late-subscriber
  replay comes from **last-event records** kept in the RegistryStore (latest
  lifecycle envelope, terminal envelope, process key, correlation key); a custom
  RegistryStore that does not implement the module's record interface falls back
  to a per-handle in-memory store, in which case replay only works within one
  handle. A terminal sequence carries a `blkitFinal` property so every handle
  closes the instance's subscriber channels exactly once.
- **Suspend-resume timers** — native **`ScheduledEnqueueTime`** on the
  `JobResume` message: Service Bus itself holds the resume until it is due, so
  there is no scheduler loop. When an instance finishes, its still-pending
  scheduled resumes are cancelled best-effort with `CancelScheduledMessages`.
- **Cancel of a queued job** — **unsupported natively**: Service Bus cannot delete
  an arbitrary queued message, so `Cancel` always takes the `JobCancel` route and
  always requires `AllowExternalCancel`.

## Configuration

The broker's own `Config`:

```go
type Config struct {
    Namespace        string                 // "myns.servicebus.windows.net"; with Credential
    ConnectionString string                 // SAS auth; or set Namespace+Credential
    Credential       azcore.TokenCredential // Azure AD / Managed Identity (recommended)
    Client           *azservicebus.Client   // optional: share one AMQP connection; never closed by the broker
    EntityPrefix     string                 // default "blkit"; isolates deployments sharing a namespace
    Registry         bl.RegistryStore       // required; should also implement InstanceRecordStore (TableRegistryStore does)
    PumpSubscription string                 // optional predeclared pump subscription; required with UseEmulator
    UseEmulator      bool                   // development only; permits the emulator's non-TLS endpoint
    Cipher           bl.PayloadCipher       // optional end-to-end payload encryption; default nil

    RegistrationTTL time.Duration // default 90s (3× the 30s heartbeat interval)
    InFlightTimeout time.Duration // default 150s; queue lock duration (Service Bus minimum 5s)
    EventRetention  time.Duration // default 1h; last-event record and topic message lifetime
}
```

- **Authentication** — supply either a `ConnectionString` (SAS) or
  `Namespace` + `Credential` (Azure AD / Managed Identity, recommended). A
  pre-built `Client` can be shared across handles; the broker never closes a
  supplied client.
- **`EntityPrefix`** — namespaces every queue, topic, and subscription this
  broker creates. Change it to run several independent blkit deployments in one
  namespace without collision.
- **`Registry`** — the required side-store; it should also implement the module's
  `InstanceRecordStore` (the last-event records), which `TableRegistryStore` does.
- **`PumpSubscription` / `UseEmulator`** — development knobs for the Service Bus
  emulator, which cannot create entities at runtime; ignore both against a real
  namespace.
- **`Cipher`** — an optional [`PayloadCipher`](../reference/blkit.md) encrypting
  payloads end to end. All brokers, workers, and the RegistryStore in a
  deployment must share the same cipher.
- **The timing knobs**:
  - **`RegistrationTTL`** (90 s) — how long a worker's registration outlives its
    last heartbeat before the sweeper declares it lost.
  - **`InFlightTimeout`** (150 s) — how long a delivered job may go unsettled
    before Service Bus redelivers it. It maps onto the queue's peek-lock
    duration, which Service Bus **clamps to a 5 s minimum**: the effective value
    is `max(InFlightTimeout, 5s)`. It must comfortably exceed your longest task
    step.
  - **`EventRetention`** (1 h) — how long a finished instance's last-event record
    stays replayable; it is also the topic and subscription
    `DefaultMessageTimeToLive`.

The RegistryStore has its own `Config`:

```go
type TableRegistryStoreConfig struct {
    ConnectionString string                 // Azure Storage connection string (Azurite in tests)
    ServiceURL       string                 // or ServiceURL + Credential
    Credential       azcore.TokenCredential
    TableName        string                 // required; created if absent
    Cipher           bl.PayloadCipher       // optional; must match the broker's

    PollInterval  time.Duration // Watch polling cadence; default 200ms
    SweepInterval time.Duration // TTL/retention sweeper cadence; default 500ms
    SweepGrace    time.Duration // grace past deadline before a worker entity is deleted; default 2s
}
```

- **`TableName`** — the single table holding workers, timers, and instance
  records; created on first use. **`Cipher`** must match the broker's, since
  registrations are stored envelope-encoded.
- **`PollInterval`** (200 ms) is the `Watch` cadence; **`SweepInterval`** (500 ms)
  is how often the sweeper runs; **`SweepGrace`** (2 s) is how long an expired
  worker entity survives past its deadline so pollers can emit `HeartbeatLost`
  before it vanishes.

## Local testing

Service Bus is the one backend whose [conformance suite](overview.md#conformance)
prefers a **live endpoint**: set `BLKIT_TEST_AZURESB_CONNECTION` to a real
namespace (the suite creates and cleans up entities under a per-run prefix), and
`BLKIT_TEST_AZURE_TABLES_CONNECTION` for the Table Storage store — or let it
auto-start **Azurite** for the store. The suite skips cleanly when neither is
available. Microsoft's Service Bus emulator proved unreliable for automated
conformance (it has no runtime entity management and was unstable under load), so
the emulator route is opt-in behind `BLKIT_TEST_AZURESB_EMULATOR=1` with a
generated config that predeclares the conformance entities. The
`TableRegistryStore` itself is always covered locally against Azurite.

## What to keep in mind

- **One queue per process key.** A deployment with very many distinct process
  keys creates a correspondingly large number of Service Bus queues; this is fine
  for typical process counts but worth knowing at the extremes.
- **Cancel is best-effort by design** — a queued job cannot be pulled back, so
  cancellation is a `JobCancel` the worker honours, and only when the process
  opted in with `AllowExternalCancel`.
- **The emulator cannot create entities at runtime**, so local development
  against it requires predeclared entities and a named `PumpSubscription`; a real
  namespace has neither constraint.
- **One `EntityPrefix` (and table) per deployment** when sharing a namespace, or
  two deployments will read each other's queues and registry.

## Reference

The backend's API is in the [Azure Service Bus reference](../reference/brokers-azure-service-bus.md);
the `MessageBroker` interface it implements is in the core [Reference](../reference/blkit.md).
