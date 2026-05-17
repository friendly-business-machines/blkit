---
name: InMemoryMessageGateway
description: Single-process, in-memory implementation of MessageGateway. Suitable for tests and small single-binary deployments. Implementation pending; this stub records the placeholder.
targets:
  - ../messagegateway/inmemory_gateway.go
---

# InMemoryMessageGateway

`InMemoryMessageGateway` is the single-process implementation of [MessageGateway](overview.spec.md) that uses Go channels and an in-memory event registry. No external broker required.

```go
func NewInMemoryMessageGateway(opts ...InMemoryOpts) *InMemoryMessageGateway

type InMemoryOpts struct {
    EventBufferSize int // per-subscriber event buffer; default 64
}
```

This implementation is suitable for:

- Unit tests that exercise the gateway interface without standing up Redis/NATS.
- Small single-binary deployments where the producer code, the worker, and the broker live in one process and don't need to scale beyond a single instance.
- Local development.

It is **not** suitable for production multi-process deployments — there is no inter-process communication. For that, use [RedisMessageGateway](redis.spec.md) or [NATSMessageGateway](nats.spec.md).

## Status

Implementation pending. This spec is a placeholder. Open design questions:

- Whether the in-memory broker is shared across all `InMemoryMessageGateway` instances in a process (singleton) or per-gateway (one broker per call to `NewInMemoryMessageGateway`). Singleton matches the worker pattern of "one in-memory store per binary."
- Backpressure-drop policy on slow subscribers.
- Lifecycle: explicit `Close()` or rely on context-cancellation.

See [overview.spec.md](overview.spec.md) for the abstract `MessageGateway` interface this implementation satisfies.
