# In-Memory

> The built-in broker that ships in core — Go channels and mutex-guarded maps,
> no external server required. For tests, examples, and single-binary
> deployments.

The in-memory backend implements [MessageBroker](overview.md) entirely inside
your Go process, using channels and mutex-guarded maps. It covers every broker
duty — the job queue, the registry, per-instance events, suspend-resume timers,
and cancellation — without a network hop or an external server, and it is held
to the same [conformance suite](overview.md#conformance) as every other backend,
so it behaves identically to them.

Because it ships in core, there is nothing to import beyond blkit itself and
nothing to construct beyond the broker. Reach for it in tests, in examples, and
in single-binary deployments where the producer, the worker, and the broker all
live in one process. When you need to bridge separate processes or machines, or
survive a restart, switch to one of the
[external backends](overview.md#the-backends) — the interface is the same.

```go
import bl "github.com/friendly-business-machines/blkit"

// NewInMemoryMessageBroker returns a ready broker value (no error to check).
// Each call creates its own independent broker, so producers and workers that
// must talk to each other share the same value.
broker := bl.NewInMemoryMessageBroker()
defer broker.Close()
```

`Close` stops the sweeper and any pending timers, closes every subscriber and
fetch channel, and makes subsequent calls fail with `ErrBrokerClosed`. It is
idempotent, so a `defer broker.Close()` is always safe.

## What it's good for

- **Unit and integration tests** that exercise the broker interface without
  standing up a server.
- **Runnable examples** where a self-contained snippet must work with no setup.
- **Single-binary deployments** where the producer code, the worker, and the
  broker all live in one process.
- **Local development.**

## No server to run

Unlike [Redis](redis.md), [NATS](nats.md), or the cloud brokers, there is
nothing to provision: the broker *is* the `*InMemoryMessageBroker` value you
construct. It runs in-process, holds all its state on the heap, and needs no
container, connection string, or credentials. Construct it, share the value with
your workers and producers, and `Close` it when you are done.

## How it works

Every broker duty maps onto plain in-process primitives, all guarded by a single
mutex, with a background sweeper goroutine and per-instance timers doing the
time-based work.

- **Job queue, ack, and redelivery** — one **pending-job slice per process key**
  (`ProcessKey`). `Submit` and due resume timers append a job; a `FetchJobs`
  dispatcher goroutine pops the first job for one of the worker's keys and moves
  it to an **in-flight map** with a redelivery `time.AfterFunc` timer (default
  `InFlightTimeout`, 150s). A terminal lifecycle report settles the job and stops
  its timer; if the timer fires first — because the worker died mid-job — the job
  is returned to the **front** of its queue and redelivered. This is the
  at-least-once guarantee, done entirely with slices and timers.
- **Selective consumption** — `FetchJobs` takes the keys the worker registered,
  and its dispatcher only ever pops from those keys' queues, so a worker sees
  only work it can handle.
- **Registry** — a map of `workerID → []ProcessRegistration` with a per-worker
  deadline. `RegisterProcesses` and `Heartbeat` push the deadline out by
  `RegistrationTTL`; the **sweeper goroutine** expires any worker whose deadline
  has passed and emits `RegistryUpdateHeartbeatLost` to registry subscribers.
  Because the registry is always current in-process, `Submit` validates against
  it directly — an unadvertised process is genuinely unknown, with no cold-start
  snapshot wait.
- **Per-instance events, fan-out, and replay** — each instance keeps a list of
  **buffered subscriber channels**; publishing an event fans it out to all of
  them (true broadcast). The broker retains the **latest lifecycle event** and
  the **terminal event** per instance, so a late subscriber first receives those
  for catch-up before following live. Once an instance finishes, its record is
  kept for `EventRetention` (default 1h) and then purged by the sweeper; after
  that, subscribing yields `INSTANCE_NOT_FOUND`.
- **Suspend-resume timers** — `ReportSuspended` with a resume time schedules a
  `time.AfterFunc` per instance that enqueues the `JobResume` back onto the
  instance's job queue when it fires. A subsequent finish or a second suspend
  stops and replaces the pending timer.
- **Cancel of a queued job** — **exact, not best-effort**: `Cancel` scans the
  pending-job slice under the mutex and removes the instance's `JobStart` if it
  has not yet been delivered, then publishes the terminal `Cancelled` event
  itself. If a worker already holds the instance, it falls through to the normal
  `JobCancel` route (which requires `AllowExternalCancel`).

Even though nothing leaves the process, every job and event is **round-tripped
through the CBOR wire envelope** (and any configured cipher) on the way through
the broker, so the in-memory backend exercises exactly the same encoding and
encryption path as the external ones.

## Configuration

There is no `Config` struct and nothing you *must* set — `NewInMemoryMessageBroker()`
with no arguments is a complete, working broker. The constructor accepts
**functional options** for the few knobs that exist, all of which have sensible
defaults derived from the 30s worker heartbeat:

```go
broker := bl.NewInMemoryMessageBroker(
    bl.WithEventBufferSize(64),                  // per-subscriber buffer; default 64
    bl.WithPayloadCipher(cipher),                // end-to-end payload cipher; default nil
    bl.WithRegistrationTTL(90*time.Second),      // default 90s  (3× heartbeat)
    bl.WithInFlightTimeout(150*time.Second),     // default 150s (5× heartbeat)
    bl.WithEventRetention(time.Hour),            // default 1h
)
```

- **`WithEventBufferSize`** (64) — the capacity of each per-subscriber event
  channel. When a subscriber's buffer overflows, events are dropped and a
  synthetic `BACKPRESSURE_DROP` error is delivered once the buffer recovers, so a
  slow consumer never blocks the broker.
- **`WithPayloadCipher`** (nil) — an optional [`PayloadCipher`](../reference/blkit.md)
  that encrypts message payloads end to end. It is honoured even in-process:
  payloads still round-trip through the envelope, so the encryption path is
  exercised by the same code as the external backends. All brokers and workers in
  a deployment must share the same cipher.
- **The three timing knobs** rarely need changing:
  - **`WithRegistrationTTL`** (90s) — how long a worker's registration outlives
    its last heartbeat before the sweeper declares it lost. Raise it if
    heartbeats can be delayed; lower it for faster failure detection.
  - **`WithInFlightTimeout`** (150s) — how long a delivered job may go unsettled
    before it is redelivered. It must comfortably exceed your longest task step,
    or a slow-but-alive worker's job will be redelivered under it.
  - **`WithEventRetention`** (1h) — how long after an instance finishes its
    latest lifecycle and terminal events stay replayable to late subscribers.

## Local testing

The [conformance suite](overview.md#conformance) that every backend must pass
runs the in-memory broker with no setup at all — there is no container to start
and no `BLKIT_TEST_*` variable to point at an external instance, because the
broker is created in-process during the test. This makes it the fastest backend
to test against and the natural choice for your own tests of process definitions
and worker logic.

## What to keep in mind

- **Nothing is durable.** All state lives on the heap, so a process exit or crash
  loses the queue, the registry, and every in-flight instance. It is for tests,
  examples, and single-binary runs — not for work that must survive a restart.
- **Single process only.** There is no inter-process communication, so it cannot
  bridge two separate binaries or two machines. Every producer and worker that
  needs to see the same jobs must share the one broker value; for anything wider,
  use an [external backend](overview.md#the-backends).
- **Semantics are identical to the external backends.** Because it passes the
  same conformance suite and round-trips through the same wire envelope, code
  written and tested against the in-memory broker behaves the same when you swap
  in Redis, NATS, or a cloud broker — the only differences are durability and
  reach, not behaviour.

## Reference

`NewInMemoryMessageBroker` and the `MessageBroker` interface it implements are
in the core API [Reference](../reference/blkit.md).
