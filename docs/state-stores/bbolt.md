# bbolt

> A durable, embedded state store kept in a single bbolt file — the simplest durable
> key-value option, tuned for reads.

The bbolt backend keeps each run's state in a bbolt database — a small, pure-Go
key-value store in a **single file on local disk**. It is **embedded**: there is no
separate server to run, just a file the program opens directly. Data is **durable** —
it survives a restart.

It lives in its own module, so its dependency is only pulled in by applications that
use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    bboltstore "github.com/friendly-business-machines/blkit/stores/bbolt"
)

var store = bboltstore.New(bboltstore.Config{Path: "/var/lib/blkit/state.db"})
```

The backend is built on [go.etcd.io/bbolt](https://github.com/etcd-io/bbolt) — the
maintained etcd fork of Bolt, pure Go with no CGO. Records are encoded as JSON, so
the file stays inspectable with the `bbolt` CLI during debugging.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Read-heavy workloads** — bbolt is a B+tree store that is very fast to read from.
- **Simple operations** — the whole state is one file to back up or move.

## How state is stored

bbolt's nested buckets map naturally onto runs: one top-level bucket holds a
sub-bucket per run, containing the run's metadata, its value records, its history
records, and a small index used to settle a task's pending writes. Keys are built
from a big-endian nanosecond timestamp and a per-bucket sequence number, so bbolt's
natural key order **is** the replay order — reading a run's history is a single
in-order cursor walk with no sorting step. A batch of writes is applied in one bbolt
transaction, atomic and durable when it returns.

## Configuration

Construct the backend with the **path to the database file**. There is no server
address and no credentials — the file is opened directly by the program.

## What to keep in mind

- **It is local to one machine.** The file lives on that machine's disk, so runs
  cannot be shared with workers elsewhere. For shared runs, use a server-based
  backend such as [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One program at a time** opens the file. bbolt allows a single writer, so it suits
  one worker process on the machine rather than several sharing the file.

Compared with the other embedded backends ([Badger](badger.md), [Pebble](pebble.md)),
bbolt is the simplest — a single file and a B+tree that favours fast reads over heavy
write throughput. If you want SQL-queryable history, use [SQLite](sqlite.md) instead.

## Concurrency

Different runs use different keys within the same file, so many runs handled by the
one worker process do not interfere. Parallel tasks within a run each write their own
keys; bbolt serialises writes internally, so concurrent writes within a run are
applied safely one after another.

## Reference

The backend's API is in the [bbolt reference](../reference/stores-bbolt.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
