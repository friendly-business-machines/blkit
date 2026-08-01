---
name: RedisMessageBroker
description: Redis/Valkey message-broker backend — Streams + consumer groups for the job queue, per-instance streams for events, sweeper-expired worker hashes + a Pub/Sub feed for the registry. Its own module under brokers/redis.
status: implemented
code:
  - brokers/redis/
implements: specs/message-brokers/overview.spec.md
---

# Redis Message Broker

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
   (`<prefix>:jobs:<key>`, where `<key>` is one base64url-encoded token of
   the three key parts — namespaces contain `/` and `.`) with a single
   consumer group created at id 0 (`MKSTREAM`). `Submit` and resume timers
   `XADD`; workers `XREADGROUP` with a unique consumer name per handle.
   Terminal lifecycle reports and `ReportSuspended` issue `XACK` + `XDEL`
   of every delivered entry tracked for the instance. An `XAUTOCLAIM` pass
   every `InFlightTimeout/3` (min-idle = `InFlightTimeout`) reclaims
   entries from dead consumers and redelivers. Undecodable entries (poison
   messages) are acked and deleted rather than redelivered forever.
2. **Selective consumption** — workers read only the streams for their
   registered keys; `XREADGROUP` accepts multiple streams in one call.
3. **Registry** — one hash per worker (`<prefix>:reg:<workerID>`) holding
   the envelope-encoded registrations plus cleartext `deadline`, `hb`, and
   per-key first-registration fields. There is **no TTL'd key** — a
   Redis-expired key would destroy the registration data needed to
   broadcast the loss — so a sweeper (tick = TTL/4) claims lapsed hashes
   atomically via Lua and broadcasts `RegistryUpdateHeartbeatLost` on the
   Pub/Sub feed channel (`<prefix>:reg-feed`); Register/Unregister
   broadcast Added/Removed the same way. `SubscribeToProcessRegistry`
   subscribes to the feed first, snapshots via `SCAN`, then relays.
   `Submit` resolves via a direct `SCAN` read (immediately consistent), so
   there is no cold-start snapshot wait.
4. **Per-instance events / fan-out / replay** — a **Stream per instance**
   (`<prefix>:inst:<id>`, `MAXLEN ~256`) rather than plain Pub/Sub; entries
   carry the envelope plus cleartext `kind`/`phase` fields. On subscribe,
   `XRANGE` recovers the latest lifecycle event and any terminal event
   (selected on the cleartext fields), then `XREAD` follows live. Each
   subscriber reads independently (broadcast). Instance→key and correlation
   routing lives in `<prefix>:instmeta:<id>`; finishing an instance
   `PEXPIRE`s both keys to `EventRetention` (default 1h) — the retention
   window starts at finish.
5. **Delayed delivery** — a sorted set (`<prefix>:timers`, score =
   fire-time ms) plus a 100ms mover loop that atomically claims due members
   via Lua and `XADD`s the `JobResume` to the instance's job stream,
   skipping finished/expired instances.
6. **Cancel of queued jobs** — best-effort and atomic: a Lua script scans
   the process key's stream for the instance's `JobStart` (by the cleartext
   fields), checks it is not in the consumer group's pending list
   (`XPENDING`), and `XDEL`s it only then; the broker then publishes the
   terminal Cancelled event itself and finishes the instance. If the script
   does not find an undelivered entry, `Cancel` falls through to the
   `JobCancel` route.
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

       RegistrationTTL time.Duration // default 90s (3× heartbeat interval)
       InFlightTimeout time.Duration // default 150s; XAUTOCLAIM min-idle
       EventRetention  time.Duration // default 1h; PEXPIRE window after finish
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
