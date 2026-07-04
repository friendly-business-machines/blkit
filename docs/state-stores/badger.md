# Badger

> A durable, embedded state store kept in a local BadgerDB directory — tuned for
> heavy write throughput.

The Badger backend keeps each run's state in a BadgerDB store — a pure-Go key-value
store that lives in **local files on disk**. It is **embedded**: there is no separate
server to run. Data is **durable** — it survives a restart.

It lives in its own module, so its dependency is only pulled in by applications that
use it:

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    badgerstore "github.com/friendly-business-machines/blkit/stores/badger"
)

var store = badgerstore.New(badgerstore.Config{Path: "/var/lib/blkit/badger"})
```

The backend is built on [dgraph-io/badger](https://github.com/dgraph-io/badger) —
pure Go, no CGO. Records are encoded as JSON, and arrival-order numbers come from
Badger's own durable monotonic counter, so they survive a reopen.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Write-heavy workloads** — Badger is built to take a high rate of writes and
  handles large values well.

## How state is stored

Badger has a flat keyspace, so runs are separated by key prefixes — one prefix each
for run metadata, value records, history records, and a pending-write index. Keys
embed a big-endian nanosecond timestamp and a sequence number, so Badger's
lexicographic key order **is** the replay order. A batch of writes is applied in a
single Badger transaction. For write throughput the backend runs with synchronous
writes off and provides durability at a flush barrier, which calls Badger's `Sync`.

## Configuration

Construct the backend with the **path to a directory** where BadgerDB keeps its
files. There is no server address and no credentials — the store is opened directly
by the program.

## What to keep in mind

- **It is local to one machine.** The files live on that machine's disk, so runs
  cannot be shared with workers elsewhere. For shared runs, use a server-based
  backend such as [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One program at a time** opens the store, so it suits one worker process on the
  machine rather than several sharing the files.

Compared with the other embedded backends: [bbolt](bbolt.md) is a single file tuned
for reads; Badger and [Pebble](pebble.md) keep a directory of files and take writes
at a higher rate. If you want SQL-queryable history, use [SQLite](sqlite.md) instead.

## Concurrency

Different runs use different keys within the same store, so many runs handled by the
one worker process do not interfere. Parallel tasks within a run each write their own
keys; Badger applies concurrent writes safely.

## Reference

The backend's API is in the [Badger reference](../reference/stores-badger.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
