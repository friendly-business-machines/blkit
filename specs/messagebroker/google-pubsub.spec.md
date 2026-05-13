---
name: GooglePubSubBrokerGateway
description: Google Cloud Pub/Sub-backed implementation of BrokerGateway. Implementation pending; this stub records the placeholder.
targets:
  - ../messagebroker/googlepubsub_gateway.go
---

# GooglePubSubBrokerGateway

`GooglePubSubBrokerGateway` is the Google Cloud Pub/Sub-backed implementation of [BrokerGateway](overview.spec.md). Pub/Sub provides at-least-once delivery, per-subscription acknowledgment, ordering keys for FIFO-within-key, and attribute-based filters — sufficient primitives for the job queue and per-instance event topic.

Constructor:

```go
func NewGooglePubSubBrokerGateway(opts GooglePubSubOpts) (*GooglePubSubBrokerGateway, error)

type GooglePubSubOpts struct {
    // GCP project that owns the topics and subscriptions.
    ProjectID string

    // Authentication: a TokenSource (e.g. from google.DefaultTokenSource).
    // If nil, Application Default Credentials are used.
    Credentials *google.Credentials

    // Optional prefix prepended to all topic/subscription names so multiple
    // blkit deployments can share a single project.
    EntityPrefix string

    // Where to keep the registry and per-instance status records. Pub/Sub is
    // not a KV store, so blkit needs a side channel. See the Status section.
    RegistryStore RegistryStore // pluggable; typically FirestoreRegistryStore or SpannerRegistryStore
}
```

## Status

Implementation pending. This spec is a placeholder. Open design questions:

- **Job queue (`FetchJobs`)** — one Pub/Sub Topic for the job stream with one pull Subscription per worker pool, filtered by attribute (`processKey in (...)`) to honor the worker's capability set. Pub/Sub filters are evaluated server-side at delivery time, so workers receive only matching jobs. Alternative: a Topic per `(Namespace, ProcessID, Version)`, which avoids filters but inflates resource count.
- **Ack + outcome verbs** — Pub/Sub holds a message in-flight until the worker `Ack`s or the ack deadline expires. Worker outcome verbs map: `MarkCompleted` / `MarkCancelled` / `MarkFailed` issue `Ack()`; `ReenqueueSuspended` issues `Nack()` and the gateway republishes a `JobResume` to the same topic when the wait condition is satisfied (a `RespondToInputRequest` arriving, a timer firing). Crashed workers miss the ack deadline and Pub/Sub redelivers automatically. Lease management (`ModifyAckDeadline`) needs a background goroutine for long-running jobs.
- **Ordering keys for per-instance serialization** — Pub/Sub supports per-key FIFO via ordering keys. Using `instanceID` as the ordering key keeps `JobCancel` / `JobResume` ordered with respect to prior jobs for the same instance, at the cost of pinning that instance's jobs to one subscriber at a time. Document whether ordering is mandatory or opt-in.
- **Per-instance event topic (`SubscribeToInstance`)** — one Pub/Sub Topic for instance events, plus a per-subscriber Subscription with a filter on `attributes.instanceID = "..."`. Subscription cleanup on subscriber disconnect: Pub/Sub Subscriptions have `expirationPolicy.ttl`, so an idle subscription auto-deletes after a configured window (typically 24h–7d). Investigate whether 24h is short enough.
- **Registry change feed (`SubscribeToProcessRegistry`)** — Pub/Sub has no KV. Pattern: a separate `RegistryStore` interface backed by Firestore (snapshot via collection query + real-time listener for change events). Firestore's `onSnapshot` natively gives snapshot + updates semantics, so the mapping is direct. Alternative: a compacted Pub/Sub topic the gateway replays on subscribe, but Pub/Sub is not designed for replay so this is awkward.
- **Per-instance status record** — `Cancel` / `Terminate` need a synchronous "is this instance already finished?" check. Store `ProcessStatus` in the same Firestore collection, keyed by `instanceID`, written by the worker via the `Mark*` verbs.
- **Dead-lettering** — Pub/Sub supports a per-subscription DLQ topic when a message exceeds the max-delivery-attempts threshold. Decide the threshold and what telemetry surfaces when a job dead-letters. blkit's `MarkFailed` happens before DLQ; DLQ is the fallback for worker crashes / Nack loops.
- **Backpressure-drop policy** when a subscriber's buffer overflows on `SubscribeToInstance`.
- **Retention** for the terminal `InstanceEventResult` — Pub/Sub message retention is per-Topic (default 7 days, configurable up to 31 days). Set retention long enough that late subscribers can still read the final result, and document the trade-off against storage cost.
- **Pub/Sub Lite** — explicitly **not** supported. Lite is a distinct product with different semantics (per-partition ordering, no filters, lower cost). The gateway targets standard Pub/Sub only.

See [overview.spec.md](overview.spec.md) for the abstract `BrokerGateway` interface this implementation satisfies.
