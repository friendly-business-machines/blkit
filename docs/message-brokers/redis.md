# Redis

> A self-hosted broker backed by Redis or Valkey — Streams and consumer groups
> for the job queue, per-instance streams for events, sweeper-expired hashes for
> the registry. The lightweight self-host default.

The Redis backend implements [MessageBroker](overview.md) against **Redis** or
**Valkey** — both fully supported, using only commands present in both. Redis
covers every broker duty natively, including true removal of still-queued jobs,
with a single lightweight dependency, making it the default self-hosted choice.

```go
import redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"

// New dials the server and verifies connectivity; the returned *Broker is a
// bl.MessageBroker the rest of blkit uses the same way as any other backend.
broker, err := redisbroker.New(redisbroker.Config{Addr: "localhost:6379"})
if err != nil {
    log.Fatalf("connect redis broker: %v", err)
}
defer broker.Close()
```

## What it's good for

- **Self-hosted deployments** that want the lightest dependency covering every
  broker duty natively.
- Teams **already running Redis or Valkey** who want to reuse it.

## Running the server

The broker needs a Redis-compatible server reachable at `Config.Addr`. Unlike
[NATS](nats.md), Redis cannot be embedded in your Go process, so the server is
always something you run alongside the app. In rough order of operational weight:

- **Local companion for development** — run `redis-server` (or `valkey-server`)
  from a package manager, or a throwaway container:
  `docker run -p 6379:6379 redis:7`. The [conformance suite](#local-testing)
  starts one automatically, so you do not need a running server just to test.
- **Sidecar or shared container** — in a compose file or Kubernetes pod, run a
  `redis`/`valkey` container next to your workers and point `Addr` at it. Enable
  AOF or RDB persistence if you want queued jobs to survive a broker restart.
- **Managed service** — the backend speaks plain Redis, so it works unchanged
  against Amazon ElastiCache / MemoryDB, Google Memorystore, Azure Cache for
  Redis, or Valkey-compatible offerings. Point `Addr` at the managed endpoint and
  set `TLS` and credentials.

**Valkey** (the open-source Redis fork) is a drop-in alternative everywhere above —
the backend restricts itself to commands present in both, so either works.

## How it works

Each broker duty maps onto Redis primitives as follows. All keys carry the
configured `KeyPrefix` (default `blkit`).

- **Job queue, ack, and redelivery** — one **Stream per process key**
  (`<prefix>:jobs:<key>`) with a single consumer group. `Submit` and due resume
  timers `XADD` an entry; workers claim work with `XREADGROUP` under a unique
  consumer name. A terminal report `XACK`s **and** `XDEL`s every delivered entry
  for the instance. A background `XAUTOCLAIM` pass (every `InFlightTimeout/3`,
  min-idle `InFlightTimeout`) reclaims entries from workers that died mid-job and
  redelivers them — this is the at-least-once guarantee. Entries that fail to
  decode (poison messages) are acked and dropped rather than redelivered forever.
- **Selective consumption** — a worker reads only the streams for the process keys
  it registered; `XREADGROUP` takes several streams in one call.
- **Registry** — one **hash per worker** (`<prefix>:reg:<workerID>`) holding the
  encoded registrations plus a heartbeat-refreshed `deadline`. There is
  deliberately **no TTL'd key** — an expired key would destroy the data needed to
  announce the loss — so a **sweeper** (tick `RegistrationTTL/4`) claims lapsed
  hashes and broadcasts `HeartbeatLost` on a Pub/Sub feed channel
  (`<prefix>:reg-feed`); register/unregister broadcast on the same channel. A new
  subscriber subscribes to the feed first, then snapshots the current workers with
  `SCAN`, so it misses no change.
- **Per-instance events, fan-out, and replay** — a **Stream per instance**
  (`<prefix>:inst:<id>`, capped with `MAXLEN`). Each subscriber reads the stream
  independently (true fan-out); on subscribe it `XRANGE`s back the latest lifecycle
  event and any terminal event before following live, so a **late subscriber still
  catches up**. When the instance finishes, the key is `PEXPIRE`d to
  `EventRetention` — that window is when late replay stops being possible.
- **Suspend-resume timers** — a **sorted set** (`<prefix>:timers`) scored by
  fire-time, with a 100 ms mover loop that atomically claims due members and
  `XADD`s the `JobResume` back onto the instance's job stream.
- **Cancel of a queued job** — best-effort and atomic: a Lua script finds the
  instance's `JobStart` in its stream, checks it is not yet in the consumer group's
  pending list, and `XDEL`s it only if it has not been delivered. If it has already
  been picked up, `Cancel` falls through to the normal cancel-message route.

## Configuration

```go
type Config struct {
    Addr     string      // e.g. "localhost:6379"; required
    Username string      // optional (Redis ACLs)
    Password string      // optional
    TLS      *tls.Config // nil = plaintext (development)

    KeyPrefix string           // default "blkit"; isolates deployments sharing a server
    Cipher    bl.PayloadCipher // optional end-to-end payload encryption; default nil

    RegistrationTTL time.Duration // default 90s (3× the 30s heartbeat interval)
    InFlightTimeout time.Duration // default 150s; XAUTOCLAIM min-idle before redelivery
    EventRetention  time.Duration // default 1h; replay window after an instance finishes
}
```

- **`Addr`, `Username`, `Password`, `TLS`** — how to reach the server. Leave
  `TLS` nil for a local plaintext server; set it (and credentials) for a managed
  or networked one.
- **`KeyPrefix`** — namespaces every key this broker creates. Change it to run
  several independent blkit deployments against one Redis server without collision.
- **`Cipher`** — an optional [`PayloadCipher`](../reference/blkit.md) that
  encrypts message payloads end to end, so the Redis server never sees plaintext.
  All brokers and workers in a deployment must share the same cipher.
- **The three timing knobs** default to values derived from the 30 s worker
  heartbeat and rarely need changing:
  - **`RegistrationTTL`** (90 s) — how long a worker's registration outlives its
    last heartbeat before the sweeper declares it lost. Raise it if workers run on
    a network where heartbeats are occasionally delayed; lower it for faster
    failure detection.
  - **`InFlightTimeout`** (150 s) — how long a delivered job may go unsettled
    before it is reclaimed and redelivered. It must comfortably exceed your
    longest task step, or a slow-but-alive worker's job will be redelivered under
    it.
  - **`EventRetention`** (1 h) — how long after an instance finishes its event
    stream stays replayable to late subscribers. Raise it if consumers may
    reconnect long after a run completes.

## Local testing

The [conformance suite](overview.md#conformance) starts a throwaway Redis (or
Valkey) container with testcontainers-go, exactly as the SQL state stores do. Set
`BLKIT_TEST_REDIS_URL` to point it at an already-running instance instead; the test
skips only when neither a container runtime nor that variable is available.

## What to keep in mind

- **Persistence is Redis's job, not the broker's.** A default Redis holds the
  queue in memory; enable AOF/RDB persistence (or a managed tier that does) if
  queued jobs must survive a server restart.
- **Cancel is best-effort by design** — a job already delivered to a worker cannot
  be pulled back; it is cancelled through the normal route instead.
- **One `KeyPrefix` per deployment** when sharing a server, or two deployments will
  read each other's streams.

## Reference

The backend's API is in the [Redis reference](../reference/brokers-redis.md);
the `MessageBroker` interface it implements is in the core [Reference](../reference/blkit.md).
