---
name: RedisMessageGateway
description: Redis/Valkey-backed implementation of MessageGateway. Implementation pending; this stub records the placeholder.
targets:
  - ../messagegateway/redis_gateway.go
---

# RedisMessageGateway

`RedisMessageGateway` is the Redis/Valkey-backed implementation of [MessageGateway](overview.spec.md). Constructor:

```go
func NewRedisMessageGateway(opts RedisOpts) (*RedisMessageGateway, error)

type RedisOpts struct {
    Addr string // e.g. "localhost:6379"
    // ... pending: auth, TLS, key-prefix, stream config
}
```

## Status

Implementation pending. This spec is a placeholder. Open design questions:

- Streams vs Pub/Sub for instance-event distribution (`SubscribeToInstance`).
- Streams (XADD/XREAD with consumer groups) for the job queue (`FetchJobs`). Worker outcome verbs (`MarkCompleted` / `MarkCancelled` / `MarkFailed` / `ReenqueueSuspended`) ack via XACK; `XAUTOCLAIM` redelivers in-flight jobs from crashed workers.
- KV (Redis hashes) plus a Pub/Sub channel for the registry change feed (`SubscribeToProcessRegistry`): snapshot via `HSCAN`, updates broadcast on a dedicated channel from `RegisterProcesses` / `Unregister` / TTL-expiry sweeper.
- Key layout — namespacing under a configurable prefix.
- Consumer-group strategy for fan-out vs broadcast on `SubscribeToInstance`.
- Retention policy for the terminal `InstanceEventResult` — how long is replay possible after an instance finishes.
- Backpressure-drop policy when a subscriber's buffer overflows.
- Status record: where to store the per-instance `ProcessStatus` so `Cancel` / `Terminate` can detect already-finished instances (likely a Redis hash keyed by instanceID, written by the worker via the Mark* verbs).

See [overview.spec.md](overview.spec.md) for the abstract `MessageGateway` interface this implementation satisfies.
