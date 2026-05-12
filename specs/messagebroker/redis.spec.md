---
name: RedisBrokerGateway
description: Redis/Valkey-backed implementation of BrokerGateway. Implementation pending; this stub records the placeholder.
targets:
  - ../messagebroker/redis_gateway.go
---

# RedisBrokerGateway

`RedisBrokerGateway` is the Redis/Valkey-backed implementation of [BrokerGateway](overview.spec.md). Constructor:

```go
func NewRedisBrokerGateway(opts RedisOpts) (*RedisBrokerGateway, error)

type RedisOpts struct {
    Addr string // e.g. "localhost:6379"
    // ... pending: auth, TLS, key-prefix, stream config
}
```

## Status

Implementation pending. This spec is a placeholder. Open design questions:

- Streams vs Pub/Sub for event distribution.
- Streams (XADD/XREAD) for the command queue, or a dedicated list-based queue.
- Key layout — namespacing under a configurable prefix.
- Consumer-group strategy for fan-out vs broadcast subscriptions.
- Retention policy for terminal `EventResult` — how long is replay possible after an instance completes.
- Backpressure-drop policy when a subscriber's buffer overflows.

See [overview.spec.md](overview.spec.md) for the abstract `BrokerGateway` interface this implementation satisfies.
