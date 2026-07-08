# NATS

> A self-hosted broker backed by NATS with JetStream — subject-filtered pull
> consumers for the job queue, per-subject streams for events, a KV bucket with
> watch for the registry. A natural fit when NATS is already your state store,
> and light enough to embed in your own process.

The NATS backend implements [MessageBroker](overview.md) against NATS with
**JetStream** (plain core NATS lacks the durability the job queue needs).
JetStream's subject hierarchy gives the cleanest selective consumption of any
backend — a worker's filter is evaluated server-side, so it never receives a job
it did not ask for — and NATS KV maps directly onto the worker registry. It is a
natural fit when NATS is already your [state store](../state-stores/nats.md):
one infrastructure piece serves both duties.

```go
import natsbroker "github.com/friendly-business-machines/blkit/brokers/nats"

// New dials the server, provisions the jobs and events streams and the two KV
// buckets, and starts the registry sweeper; the returned *Broker is a
// bl.MessageBroker the rest of blkit uses like any other backend.
broker, err := natsbroker.New(natsbroker.Config{URL: "nats://localhost:4222"})
if err != nil {
    log.Fatalf("connect nats broker: %v", err)
}
defer broker.Close()
```

## What it's good for

- **Setups that already run NATS** — as the state store, the broker, or both —
  so one dependency covers every duty.
- Deployments that want **native, server-side selective consumption** with no
  client-side filtering.
- **Single-binary or edge deployments** — NATS can run embedded inside your Go
  process, so there is no separate service to operate at all.

## Running the server

The broker needs a JetStream-enabled NATS server reachable at `Config.URL`.
JetStream must be turned on: a bare `nats-server` with no JetStream will fail to
provision the streams. In rough order of operational weight:

- **Embedded in-process** — `github.com/nats-io/nats-server` is an ordinary Go
  library, so you can start a JetStream server inside your own binary and point
  the broker at it over the in-process connection. This is the lightest option:
  no separate service, no container, nothing to deploy alongside the app. The
  [conformance suite](#local-testing) runs exactly this way. It suits
  single-binary tools, edge nodes, and tests.
- **Local companion for development** — run `nats-server -js` from a package
  manager or a throwaway container (`docker run -p 4222:4222 nats:latest -js`)
  and point `URL` at `nats://localhost:4222`. The `-js` flag enables JetStream.
- **Sidecar or shared container** — in a compose file or Kubernetes pod, run a
  `nats` container (or a small cluster for HA) next to your workers with
  JetStream enabled and file-based storage, and point `URL` at it. Enable
  clustering and replicas if queued jobs must survive a node loss.
- **Managed service** — the backend speaks plain NATS, so it works unchanged
  against **Synadia Cloud** and the **NGS** global NATS service. Point `URL` at
  the managed endpoint and supply a credentials file through `Credentials` (and
  `TLS` where required).

## How it works

Each broker duty maps onto JetStream primitives as follows. All streams,
consumers, and buckets carry the configured `SubjectPrefix` (default `blkit`);
process-key parts are base64url-encoded into single subject tokens because
namespaces legitimately contain `/` and `.`.

- **Job queue, ack, and redelivery** — one **jobs stream**
  (`<prefix>-jobs`, capturing `<prefix>.jobs.>`) with **Limits retention**:
  jobs are removed by explicit `DeleteMsg` when they settle, not by the queue
  policy. (WorkQueue retention is avoided because it forbids the overlapping
  consumer filters selective consumption needs; Interest retention would drop
  jobs published before any worker subscribed.) Workers pull through a
  **durable consumer** whose name is derived from the sorted set of subject
  filters, so every worker fetching the same keys competes on one consumer.
  `AckWait` is set to `InFlightTimeout`. A terminal report `Ack`s **and**
  `DeleteMsg`s every delivery held for the instance; a worker that dies stops
  extending its `AckWait`, and JetStream redelivers automatically — that is the
  at-least-once guarantee. Messages that fail to decode (a poison payload or a
  foreign cipher key) are `Term`ed so they are never redelivered.
- **Selective consumption** — the pull consumer carries **subject filters** for
  exactly the worker's registered keys (`<prefix>.jobs.<ns>.<proc>.<ver>.*`).
  Filtering happens on the server; there is no client-side discard.
- **Registry** — a NATS **KV bucket** (`<prefix>-registry`), one
  envelope-encoded entry per worker holding the stamped registrations, a
  TTL deadline, and a generation counter that is bumped on registration but not
  on a heartbeat (so heartbeat refreshes do not wake watchers). Expiry is
  **sweeper-driven, not KV TTL** — age-based key expiry produces no reliable
  watch event, so a background sweeper instead tombstones a lapsed entry (with
  its old registrations and a reason attached) and then deletes it. That lets a
  `KV.Watch()` translate the change into the right update: `Removed` for an
  `Unregister`, `HeartbeatLost` for a sweeper expiry. A new subscriber's watch
  delivers the current entries as a snapshot, then a `SnapshotComplete`
  sentinel, then live updates. `Submit` reads the bucket directly — it is
  immediately consistent — so there is no cold-start snapshot wait.
- **Per-instance events, fan-out, and replay** — an **events stream**
  (`<prefix>-events`, capturing `<prefix>.inst.>`, with `MaxAge` set to
  `EventRetention`). Each instance publishes to per-kind subjects
  (`<prefix>.inst.<id>.lifecycle` / `.terminal` / `.inputreq` / `.node` /
  `.err`). A subscriber first replays the latest lifecycle event (and the
  terminal Result/Error, if the instance finished within the retention window)
  with `GetLastMsgForSubject`, then follows live from the next stream sequence
  through an **ordered consumer** — no gap, no duplicate — so a **late
  subscriber still catches up**. Every subscriber gets its own consumer, giving
  true fan-out. A small `<prefix>-instmeta` KV bucket records each instance's
  routing key, correlation key, and finish time, for routing
  `RespondToInputRequest`, stamping the correlation key onto events, and driving
  retention cleanup.
- **Suspend-resume timers** — NATS has no native delayed publish, so
  `ReportSuspended` with a resume time schedules the `JobResume` with an
  **in-process timer**. When it fires, the broker re-checks the instance is
  still live and republishes the resume job. Because the timer lives in the
  broker process, a **restart before the fire-time loses the pending resume**;
  durable timers are a planned improvement.
- **Cancel of a queued job** — best-effort. The instance id is the last token
  of the job subject, so `Cancel` looks up the `JobStart` with
  `GetLastMsgForSubject`, confirms it is a start job, verifies it is still
  undelivered (checking the local in-flight map and every matching consumer's
  delivered floor), and `DeleteMsg`s it by sequence — then publishes the
  terminal Cancelled event itself. If the job was already delivered to a worker
  it cannot be pulled back, and `Cancel` falls through to the normal
  `JobCancel` route, which requires the process to have opted in with
  `AllowExternalCancel`.

## Configuration

```go
type Config struct {
    URL         string      // NATS server URL(s), e.g. "nats://localhost:4222"; required
    Credentials string      // optional path to a .creds file
    TLS         *tls.Config // nil = plaintext (development)

    SubjectPrefix string           // default "blkit"; isolates deployments sharing a server
    Cipher        bl.PayloadCipher // optional end-to-end payload encryption; default nil

    RegistrationTTL time.Duration // default 90s (3× the 30s heartbeat interval)
    InFlightTimeout time.Duration // default 150s; maps to the consumer AckWait
    EventRetention  time.Duration // default 1h; maps to the events-stream MaxAge
}
```

- **`URL`, `Credentials`, `TLS`** — how to reach the server. Leave `TLS` nil for
  a local plaintext server; set it, and point `Credentials` at a `.creds` file
  (NKeys and credentials files are both supported), for a managed or networked
  one. `URL` may list several servers for failover.
- **`SubjectPrefix`** — namespaces every subject, stream, and bucket this broker
  creates. Change it to run several independent blkit deployments against one
  NATS server without collision. It must contain only alphanumerics, `-`, and
  `_`.
- **`Cipher`** — an optional [`PayloadCipher`](../reference/blkit.md) that
  encrypts message payloads end to end, so the NATS server only ever stores
  ciphertext (routing data stays in the cleartext subject and headers). Every
  broker and worker in a deployment must share the same cipher.
- **The three timing knobs** default to values derived from the 30 s worker
  heartbeat and rarely need changing:
  - **`RegistrationTTL`** (90 s) — how long a worker's registration outlives its
    last heartbeat before the sweeper tombstones it as heartbeat loss. Raise it
    on networks where heartbeats are occasionally delayed; lower it for faster
    failure detection.
  - **`InFlightTimeout`** (150 s) — the consumer `AckWait`: how long a delivered
    job may go unsettled before JetStream redelivers it. It must comfortably
    exceed your longest task step, or a slow-but-alive worker's job will be
    redelivered under it.
  - **`EventRetention`** (1 h) — the events-stream `MaxAge`: how long after an
    instance finishes its events stay replayable to late subscribers. Raise it
    if consumers may reconnect long after a run completes.

## Local testing

The [conformance suite](overview.md#conformance) runs against a **real
JetStream server embedded in the test process** — `nats-server` is importable as
a Go library, so no container is needed, exactly as the NATS state store does.
Set `BLKIT_TEST_NATS_URL` to point the suite at an already-running external
server instead; the test uses the embedded server whenever that variable is
unset.

## What to keep in mind

- **JetStream must be enabled and durable.** The backend provisions streams and
  KV buckets on connect; a server without JetStream fails to construct. For
  queued jobs to survive a restart, back JetStream with file storage (and
  replicas in a cluster), not memory storage.
- **Resume timers are in-process.** A suspended instance's `JobResume` is held
  by a timer in the broker; a broker restart before the fire-time drops it.
  Keep this in mind for long suspensions until durable timers land.
- **Cancel is best-effort by design** — a job already delivered to a worker
  cannot be pulled out of the stream and is cancelled through the message route
  instead, which the process must have allowed.
- **One `SubjectPrefix` per deployment** when sharing a server, or two
  deployments will read each other's streams and registry.

## Reference

The backend's API is in the [NATS reference](../reference/brokers-nats.md);
the `MessageBroker` interface it implements is in the core [Reference](../reference/blkit.md).
