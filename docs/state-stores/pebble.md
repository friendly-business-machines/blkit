# Pebble

> A durable, embedded state store kept in a local Pebble directory — strong,
> balanced read-and-write performance.

The Pebble backend keeps each run's state in a Pebble store — a pure-Go key-value
store that lives in **local files on disk**. It is **embedded**: there is no separate
server to run. Data is **durable** — it survives a restart.

It lives in its own module, so its dependency is only pulled in by applications that
use it:

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    pebblestore "github.com/friendly-business-machines/blkit/stores/pebble"
)

var store = pebblestore.New(pebblestore.Config{Path: "/var/lib/blkit/pebble"})
```

The backend is built on [cockroachdb/pebble](https://github.com/cockroachdb/pebble) —
the pure-Go LSM store that underpins CockroachDB. Records are encoded as JSON, and
arrival-order numbers come from an atomic counter seeded at open from the highest
existing sequence in the store.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Mixed read-and-write workloads** — Pebble is a general-purpose store with strong
  all-round performance.

## How state is stored

Pebble uses the **same key scheme as the [Badger backend](badger.md#how-state-is-stored)**
— prefixes for run metadata, value records, history records, and a pending-write
index, with big-endian nanosecond timestamps and a sequence tiebreak — so its
lexicographic key order is the replay order. A batch of writes is applied as a single
Pebble batch. For write throughput batches are applied without syncing, and a flush
barrier applies an empty synced batch, which makes every previously applied batch
durable when it returns.

## Configuration

Construct the backend with the **path to a directory** where Pebble keeps its files.
There is no server address and no credentials — the store is opened directly by the
program.

## What to keep in mind

- **It is local to one machine.** The files live on that machine's disk, so runs
  cannot be shared with workers elsewhere. For shared runs, use a server-based
  backend such as [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One program at a time** opens the store, so it suits one worker process on the
  machine rather than several sharing the files.

Compared with the other embedded backends: [bbolt](bbolt.md) is a single file tuned
for reads; [Badger](badger.md) and Pebble keep a directory of files and take writes
at a higher rate, with Pebble aiming for balanced read-and-write performance. If you
want SQL-queryable history, use [SQLite](sqlite.md) instead.

## Concurrency

Different runs use different keys within the same store, so many runs handled by the
one worker process do not interfere. Parallel tasks within a run each write their own
keys; Pebble applies concurrent writes safely.

## Reference

The backend's API is in the [Pebble reference](../reference/stores-pebble.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
