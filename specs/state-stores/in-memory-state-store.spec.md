---
name: InMemoryStateStore
description: The built-in state-store backend that keeps a ProcessState in memory — zero dependencies, not durable, the default for tests and local runs
status: implemented
code:
  - core/in_memory_state_store.go
implements: specs/state-stores/overview.spec.md
---

# InMemoryStateStore

The `InMemoryStateStore` keeps each run's [ProcessState](../processes/process-state.spec.md)
in memory. It is **built into the core** blkit module and needs no extra
dependencies, which makes it the default choice for tests, examples, and local
single-process runs.

```go
var store = bl.NewInMemoryStateStore()
```

---

## What it does

It holds everything for a run — the values tasks write and the execution history —
in ordinary in-memory data structures, one set per run, keyed by the run's id. All
the create / save / load / read-back behaviour a state store must provide (see
[overview.spec.md](./overview.spec.md#what-every-state-store-does)) is served
straight from memory.

Because there is no external storage system, saving is immediate — there is nothing
to send over a network and nothing to wait for.

## Implementation

A map from run id to a per-run record, guarded by a `sync.RWMutex`:

- the run's **metadata** (written via Save),
- an append-only slice of **value records** (`{task_id, execution_id, field, value,
  status, ts}`),
- an append-only slice of **history records**.

The [write contract](./overview.spec.md#the-write-contract) maps directly:
`ValueWrite` appends a record with `status: pending`; `StatusFlip` walks the run's
value slice and settles the task's pending records in place; `HistoryEntry` appends.
A `WriteBatch` is applied synchronously under the lock (so it is atomic), which makes
**`Flush` a no-op**. The current version is a fold over the value slice (latest
committed per `(task_id, field)`); the full history is both slices sorted by
`(Timestamp, append order)`. `Config()` returns an error — in-memory state cannot be
reached from another process.

---

## What it is good for

- **Tests** — fast, no setup, no cleanup.
- **Examples and local development** — run a process end to end with no database.
- **Short-lived, single-process runs** where losing the data when the program exits
  is acceptable.

---

## What it is not for

- **It is not durable.** Everything is lost when the program exits. A run that has
  not finished cannot be recovered afterwards.
- **It cannot be shared.** The data lives inside one running program's memory, so it
  cannot be handed to a worker in another process or on another machine. Asking it
  for connection details (so another process could reach it) is an error — there is
  nothing to connect to.

For durable or shared runs, use a backend such as
[PostgreSQL](./postgres-state-store.spec.md) or [NATS](./nats-state-store.spec.md).

---

## Concurrency

Within a single run, several tasks can run in parallel and write to the same
`ProcessState` at once (see
[process-state.spec.md](../processes/process-state.spec.md#many-runs-at-once)). The
`InMemoryStateStore` is safe to use from those parallel tasks — its in-memory data
structures are protected so concurrent writes within a run do not corrupt each other.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). Because it needs no external system,
the suite runs in-process as part of the module's normal `go test` run — no setup and
nothing to clean up.

Verified by [`in_memory_state_store_test.go`](../../core/in_memory_state_store_test.go).
