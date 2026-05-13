---
name: NATSBrokerGateway
description: NATS + JetStream-backed implementation of BrokerGateway. Implementation pending; this stub records the placeholder.
targets:
  - ../messagebroker/nats_gateway.go
---

# NATSBrokerGateway

`NATSBrokerGateway` is the NATS + JetStream-backed implementation of [BrokerGateway](overview.spec.md). Constructor:

```go
func NewNATSBrokerGateway(opts NATSOpts) (*NATSBrokerGateway, error)

type NATSOpts struct {
    URL string // e.g. "nats://localhost:4222"
    // ... pending: credentials, TLS, subject prefix, JetStream stream config
}
```

## Status

Implementation pending. This spec is a placeholder. Open design questions:

- Core NATS pub/sub vs JetStream for job queue durability (`FetchJobs` needs durable delivery + redelivery; lean toward JetStream pull consumers).
- JetStream subject hierarchy — per-process-key subjects for fine-grained routing, or a single stream with header-based filtering.
- How worker outcome verbs (`MarkCompleted` / `MarkCancelled` / `MarkFailed` / `ReenqueueSuspended`) map to JetStream acks: `MarkCompleted` / `MarkCancelled` / `MarkFailed` issue `AckAck`; `ReenqueueSuspended` issues `AckTerm` (the job leaves the consumer) and the wait-condition-satisfied signal republishes a `JobResume` on the same subject.
- Pull-consumer vs push-consumer for `SubscribeToInstance`.
- Whether `Cancel` / `Terminate` use a separate command subject or share one with `Submit` and `RespondToInputRequest`.
- KV bucket + watcher for the registry change feed (`SubscribeToProcessRegistry`). NATS KV's `Watch()` already gives snapshot + updates semantics — map directly.
- Retention policy for the terminal `InstanceEventResult`.
- Backpressure-drop policy when a subscriber's buffer overflows.
- Status record: a small KV bucket keyed by instanceID for `ProcessStatus` so `Cancel` / `Terminate` can detect already-finished instances.

See [overview.spec.md](overview.spec.md) for the abstract `BrokerGateway` interface this implementation satisfies.
