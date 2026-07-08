# Google Pub/Sub

> A cloud-managed broker on Google Cloud — a jobs topic with per-key filtered
> subscriptions, per-subscriber ordered subscriptions with last-event-record
> replay for instance events, and a Firestore RegistryStore for the registry,
> timers, and last-event records.

The Google Pub/Sub backend implements [MessageBroker](overview.md) against
standard Cloud Pub/Sub — at-least-once delivery with per-message
acknowledgement, server-side attribute filters, and ordered delivery. It is the
most workaround-heavy of the supported backends: Pub/Sub has no message deletion,
no native delayed delivery, and quota-bound subscription management, so several
duties are met indirectly, and each is described below. Pub/Sub **Lite** is not
supported — it is a distinct product with partitioned-log semantics.

Pub/Sub has no key-value store, so the worker registry, durable timer records,
and the last-event records that seed late subscribers live in a required
**RegistryStore** side-store — typically **Firestore**. You construct it and pass
it in; the broker never opens it for you, and it will not construct without one.

```go
import (
    "log"

    gpsbroker "github.com/friendly-business-machines/blkit/brokers/google-pubsub"
)

// Firestore holds the worker registry, the durable timer records that stand in
// for Pub/Sub's missing delayed delivery, and the last-event records that let
// late subscribers replay. NewFirestoreRegistryStore connects and starts the
// janitor that sweeps expired documents.
registry, err := gpsbroker.NewFirestoreRegistryStore(gpsbroker.FirestoreConfig{
    ProjectID: "my-project",
})
if err != nil {
    log.Fatalf("connect firestore registry: %v", err)
}
defer registry.Close()

// New connects to Pub/Sub, ensures the jobs and instance-events topics exist,
// and starts the timer scheduler; the returned *Broker is a bl.MessageBroker
// the rest of blkit uses the same way as any other backend. It does not close
// the registry — the caller owns it.
broker, err := gpsbroker.New(gpsbroker.Config{
    ProjectID: "my-project",
    Registry:  registry,
})
if err != nil {
    log.Fatalf("connect pubsub broker: %v", err)
}
defer broker.Close()
```

With `Credentials` left nil both clients use Application Default Credentials, so
on properly configured GCP infrastructure no secrets appear in code.

## What it's good for

- **Deployments already on Google Cloud** that want a managed broker with no
  server to run.
- Teams comfortable pairing Pub/Sub with **Firestore** for the registry
  side-store.

## Provisioning

There is no server to run: Pub/Sub and Firestore are managed Google Cloud
services, and the broker creates the entities it needs — a jobs topic, an
instance-events topic, per-key job subscriptions, and per-subscriber event
subscriptions — on demand. What you provide is a project, credentials with
permission to manage and use those entities, and a Firestore database. Note that
subscription creation is **quota-bound and takes ~100 ms to seconds**, which is
this backend's weakest point (see [What to keep in mind](#what-to-keep-in-mind)).

For local development and the [conformance suite](#local-testing) the same code
runs against the **gcloud Pub/Sub emulator** and the **Firestore emulator**,
reached through the development-only `Endpoint` override on the broker and the
RegistryStore. Point `Endpoint` at each emulator's host and everything else stays
the same.

## How it works

Each broker duty maps onto Pub/Sub primitives as follows. Every entity name
carries the configured `EntityPrefix` (default `blkit`); the Firestore
collections carry `CollectionPrefix`.

- **Job queue, ack, and redelivery** — one **jobs topic** (`<prefix>-jobs`) with
  one pull **subscription per process key**, created with a server-side
  **attribute filter** on the key. Pub/Sub holds a message in-flight until it is
  acked or its **ack deadline** (minimum 10 s) lapses; a lease-extension
  goroutine keeps a long job leased up to `InFlightTimeout`. A terminal lifecycle
  report and `ReportSuspended` `Ack()` the message. A fetcher whose context ends
  **nacks** its unsettled jobs for prompt redelivery to another worker; a hard
  crash redelivers when the ack deadline lapses. Messages that fail to decode
  (poison messages) are acked and never processed.
- **Selective consumption** — each key's subscription carries the server-side
  attribute filter (`attributes.key = "..."`), **plus an always-on client-side
  skip-and-ack**: the emulator accepts but does not enforce filters, and because
  each key owns its own subscription any skipped message is that subscription's
  own copy, so acking it is safe. Per-key subscriptions (rather than one filtered
  per-worker-pool subscription) avoid replacement churn, since a subscription's
  filter is immutable once created.
- **Registry** — the FirestoreRegistryStore keeps one **document per worker**
  holding the envelope-encoded registrations and a **deadline**. Expiry is
  client-side: reads drop past-deadline documents and a **janitor** sweeps them.
  `Watch` is a **~200 ms polling diff loop** — not a Firestore `onSnapshot`
  listener — that emits `HeartbeatLost` for a worker that vanished past its
  deadline and `Removed` for one deleted while still live; a per-document `rev`
  that changes only on re-registration lets it detect a replaced registration
  set.
- **Per-instance events, fan-out, and replay** — one **events topic**
  (`<prefix>-inst-events`, message-ordering enabled) with a per-subscriber
  filtered **and ordered** subscription (ordering key = instance id, an
  expiration policy for cleanup, deleted on unsubscribe). Ordering guarantees the
  lifecycle → terminal → close sequence holds. Late-subscriber replay comes from
  the **last-event records in the RegistryStore** — not from topic retention plus
  `Seek`, whose emulator support is unreliable and which is slower in production
  too — so `Config.Registry` must implement the module's `InstanceStore`
  interface (`FirestoreRegistryStore` does). A duplicate between the replay and
  live delivery is suppressed by an `eventID` attribute.
- **Suspend-resume timers** — **none natively**. A duration or datetime wait
  writes a **timer record** to the RegistryStore, and a broker-owned scheduler
  loop (200 ms poll) claims due timers in a Firestore transaction — so concurrent
  brokers never double-fire — and publishes the `JobResume` to the jobs topic.
- **Cancel of a queued job** — **unsupported natively**: Pub/Sub has no message
  deletion, so `Cancel` always takes the `JobCancel` route and always requires
  `AllowExternalCancel`.

## Configuration

The broker's own `Config`:

```go
type Config struct {
    ProjectID    string              // GCP project owning the topics/subscriptions; required
    Credentials  *google.Credentials // nil = Application Default Credentials
    EntityPrefix string              // default "blkit"; isolates deployments sharing a project
    Registry     bl.RegistryStore    // required; must also implement InstanceStore (FirestoreRegistryStore does)
    Endpoint     string              // development only; emulator endpoint override
    Cipher       bl.PayloadCipher    // optional end-to-end payload encryption; default nil

    RegistrationTTL time.Duration // default 90s (3× the 30s heartbeat interval)
    InFlightTimeout time.Duration // default 150s; lease-extension cap (ack deadline minimum 10s)
    EventRetention  time.Duration // default 1h; last-event record lifetime
}
```

- **`ProjectID`, `Credentials`** — how to reach Pub/Sub. Leave `Credentials` nil
  to use Application Default Credentials.
- **`EntityPrefix`** — namespaces every topic and subscription this broker
  creates. Change it to run several independent blkit deployments in one project
  without collision.
- **`Registry`** — the required side-store. It must also implement the module's
  `InstanceStore` interface (the last-event records) — without them Pub/Sub
  cannot replay to a late subscriber — which `FirestoreRegistryStore` does, so a
  single store satisfies both roles.
- **`Cipher`** — an optional [`PayloadCipher`](../reference/blkit.md) that
  encrypts payloads end to end, so Google never sees plaintext. All brokers,
  workers, and the RegistryStore in a deployment must share the same cipher.
- **The three timing knobs**:
  - **`RegistrationTTL`** (90 s) — how long a worker's registration outlives its
    last heartbeat before it is declared lost.
  - **`InFlightTimeout`** (150 s) — how long a delivered job may go unsettled
    before Pub/Sub redelivers it. It caps lease extension; a value below Pub/Sub's
    **10 s minimum ack deadline** is raised to it. It must comfortably exceed your
    longest task step.
  - **`EventRetention`** (1 h) — how long after an instance finishes its
    last-event record stays replayable to late subscribers.

The RegistryStore has its own `Config`:

```go
type FirestoreConfig struct {
    ProjectID        string              // GCP project owning the Firestore database; required
    DatabaseID       string              // default "(default)"
    Credentials      *google.Credentials // nil = Application Default Credentials
    CollectionPrefix string              // default "blkit"; collections <prefix>-workers/-timers/-instances
    Endpoint         string              // development only; emulator endpoint override
    Cipher           bl.PayloadCipher    // optional; must match the broker's

    PollInterval   time.Duration // Watch/janitor polling cadence; default 200ms
    EventRetention time.Duration // finished-record retention before the janitor deletes; default 1h
}
```

- **`DatabaseID` / `CollectionPrefix`** — which Firestore database and which
  collection prefix hold the workers, timers, and instance records. Give each
  deployment its own prefix to keep them isolated.
- **`Cipher`** — must match the broker's, because registrations are stored
  envelope-encoded and the store decodes them for `Snapshot`/`Watch`.
- **`PollInterval`** (200 ms) is the cadence of both the `Watch` change feed and
  the janitor. **`EventRetention`** (1 h) bounds how long a finished instance
  record survives before the janitor sweeps it; keep it aligned with the broker's
  `EventRetention`.

## Local testing

The [conformance suite](overview.md#conformance) starts one **gcloud Pub/Sub
emulator** and one **Firestore emulator** container per test binary with
testcontainers-go's gcloud module. `BLKIT_TEST_GPUBSUB_ENDPOINT`,
`BLKIT_TEST_FIRESTORE_ENDPOINT`, and `BLKIT_TEST_GPUBSUB_PROJECT` override them to
point at instances you run yourself or a real project. Redelivery-by-deadline is
skipped under the emulator because the 10 s minimum ack deadline exceeds the
suite's wait budget, and a probe test documents that the emulator accepts but
does not enforce subscription filters — which the always-on client-side filtering
compensates for.

## What to keep in mind

- **Subscription creation is the slow path.** Each `SubscribeToInstance` creates a
  Pub/Sub subscription, which is quota-bound and takes ~100 ms to seconds — the
  backend's weakest point. A workload that subscribes to very many instances at
  once will feel this; budget for it and prefer longer-lived subscribers.
- **Timers are store-driven, not native.** Every suspend-with-resume becomes a
  Firestore timer record polled by the broker's scheduler, so resume latency is
  bounded by `PollInterval` (200 ms) rather than being exact.
- **Cancel is best-effort by design** — a queued job cannot be pulled back, so
  cancellation is a `JobCancel` the worker honours, and only when the process
  opted in with `AllowExternalCancel`.
- **Pub/Sub Lite is not supported** — it is a different product; use standard
  Cloud Pub/Sub.
- **One `EntityPrefix` (and `CollectionPrefix`) per deployment** when sharing a
  project, or two deployments will read each other's topics and registry.

## Reference

The backend's API is in the [Google Pub/Sub reference](../reference/brokers-google-pubsub.md);
the `MessageBroker` interface it implements is in the core [Reference](../reference/blkit.md).
