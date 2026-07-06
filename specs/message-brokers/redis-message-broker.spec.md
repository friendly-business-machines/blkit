---
name: RedisMessageBroker
description: Redis/Valkey message-broker backend — Streams + consumer groups for the job queue, per-instance streams for events, TTL'd keys for the registry. Its own module under brokers/redis.
targets:
  - ../../brokers/redis/broker.go
---

# Redis Message Broker

> **Status:** This spec is a work in progress. Implementation pending.

The Redis backend implements [MessageBroker](overview.spec.md) against Redis
or **Valkey** (both fully supported — only commands present in both are
used). Redis covers every broker duty natively — including true removal of
still-queued jobs — with a single lightweight dependency, making it the
default self-hosted choice.

```go
import redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"

broker, err := redisbroker.New(redisbroker.Config{Addr: "localhost:6379"})
```

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one **Stream per `ProcessKey`**
   (`<prefix>:jobs:<ns>:<proc>:<ver>`) with a single consumer group.
   `Submit` and resume timers `XADD`; workers `XREADGROUP`. Terminal
   lifecycle reports and `ReportSuspended` issue `XACK` (+ `XDEL` of the
   consumed entry). A background `XAUTOCLAIM` loop reclaims entries pending
   longer than the in-flight timeout and redelivers them.
2. **Selective consumption** — workers read only the streams for their
   registered keys; `XREADGROUP` accepts multiple streams in one call.
3. **Registry** — one hash per worker (`<prefix>:reg:<workerID>`) holding
   the envelope-encoded registrations, plus a TTL key refreshed by
   `Heartbeat`. Changes are broadcast on a Pub/Sub channel
   (`<prefix>:reg-feed`); heartbeat loss is detected by a sweeper (or
   keyspace notifications where enabled) and broadcast as
   `RegistryUpdateHeartbeatLost`. `SubscribeToProcessRegistry` snapshots via
   `SCAN` over `<prefix>:reg:*`, then follows the feed.
4. **Per-instance events / fan-out / replay** — a **Stream per instance**
   (`<prefix>:inst:<id>`, `MAXLEN ~` capped) rather than plain Pub/Sub, so
   late subscribers replay: on subscribe, `XRANGE` recovers the latest
   lifecycle event and any terminal event, then `XREAD` follows live. Each
   subscriber reads independently (broadcast). The stream key gets an
   `EXPIRE` after the terminal event (default 24h) — the retention window.
5. **Delayed delivery** — a sorted set (`<prefix>:timers`, score =
   fire-time) plus a mover loop that atomically claims due members and
   `XADD`s the `JobResume` to the instance's job stream.
6. **Cancel of queued jobs** — best-effort and atomic: a Lua script scans
   the process key's stream for the instance's `JobStart`, checks it is not
   in the consumer group's pending list (`XPENDING`), and `XDEL`s it only
   then. If the script does not find an undelivered entry, `Cancel` falls
   through to the `JobCancel` route.
7. **TLS** — `Config.TLS *tls.Config`; nil means plaintext (development).
8. **Config + constructor** —

   ```go
   func New(cfg Config) (*Broker, error)

   type Config struct {
       Addr      string            // e.g. "localhost:6379"
       Username  string            // optional (Redis ACLs)
       Password  string            // optional
       TLS       *tls.Config       // nil = plaintext
       KeyPrefix string            // default "blkit"; isolates deployments sharing a server
       Cipher    bl.PayloadCipher  // optional end-to-end payload encryption; default nil
   }
   ```

9. **Local testing** — the conformance suite starts a throwaway Redis (or
   Valkey) container with testcontainers-go, exactly as the SQL state stores
   do. `BLKIT_TEST_REDIS_URL` points it at an already-running instance
   instead; the test skips only when neither is available.

## Notes

- All payloads are [CBOR envelopes](overview.spec.md#wire-format); stream
  entries carry the envelope in one field plus cleartext routing fields
  (`kind`, `instance`, `key`) for scanning.
- Backpressure and fan-out follow the overview defaults.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
