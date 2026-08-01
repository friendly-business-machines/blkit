---
name: GooglePubSubMessageBroker
description: Google Cloud Pub/Sub message-broker backend — a jobs topic with per-key filtered subscriptions, per-subscriber ordered subscriptions with last-event-record replay for instance events, and a RegistryStore (Firestore) for the registry, timers, and last-event records. Its own module under brokers/google-pubsub.
status: implemented
code:
  - brokers/google-pubsub/
implements: specs/message-brokers/overview.spec.md
---

# Google Pub/Sub Message Broker

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
   one pull **Subscription per process key**. Pub/Sub holds a message
   in-flight until acked or the ack deadline lapses (minimum 10s); a
   lease-extension goroutine caps extension at `InFlightTimeout`. Terminal
   lifecycle reports and `ReportSuspended` issue `Ack()`. A fetcher whose
   ctx dies **nacks** its unsettled jobs for prompt redelivery; a hard
   crash redelivers via ack-deadline lapse. Dead-lettering at a
   max-delivery-attempts threshold is documented but not yet implemented.
2. **Selective consumption** — each key's Subscription carries a
   server-side **attribute filter** (`attributes.key = "..."`), **plus an
   always-on client-side skip-and-ack** — the emulator accepts but does not
   enforce filters, and each key owns its subscription so skipped messages
   are its own copies. Per-key subscriptions replace the specced
   per-worker-pool `IN (...)` filter (filters are immutable per
   subscription; per-key granularity avoids replacement churn when a
   capability set changes).
3. **Registry — RegistryStore** — `FirestoreRegistryStore` implements the
   core `RegistryStore` interface: one document per worker with
   envelope-encoded registrations and a deadline; client-side TTL filtering
   plus a janitor sweep; `Watch` is a **~200ms polling diff loop** (not an
   `onSnapshot` listener). Registration stamping (WorkerID /
   RegisteredAt-preserved / LastHeartbeat) happens store-side. `Submit`
   reads the store directly (immediately consistent), so there is no
   cold-start snapshot wait.
4. **Per-instance events / fan-out / replay** — one **events Topic**
   (`<prefix>-inst-events`) with a per-subscriber filtered **and ordered**
   Subscription (ordering key = instance id; `expirationPolicy.ttl` for
   cleanup; deleted on unsubscribe). Latest-event replay comes from
   **last-event records in the RegistryStore** (latest lifecycle + terminal
   envelopes + process key + correlation key, upserted on lifecycle
   publishes) — not from topic retention + `Seek`, whose emulator support
   is unreliable; `Config.Registry` must implement the module's
   `InstanceStore` extension (FirestoreRegistryStore does). Duplicates
   between replay and live delivery are suppressed by an `eventID`
   attribute. Subscription creation is quota-bound and takes ~100ms–seconds;
   this is the backend's weakest point.
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
       Registry     bl.RegistryStore    // required; must also implement this module's InstanceStore (FirestoreRegistryStore does)
       Endpoint     string              // development only; emulator endpoint override
       Cipher       bl.PayloadCipher    // optional end-to-end payload encryption; default nil

       RegistrationTTL time.Duration // default 90s
       InFlightTimeout time.Duration // default 150s; lease-extension cap (ack deadline min 10s)
       EventRetention  time.Duration // default 1h; last-event record lifetime
   }
   ```

9. **Local testing** — the conformance suite starts one **gcloud Pub/Sub
   emulator** and one **Firestore emulator** container per test binary via
   testcontainers-go's gcloud module. `BLKIT_TEST_GPUBSUB_ENDPOINT`,
   `BLKIT_TEST_FIRESTORE_ENDPOINT`, and `BLKIT_TEST_GPUBSUB_PROJECT`
   override them. Redelivery-by-deadline is skipped in conformance
   (`InFlightTimeout: 0` — the 10s minimum ack deadline exceeds the suite's
   wait budget); a probe test documents that the emulator accepts but does
   not enforce subscription filters, which the always-on client-side
   filtering compensates for.

## Notes

- **Ordering keys** are used on the **events topic** (ordering key =
  instance id) so lifecycle → terminal → close order holds. Jobs are
  unordered — the at-least-once + state-store-check model does not require
  ordering there; an opt-in ordered-jobs variant is not implemented.
- **Dead-lettering**: a per-subscription DLQ topic at a
  max-delivery-attempts threshold is the intended fallback for repeated
  worker crashes; `ReportFailed` is the normal failure path. Not yet
  implemented.
- **Shutdown nacks**: `sub.Receive` blocks shutdown until outstanding
  messages settle, so a closing fetcher nacks unsettled jobs — which is
  also the production-correct prompt-redelivery behavior.
- All payloads are [CBOR envelopes](overview.spec.md#wire-format) in the
  message body; Pub/Sub attributes carry the cleartext routing metadata.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
