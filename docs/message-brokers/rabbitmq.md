# RabbitMQ

> A self-hosted broker backed by RabbitMQ 3.13+ — quorum queues behind a topic
> exchange for jobs, a shared RabbitMQ Stream for instance events, a
> heartbeat broadcast for the registry. Its redelivery on a crash is immediate.

The RabbitMQ backend implements [MessageBroker](overview.md) against RabbitMQ
3.13+. RabbitMQ's work-queue semantics — manual acks, automatic requeue of
unacked messages, per-consumer prefetch — fit the job queue naturally. The
registry and delayed delivery use documented AMQP patterns because RabbitMQ has
no key-value store or native message scheduling. It is a good choice for the
large base of teams already standardised on RabbitMQ.

```go
import rabbitbroker "github.com/friendly-business-machines/blkit/brokers/rabbitmq"

// New dials the server, declares the exchanges, the delay queue, the shared
// event stream, and this handle's private registry queue, and starts the
// registry consumer and sweeper; the returned *Broker is a bl.MessageBroker.
broker, err := rabbitbroker.New(rabbitbroker.Config{URL: "amqp://guest:guest@localhost:5672/"})
if err != nil {
    log.Fatalf("connect rabbitmq broker: %v", err)
}
defer broker.Close()
```

## What it's good for

- **Enterprises already standardised on RabbitMQ**, reusing an existing broker
  and its operational know-how.
- Deployments that value its **immediate redelivery** — a worker crash requeues
  every unacked message the moment its channel closes, with no timeout to wait
  out.

## Running the server

The broker needs a RabbitMQ 3.13+ server reachable at `Config.URL`, with the
**streams feature enabled** (the instance-event log is a stream queue). RabbitMQ
cannot be embedded in your Go process, so the server is always something you run
alongside the app. In rough order of operational weight:

- **Local companion for development** — a throwaway container is easiest:
  `docker run -p 5672:5672 -p 15672:15672 rabbitmq:3.13-management`. The
  management image ships with the streams plug-in active, which the event log
  requires. The [conformance suite](#local-testing) starts one automatically,
  so you do not need a running server just to test.
- **Sidecar or shared container** — in a compose file or Kubernetes pod (the
  RabbitMQ Cluster Operator is the usual route on Kubernetes), run a `rabbitmq`
  container — or a cluster for HA — next to your workers and point `URL` at it.
  Quorum queues and streams both want a durable, ideally clustered broker so
  queued jobs survive a node loss.
- **Managed service** — the backend speaks plain AMQP 0-9-1, so it works
  unchanged against **Amazon MQ for RabbitMQ** and **CloudAMQP**. Point `URL` at
  the managed endpoint, supply credentials in the URI, and set `TLS`.

## How it works

Each broker duty maps onto RabbitMQ primitives as follows. Every exchange and
queue carries the configured `ExchangePrefix` (default `blkit`); a process key
becomes a routing key by hex-encoding each of its three segments (they contain
`/` and `.`, which AMQP topic routing treats specially), with a truncated
SHA-256 fallback for segments that would otherwise exceed AMQP's name limits.

- **Job queue, ack, and redelivery** — one **quorum queue per process key**
  (`<prefix>.jobs.<enc>`), fed by a topic exchange (`<prefix>.jobs`). Queues are
  declared idempotently both when a worker fetches and before every job publish,
  so a job can never be lost for want of a binding. Workers consume with
  **manual ack and a prefetch of 16**. Terminal reports and `ReportSuspended`
  `basic.ack` every delivery held for the instance. `FetchJobs` owns a
  **dedicated channel** that is closed on ctx-cancel *without* acking, so a
  worker crash — or a clean fetch shutdown — makes RabbitMQ **requeue every
  unacked message immediately**. Redelivery is therefore channel-close-driven,
  not timer-driven: there is no lock to wait out. Undecodable payloads (a
  foreign cipher key or garbage) are nacked without requeue and surfaced as an
  Error event with code `DECRYPT_FAILED`.
- **Selective consumption** — a worker consumes only the queues for the process
  keys it registered.
- **Registry — heartbeat broadcast** (RabbitMQ has no KV). `RegisterProcesses`,
  `Heartbeat`, and `Unregister` publish the worker's full envelope-encoded
  registration set (or an explicit removal) on a fanout exchange
  (`<prefix>.registry`). Every broker handle binds its own private exclusive
  auto-delete queue at construction and keeps a **local registry cache** fed
  from that queue; each broadcast carries an origin-handle header so a handle
  ignores its own echoes (its verbs already applied to the local cache). To keep
  registrations alive between explicit `Heartbeat` calls, a handle
  **re-broadcasts the sets it originated every `RegistrationTTL/4`** while the
  worker's verb-based deadline holds. Subscribers mark a worker
  `HeartbeatLost` **client-side** when no broadcast arrives within
  `RegistrationTTL` (at worst about 2× the TTL after the last verb). `Submit`
  blocks only during a cold start — an entirely empty cache; once any
  registration is visible, an unknown key fails fast with `ErrUnknownProcess`.
- **Per-instance events, fan-out, and replay** — **one shared stream queue**
  (`<prefix>.inst-events`, `x-queue-type: stream`, `x-max-age` set to
  `EventRetention`) carries the events of *all* instances, with the instance id
  and process key in cleartext headers and filtered client-side. On subscribe,
  the backend publishes a unique **marker** message, consumes the stream from
  `x-stream-offset: first`, collapses everything before its marker into the
  latest lifecycle event plus the terminal Result/Error (or
  `INSTANCE_NOT_FOUND` when nothing is visible), then follows live. Each
  subscriber reads at its own offset, giving true fan-out. A plain topic
  exchange with per-subscriber auto-delete queues would be simpler but retains
  nothing — a late subscriber would see no history — so a stream is required.
- **Suspend-resume timers** — the **per-message-TTL + dead-letter-exchange**
  pattern, with no plugin dependency. `ReportSuspended` with a resume time
  publishes the `JobResume` with an expiration to a holding queue
  (`<prefix>.delay.q`) whose dead-letter exchange is the jobs exchange; when the
  TTL fires the message dead-letters into the instance's job queue under its
  original routing key. Note the classic head-of-line caveat: a per-message TTL
  only dead-letters from the *head* of the holding queue, so a long delay ahead
  of a short one postpones the short one. The delayed-message-exchange plugin is
  a documented alternative for deployments that already run it.
- **Cancel of a queued job** — **unsupported natively**: AMQP has no selective
  removal from a queue, so the overview's best-effort queue removal is skipped
  and `Cancel` always takes the `JobCancel` message route. The opt-in check
  therefore applies to every cancel — even of a still-queued job the process
  must have set `AllowExternalCancel`.

## Configuration

```go
type Config struct {
    URL            string           // AMQP URI, e.g. "amqp://user:pass@localhost:5672/"; required
    TLS            *tls.Config      // nil = plaintext (development); amqps when set
    ExchangePrefix string           // default "blkit"; isolates deployments sharing a server
    Cipher         bl.PayloadCipher // optional end-to-end payload encryption; default nil

    RegistrationTTL time.Duration // default 90s; heartbeat-broadcast loss window
    InFlightTimeout time.Duration // default 150s; accepted but NOT used as a timer
    EventRetention  time.Duration // default 1h; the event stream's x-max-age
}
```

- **`URL`, `TLS`** — how to reach the server. Leave `TLS` nil for a local
  plaintext server; set it for a managed or networked one, and the connection is
  upgraded to `amqps` automatically (an `amqp://` URL is rewritten). Put
  credentials and vhost in the URI.
- **`ExchangePrefix`** — namespaces every exchange and queue this broker
  creates. Change it to run several independent blkit deployments against one
  RabbitMQ server without collision.
- **`Cipher`** — an optional [`PayloadCipher`](../reference/blkit.md) that
  encrypts message payloads end to end, so the server only ever stores
  ciphertext (routing keys and headers stay cleartext). Every broker and worker
  in a deployment must share the same cipher; a message it cannot decrypt is
  surfaced as a `DECRYPT_FAILED` error event rather than processed.
- **The timing knobs** default to values derived from the 30 s worker heartbeat:
  - **`RegistrationTTL`** (90 s) — the heartbeat-broadcast loss window: how long
    a worker's registration survives in each handle's cache without a fresh
    broadcast before it is marked `HeartbeatLost`. Raise it on networks where
    broadcasts are occasionally delayed; lower it for faster failure detection.
  - **`InFlightTimeout`** (150 s) — **accepted for interface symmetry with the
    other backends but not used as a timer**. RabbitMQ requeues a worker's
    unacked deliveries the instant its channel or connection closes, so
    redelivery is immediate and there is no in-flight lock duration to tune.
  - **`EventRetention`** (1 h) — the event stream's `x-max-age`: how long after
    an instance finishes its events stay replayable to late subscribers. Raise
    it if consumers may reconnect long after a run completes.

## Local testing

The [conformance suite](overview.md#conformance) starts a throwaway RabbitMQ
container with testcontainers-go, using the **management image** (it includes
the streams feature the event log needs). Set `BLKIT_TEST_RABBITMQ_URL` to point
the suite at an already-running instance instead; the test skips only when
neither a container runtime nor that variable is available.

## What to keep in mind

- **Streams must be available.** The instance-event log is a stream queue, so
  the server needs the streams feature (present in the management image and in
  RabbitMQ 3.13+ by default). Managed offerings generally enable it, but confirm
  before pointing production at one.
- **Redelivery is immediate, so `InFlightTimeout` does nothing here.** A crash
  requeues in-flight jobs at once — an operational strength — but it also means
  a job briefly redelivered during a rolling worker restart is normal; tasks
  must tolerate at-least-once delivery, as they must on every backend.
- **Delayed resumes are head-of-line.** Because the delay uses per-message TTL
  on a shared holding queue, a long resume delay can hold up shorter ones queued
  behind it. Install the delayed-message-exchange plugin if that matters.
- **One `ExchangePrefix` per deployment** when sharing a server, or two
  deployments will consume each other's job queues and registry broadcasts.

## Reference

The backend's API is in the [RabbitMQ reference](../reference/brokers-rabbitmq.md);
the `MessageBroker` interface it implements is in the core [Reference](../reference/blkit.md).
