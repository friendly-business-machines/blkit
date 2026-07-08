---
name: InMemoryMessageBroker
description: The built-in single-process message-broker backend — Go channels and maps, no external dependencies. Part of core; for tests, local development, and single-binary deployments.
targets:
  - ../../core/in_memory_message_broker.go
---

# In-Memory Message Broker

> **Status:** Implemented.

The in-memory broker is the built-in backend that ships in core — Go channels
and mutex-guarded maps, no external broker required. It implements the full
[MessageBroker](overview.spec.md) surface within a single process.

```go
func NewInMemoryMessageBroker(opts ...InMemoryBrokerOption) *InMemoryMessageBroker
```

Each call to `NewInMemoryMessageBroker` creates its **own independent broker**
(matching `NewInMemoryStateStore`); producers and workers that must talk to
each other share the same broker value. Release resources with an explicit
`Close()` — it stops the sweeper and timer goroutines and closes all
subscriber channels.

Suitable for:

- Unit tests that exercise the broker interface without standing up a server.
- Single-binary deployments where the producer code, the worker, and the
  broker live in one process.
- Local development.

It is **not** suitable for multi-process deployments — there is no
inter-process communication and nothing survives a restart. For that, use one
of the [external backends](overview.spec.md#supported-backends).

## Mapping to primitives

The nine standard questions (see
[overview.spec.md § Desired properties](overview.spec.md#desired-properties--admitting-a-future-backend)):

1. **Queue + ack + redelivery** — one pending-job slice per `ProcessKey`,
   guarded by a mutex. Delivering a job moves it to an in-flight map with a
   redelivery timer (default 5× heartbeat interval); a lifecycle report
   settles it, and a fired timer moves it back to pending for redelivery.
2. **Selective consumption** — `FetchJobs` registers the worker's keys; a
   dispatcher goroutine feeds each worker channel only from those keys'
   queues.
3. **Registry** — a map of `workerID → []ProcessRegistration` with
   per-worker deadlines; a sweeper goroutine expires entries whose deadline
   passed and emits `RegistryUpdateHeartbeatLost` to registry subscribers.
4. **Per-instance events / fan-out / replay** — a per-instance subscriber
   list of buffered channels (broadcast). The broker keeps the latest
   lifecycle event and the terminal event per instance for latest-event
   replay; entries are dropped a configurable window after the terminal
   event (default 1h), after which subscriptions get `INSTANCE_NOT_FOUND`.
5. **Delayed delivery** — `time.AfterFunc` per suspended instance publishes
   the `JobResume` when the wait condition is a duration or datetime.
6. **Cancel of queued jobs** — exact, not just best-effort: the pending-job
   slice is scanned under the mutex and the `JobStart` removed.
7. **TLS** — not applicable (no network).
8. **Config + constructor** — functional options:

   ```go
   NewInMemoryMessageBroker(
       bl.WithEventBufferSize(64),          // per-subscriber buffer; default 64
       bl.WithPayloadCipher(cipher),        // default nil
       bl.WithRegistrationTTL(90*time.Second),   // default 90s (3× heartbeat)
       bl.WithInFlightTimeout(150*time.Second),  // default 150s (5× heartbeat)
       bl.WithEventRetention(time.Hour),         // default 1h
   )
   ```

   A `PayloadCipher` is honoured even in-process — payloads still round-trip
   through the [envelope](overview.spec.md#wire-format), so the encryption
   path is exercised by the same code as the external backends.
9. **Local testing** — in-process, no setup; the conformance suite runs as
   part of the normal `go test` run.

## Backpressure

The overview default applies unchanged: a full subscriber buffer drops
events and synthesizes `BACKPRESSURE_DROP` when it recovers.

See [overview.spec.md](overview.spec.md) for the interface and shared
semantics this backend implements.
