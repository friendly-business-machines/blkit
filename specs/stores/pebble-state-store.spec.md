---
name: PebbleStateStore
description: A durable, embedded state-store backend that keeps each run's ProcessState in a local Pebble store — its own module, no external server, strong general-purpose performance
targets:
  - ../../stores/pebble/store.go
---

# PebbleStateStore

> **Status:** Work in progress. See
> [overview.spec.md](./overview.spec.md) for how backends are laid out, and
> [process-state.spec.md](../processes/process-state.spec.md) for what a
> `ProcessState` is.

The Pebble backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **Pebble** store — a pure-Go
key-value store that lives in **local files on disk**. It is **embedded**: there is
no separate server to run. Data is durable — it survives a restart.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/pebble`,
so its dependency is only pulled in by applications that actually use it.

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    pebblestore "github.com/friendly-business-machines/blkit/stores/pebble"
)

var store = pebblestore.New(pebblestore.Config{Path: "/var/lib/blkit/pebble"})
```

---

## Implementation

The backend is built on **`github.com/cockroachdb/pebble`** — the pure-Go LSM store
that underpins CockroachDB. Records are encoded as JSON. Arrival-order numbers come
from an atomic in-process counter, seeded at open from the highest existing `{seq}`
in the store.

## Key layout

The **same key scheme as the [Badger backend](./badger-state-store.spec.md#key-layout)**
— `m|`, `v|`, `h|`, and `p|` prefixes with big-endian nanosecond timestamps and a
sequence tiebreak — so Pebble's lexicographic key order is the replay order, and the
write-op mapping (pending record + flip index, flip walks the `p|` prefix and
rewrites) is identical.

What differs is the engine idiom:

- A `WriteBatch` is applied as a single **`pebble.Batch`** — atomic on apply.
- Batches are applied with **`pebble.NoSync`** for write throughput; **`Flush`
  applies an empty batch with `pebble.Sync`**, which acts as a WAL sync barrier —
  when it returns, every previously applied batch is durable. This is the standard
  Pebble pattern for grouped durability and is exactly the shape the
  [write contract](./overview.spec.md#the-write-contract)'s Flush requires.
- Iteration uses Pebble's prefix iterators (`IterOptions` with bounds) for the
  current-version fold and full-history reads.

---

## Configuration

The backend is constructed with the **path to a directory** where Pebble keeps its
files. There is no server address and no credentials — the store is opened directly
by the program.

---

## What it is good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Mixed read-and-write workloads** — Pebble is a general-purpose store with strong
  all-round performance.

---

## What to keep in mind

- **It is local to one machine.** The files live on that machine's disk, so runs
  cannot be shared with workers on other machines. For runs shared across machines,
  use a server-based backend such as [PostgreSQL](./postgres-state-store.spec.md) or
  [NATS](./nats-state-store.spec.md).
- **One program at a time** opens the store, so it suits one worker process on the
  machine rather than several sharing the files.

Compared with the other embedded backends: [bbolt](./bbolt-state-store.spec.md) is a
single file tuned for reads; [Badger](./badger-state-store.spec.md) and Pebble keep a
directory of files and take writes at a higher rate. Pebble aims for balanced
read-and-write performance.

---

## Concurrency

- **Different runs** use different keys within the same store, so many runs handled by
  the one worker process do not interfere.
- **Parallel tasks within one run** each write their own keys. Pebble applies
  concurrent writes safely, so parallel tasks within a run do not corrupt each other.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a store
opened in a **temporary directory** that is removed when the test finishes, so it
needs no external system and runs as part of the module's normal `go test` run.
Reopening the store mid-suite verifies the data survives a close/open cycle.

`[@test] ../../stores/pebble/store_test.go`
