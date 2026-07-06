# Redis

> A self-hosted broker backed by Redis or Valkey — Streams and consumer groups
> for the job queue, per-instance streams for events, TTL'd keys for the registry.
> The lightweight self-host default.

!!! note "Status — implementation pending"
    The message-broker subsystem is still being built. This page describes the
    intended design; see `specs/message-brokers/` for the authoritative spec.

The Redis backend implements [MessageBroker](overview.md) against **Redis** or
**Valkey** — both fully supported, using only commands present in both. Redis
covers every broker duty natively, including true removal of still-queued jobs,
with a single lightweight dependency, making it the default self-hosted choice.

```go
import redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"

var broker, err = redisbroker.New(redisbroker.Config{Addr: "localhost:6379"})
```

## What it's good for

- **Self-hosted deployments** that want the lightest dependency covering every
  duty natively.
- Teams **already running Redis or Valkey** who want to reuse it.

## How it works

- **Job queue** — one **Stream per process key** with a single consumer group.
  Workers read with `XREADGROUP`; terminal reports `XACK` the entry, and a
  background `XAUTOCLAIM` loop redelivers entries pending longer than the
  in-flight timeout.
- **Selective consumption** — workers read only the streams for their registered
  keys; `XREADGROUP` accepts multiple streams at once.
- **Registry** — one hash per worker plus a TTL key refreshed by heartbeats;
  changes broadcast on a Pub/Sub channel, and a sweeper detects heartbeat loss.
- **Per-instance events** — a **Stream per instance** (capped with `MAXLEN`), so
  late subscribers replay the latest lifecycle and terminal events via `XRANGE`
  before following live. The key gets an `EXPIRE` after the terminal event
  (default 24h) — the retention window.
- **Timers** — a sorted set keyed by fire-time, with a mover loop that `XADD`s
  the resume when due.
- **Cancel of queued jobs** — best-effort and atomic via a Lua script that
  removes the job only if it has not yet been delivered.

## Configuration

```go
type Config struct {
    Addr      string            // e.g. "localhost:6379"
    Username  string            // optional (Redis ACLs)
    Password  string            // optional
    TLS       *tls.Config       // nil = plaintext (development)
    KeyPrefix string            // default "blkit"; isolates deployments sharing a server
    Cipher    bl.PayloadCipher  // optional end-to-end payload encryption; default nil
}
```

## Local testing

The conformance suite starts a throwaway Redis (or Valkey) container with
testcontainers-go, exactly as the SQL state stores do. `BLKIT_TEST_REDIS_URL`
points it at an already-running instance instead; the test skips only when
neither is available.

## Reference

The `MessageBroker` interface this backend implements is part of the core API
[Reference](../reference/blkit.md).
