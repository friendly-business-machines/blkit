---
name: GooglePubSubMessageBroker
description: Google Cloud Pub/Sub message-broker backend — a filtered jobs topic, per-subscriber filtered subscriptions with retention+Seek replay for instance events, and a RegistryStore (Firestore) for the registry and timers. Its own module under brokers/google-pubsub.
targets:
  - ../../brokers/google-pubsub/broker.go
---

# Google Pub/Sub Message Broker

> **Status:** This spec is a work in progress. Implementation pending.

The Google Pub/Sub backend implements [MessageBroker](overview.spec.md)
against standard Cloud Pub/Sub. Pub/Sub provides at-least-once delivery with
per-message acknowledgement, server-side attribute filters, and seekable
topic retention. It is the most workaround-heavy of the supported backends —
no message deletion, no native delayed delivery, and quota-bound subscription
management — and this spec documents each workaround. Pub/Sub **Lite** is
explicitly not supported (a distinct product with partitioned-log semantics).

Pub/Sub has no KV store, so the worker registry and timer records live in a
pluggable [RegistryStore](overview.spec.md#the-registrystore), typically
Firestore.

```go
import gpsbroker "github.com/friendly-business-machines/blkit/brokers/google-pubsub"

broker, err := gpsbroker.New(gpsbroker.Config{
    ProjectID: "my-project",
    Registry:  firestoreRegistryStore,
})
```

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one **jobs Topic** (`<prefix>-jobs`) with
   one pull **Subscription per worker pool**. Pub/Sub holds a message
   in-flight until acked or the ack deadline lapses; a lease-extension
   goroutine (`ModifyAckDeadline`) covers long jobs. Terminal lifecycle
   reports and `ReportSuspended` issue `Ack()`. A crashed worker misses the
   deadline and Pub/Sub redelivers; repeated redelivery dead-letters at the
   subscription's max-delivery-attempts threshold (see notes).
2. **Selective consumption** — the worker-pool Subscription carries a
   server-side **attribute filter** on the process key
   (`attributes.key IN (...)` expressed in Pub/Sub filter syntax), evaluated
   at delivery time. Filters are immutable per subscription, so a changed
   capability set means a replacement subscription.
3. **Registry — RegistryStore** — **Firestore** implements the core
   `RegistryStore` interface: one document per worker with a TTL field
   (expired by a sweeper), and Firestore's `onSnapshot` listener as the
   `Watch` feed — a direct snapshot-then-updates mapping.
4. **Per-instance events / fan-out / replay** — one **events Topic**
   (`<prefix>-inst-events`) with topic **message retention** enabled
   (default 24h — the retention window). Each subscriber creates a filtered
   Subscription (`attributes.instanceID = "..."`, `expirationPolicy.ttl` for
   cleanup) and **Seeks** it back to the retention window, which delivers
   retained messages published *before* the subscription existed — that is
   the latest-event replay path. Subscription creation is quota-bound and
   takes seconds; this is the backend's weakest point and the reason
   long-lived client processes should reuse subscriptions where possible.
5. **Delayed delivery** — none natively. `ReportSuspended` for a
   duration/datetime wait writes a **timer record to the RegistryStore**
   (Firestore); a broker-owned scheduler loop claims due timers atomically
   and publishes the `JobResume` to the jobs topic.
6. **Cancel of queued jobs** — **unsupported natively** (Pub/Sub has no
   message deletion). `Cancel` always takes the `JobCancel` route.
7. **TLS** — always on through the Google SDK. The emulator is reached via
   an endpoint override (`PUBSUB_EMULATOR_HOST` semantics) — development
   only.
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       ProjectID    string              // GCP project owning the topics/subscriptions
       Credentials  *google.Credentials // nil = Application Default Credentials
       EntityPrefix string              // default "blkit"; isolates deployments sharing a project
       Registry     bl.RegistryStore    // required; Firestore implementation ships with this module
       Endpoint     string              // development only; emulator endpoint override
       Cipher       bl.PayloadCipher    // optional end-to-end payload encryption; default nil
   }
   ```

9. **Local testing** — the conformance suite starts the **gcloud Pub/Sub
   emulator** and **Firestore emulator** containers via testcontainers-go.
   `BLKIT_TEST_GPUBSUB_PROJECT` (with ambient credentials) points it at a
   real project instead. Emulator gaps (it does not enforce every quota or
   filter feature) are tagged; those conformance areas run against a real
   project in CI when credentials are present.

## Notes

- **Ordering keys** (FIFO per `instanceID`) are opt-in: they keep an
  instance's `JobCancel`/`JobResume` ordered behind its `JobStart` at the
  cost of pinning that instance's messages to one subscriber. Default off;
  the at-least-once + state-store-check model does not require them.
- **Dead-lettering**: a per-subscription DLQ topic catches messages that
  exhaust max delivery attempts (default 10) — the fallback for repeated
  worker crashes. `ReportFailed` is the normal failure path.
- All payloads are [CBOR envelopes](overview.spec.md#wire-format) in the
  message body; Pub/Sub attributes carry the cleartext routing metadata.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
