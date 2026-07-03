---
name: BadgerStateStore
description: A durable, embedded state-store backend that keeps each run's ProcessState in a local BadgerDB store — its own module, no external server, tuned for write throughput
targets:
  - ../../stores/badger/store.go
---

# BadgerStateStore

> **Status:** Work in progress. See
> [overview.spec.md](./overview.spec.md) for how backends are laid out, and
> [process-state.spec.md](../processes/process-state.spec.md) for what a
> `ProcessState` is.

The Badger backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **BadgerDB** store — a
pure-Go key-value store that lives in **local files on disk**. It is **embedded**:
there is no separate server to run. Data is durable — it survives a restart.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/badger`,
so its dependency is only pulled in by applications that actually use it.

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    badgerstore "github.com/friendly-business-machines/blkit/stores/badger"
)

var store = badgerstore.New(badgerstore.Config{Path: "/var/lib/blkit/badger"})
```

---

## Implementation

The backend is built on **`github.com/dgraph-io/badger/v4`** — pure Go, no CGO.
Records are encoded as JSON. Arrival-order numbers come from Badger's own
**`DB.GetSequence`** API (a durable monotonic counter), so they survive reopen.

## Key layout

Badger has a flat keyspace, so runs are separated by key prefixes. Keys are built so
that Badger's lexicographic key order **is** the replay order:

```
m|{runID}                        → run metadata record (written via Save)
v|{runID}|{ts}|{seq}             → {task_id, execution_id, field, value, status}
h|{runID}|{ts}|{seq}             → {kind, node_id, execution_id, payload}
p|{runID}|{task_id}|{ts}|{seq}   → key of the pending value record (flip index)
```

`{ts}` is the event timestamp as big-endian Unix nanoseconds; `{seq}` is the
`GetSequence` counter (the arrival tiebreak).

How the write ops map onto it:

- **`ValueWrite`** → set a `v|` record with `status: pending` plus its `p|` index
  entry.
- **`StatusFlip`** → iterate the `p|{runID}|{task_id}|` prefix; rewrite each
  referenced `v|` record's status, delete the index entry. All inside one
  transaction, so the task's outputs settle together.
- **`HistoryEntry`** → set an `h|` record.
- A `WriteBatch` is applied in a single **`DB.Update` transaction** (split only if a
  batch ever exceeds Badger's transaction size limit).
- **`Flush`** calls **`DB.Sync()`** — the backend runs with `SyncWrites` off for
  write throughput, and the sync barrier at Flush provides the durability guarantee
  the [write contract](./overview.spec.md#the-write-contract) requires.
- **Current version** — iterate `v|{runID}|` once, folding the latest committed
  record per `(task_id, field)`. **Full history** — iterate `v|` and `h|` prefixes
  in key order; pending and aborted records are included with their status.

---

## Configuration

The backend is constructed with the **path to a directory** where BadgerDB keeps its
files. There is no server address and no credentials — the store is opened directly
by the program.

---

## What it is good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Write-heavy workloads** — Badger is built to take a high rate of writes, and
  handles large values well.

---

## What to keep in mind

- **It is local to one machine.** The files live on that machine's disk, so runs
  cannot be shared with workers on other machines. For runs shared across machines,
  use a server-based backend such as [PostgreSQL](./postgres-state-store.spec.md) or
  [NATS](./nats-state-store.spec.md).
- **One program at a time** opens the store, so it suits one worker process on the
  machine rather than several sharing the files.

Compared with the other embedded backends: [bbolt](./bbolt-state-store.spec.md) is a
single file tuned for reads; Badger and [Pebble](./pebble-state-store.spec.md) keep a
directory of files and are tuned to take writes at a higher rate.

---

## Concurrency

- **Different runs** use different keys within the same store, so many runs handled by
  the one worker process do not interfere.
- **Parallel tasks within one run** each write their own keys. Badger applies
  concurrent writes safely, so parallel tasks within a run do not corrupt each other.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a store
opened in a **temporary directory** that is removed when the test finishes, so it
needs no external system and runs as part of the module's normal `go test` run.
Reopening the store mid-suite verifies the data survives a close/open cycle.

`[@test] ../../stores/badger/store_test.go`
