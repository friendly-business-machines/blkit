---
name: InMemoryBrokerGateway
description: Single-process, in-memory implementation of BrokerGateway. Suitable for tests and small single-binary deployments. Implementation pending; this stub records the placeholder.
targets:
  - ../messagebroker/inmemory_gateway.go
---

# InMemoryBrokerGateway

`InMemoryBrokerGateway` is the single-process implementation of [BrokerGateway](overview.spec.md) that uses Go channels and an in-memory event registry. No external broker required.

```go
func NewInMemoryBrokerGateway(opts ...InMemoryOpts) *InMemoryBrokerGateway

type InMemoryOpts struct {
    EventBufferSize int // per-subscriber event buffer; default 64
}
```

This implementation is suitable for:

- Unit tests that exercise the gateway interface without standing up Redis/NATS.
- Small single-binary deployments where the producer code, the worker, and the broker live in one process and don't need to scale beyond a single instance.
- Local development.

It is **not** suitable for production multi-process deployments — there is no inter-process communication. For that, use [RedisBrokerGateway](redis.spec.md) or [NATSBrokerGateway](nats.spec.md).

## Status

Implementation pending. This spec is a placeholder. Open design questions:

- Whether the in-memory broker is shared across all `InMemoryBrokerGateway` instances in a process (singleton) or per-gateway (one broker per call to `NewInMemoryBrokerGateway`). Singleton matches the FAAS/worker pattern of "one in-memory store per binary."
- Backpressure-drop policy on slow subscribers.
- Lifecycle: explicit `Close()` or rely on context-cancellation.

See [overview.spec.md](overview.spec.md) for the abstract `BrokerGateway` interface this implementation satisfies.
