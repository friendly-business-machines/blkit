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

- Core NATS pub/sub vs JetStream for command queue durability.
- JetStream subject hierarchy — per-instance subjects for fine-grained routing, or a single stream with header-based filtering.
- Pull-consumer vs push-consumer for `Subscribe`.
- Whether `Cancel` / `Terminate` use a separate command subject or share one with `Submit` and `DeliverMessage`.
- Retention policy for terminal `EventResult`.
- Backpressure-drop policy when a subscriber's buffer overflows.

See [overview.spec.md](overview.spec.md) for the abstract `BrokerGateway` interface this implementation satisfies.
