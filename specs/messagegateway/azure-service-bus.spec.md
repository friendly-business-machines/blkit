---
name: AzureServiceBusMessageGateway
description: Azure Service Bus-backed implementation of MessageGateway. Implementation pending; this stub records the placeholder.
targets:
  - ../messagegateway/azuresb_gateway.go
---

# AzureServiceBusMessageGateway

`AzureServiceBusMessageGateway` is the Azure Service Bus-backed implementation of [MessageGateway](overview.spec.md). Service Bus provides durable queues, topics with subscriptions, peek-lock delivery, and per-session ordering — the primitives needed for both the job queue and the per-instance event topic.

Constructor:

```go
func NewAzureServiceBusMessageGateway(opts AzureServiceBusOpts) (*AzureServiceBusMessageGateway, error)

type AzureServiceBusOpts struct {
    // Fully-qualified namespace, e.g. "myns.servicebus.windows.net".
    Namespace string

    // Authentication: one of ConnectionString or Credential must be set.
    ConnectionString string             // shared-access-key style
    Credential       azcore.TokenCredential // Azure AD / Managed Identity

    // Optional prefix prepended to all topic/queue/subscription names so
    // multiple blkit deployments can share a single Service Bus namespace.
    EntityPrefix string

    // Where to keep the registry and per-instance status records. Service Bus
    // is not a KV store, so blkit needs a side channel. See the Status section.
    RegistryStore RegistryStore // pluggable; typically AzureTableRegistryStore or CosmosRegistryStore
}
```

## Status

Implementation pending. This spec is a placeholder. Open design questions:

- **Job queue (`FetchJobs`)** — one Service Bus queue per `(Namespace, ProcessID, Version)` so workers can subscribe selectively by topic name, or a single shared queue with header-based routing and per-worker filters? The per-key-queue model maps cleanly onto Service Bus's native scaling unit but inflates entity count for deployments with many processes.
- **Peek-lock + outcome verbs** — Service Bus uses peek-lock-with-lock-renewal for in-flight delivery. Worker outcome verbs map naturally: `MarkCompleted` / `MarkCancelled` / `MarkFailed` issue `Complete()`; `ReenqueueSuspended` issues `Defer()` plus a sequence-number record so the matching `JobResume` (driven by `RespondToInputRequest` or a timer) can `ReceiveDeferredMessage()` later. Crashed workers lose the lock and Service Bus redelivers automatically.
- **Sessions for per-instance ordering** — Service Bus sessions guarantee FIFO within a session id. Using `instanceID` as the session id keeps all jobs for one instance ordered and pinned to one consumer at a time, which simplifies reasoning about `JobCancel` / `JobResume` race conditions but caps per-instance throughput. Document whether sessions are mandatory or opt-in.
- **Per-instance event topic (`SubscribeToInstance`)** — one Service Bus Topic for instance events, with a per-subscriber Subscription created on demand. SQL filters on `instanceID` header route only the subscriber's instance to them. Subscription cleanup on subscriber disconnect is the awkward part; investigate auto-delete-on-idle (Service Bus supports this natively via `AutoDeleteOnIdle`).
- **Registry change feed (`SubscribeToProcessRegistry`)** — Service Bus has no KV. Two options: (a) a separate `RegistryStore` interface backed by Azure Table Storage / Cosmos DB for the snapshot, plus a Service Bus Topic for change events emitted by `RegisterProcesses` / `Unregister` / a TTL sweeper; or (b) a single Service Bus Topic with a "compacted" stream the gateway replays on subscribe. (a) is more standard; (b) avoids the extra Azure resource.
- **Per-instance status record** — `Cancel` / `Terminate` need a synchronous "is this instance already finished?" check. Store `ProcessStatus` in the same `RegistryStore` (Table / Cosmos), keyed by `instanceID`, written by the worker via the `Mark*` verbs.
- **Dead-lettering** — Service Bus's built-in DLQ handles jobs whose lock expires too many times. Decide the max-delivery-count and what telemetry surfaces when a job dead-letters. blkit's `MarkFailed` happens before DLQ; DLQ is the fallback for worker crashes.
- **Backpressure-drop policy** when a subscriber's buffer overflows on `SubscribeToInstance`.
- **Retention** for the terminal `InstanceEventResult` — Service Bus messages have a TTL (`DefaultMessageTimeToLive`); set it long enough that late subscribers can still read the final result.
- **Authentication wiring** — Managed Identity (the recommended path in production Azure deployments) vs SAS keys. Both should work; document which path the constructor takes.

See [overview.spec.md](overview.spec.md) for the abstract `MessageGateway` interface this implementation satisfies.
