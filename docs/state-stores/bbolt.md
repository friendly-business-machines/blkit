# bbolt

> A durable, embedded state store kept in a single bbolt file — the simplest durable
> key-value option, tuned for reads.

The bbolt backend keeps each run's state in a bbolt database — a small, pure-Go
key-value store that lives in a **single file on local disk**. It is **embedded**:
there is no separate server to run, just a file the process opens directly. Data is
**durable** — it survives a restart.

It lives in its own module, so the dependency is only pulled in by applications that
use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    bboltstore "github.com/friendly-business-machines/blkit/stores/bbolt"
)

// New opens (creating if needed) the file and panics if it cannot; the returned
// *Store is a bl.StateStore the rest of blkit uses like any other backend.
store := bboltstore.New(bboltstore.Config{Path: "/var/lib/blkit/state.db"})
defer store.Close()
```

The backend is built on [go.etcd.io/bbolt](https://github.com/etcd-io/bbolt) — the
maintained etcd fork of Bolt, pure Go with no CGO. bbolt is a single-file B+tree with
one writer at a time and many concurrent readers, which is what makes it read-fast and
operationally trivial. Records are encoded as JSON, so the file stays inspectable with
the `bbolt` CLI while debugging.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Read-heavy workloads** — bbolt's B+tree is very fast to read from, and reads never
  block on the single writer.
- **Simple operations** — the whole state is one file to back up, copy, or move.

## No server to run

There is nothing to provision: bbolt is not a server, just a file the process opens.
`Config.Path` names that file, and blkit creates it on first use. Two operational
notes follow from the single-file, single-writer design:

- **One process owns the file.** bbolt takes an exclusive lock on open, so exactly one
  process at a time may have the database open. This suits one worker process per
  machine; it is not a way to share state between processes.
- **Back up the file.** Because all state is in that one path, a consistent copy of the
  file (taken while no process holds it open, or via bbolt's own read transaction) is a
  complete backup.

## Data model

bbolt gives you nested buckets rather than tables, and the layout uses them directly.
A single top-level **`runs`** bucket holds one **sub-bucket per run**, keyed by the run
id. Inside a run's bucket sit its metadata plus three more sub-buckets:

```
runs                              (top-level bucket)
└── {runID}                       (one sub-bucket per run)
    ├── meta                  →   run metadata record (written by Save)
    ├── values                    (sub-bucket: one entry per value written)
    │   └── {ts}{seq}         →   {task_id, execution_id, field, value, status}
    ├── history                   (sub-bucket: one entry per history event)
    │   └── {ts}{seq}         →   {kind, node_id, execution_id, payload}
    └── pending                   (sub-bucket: flip index)
        └── {task_id}{seq…}   →   the values-bucket key of a pending write
```

**Event keys carry their own order.** A `values` or `history` key is 16 bytes: the
event's timestamp as a big-endian `uint64` of Unix nanoseconds, followed by the
bucket's `NextSequence()` counter as a big-endian `uint64`. Because both halves are
fixed-width big-endian, bbolt's natural byte order over the keys **is** the replay
order `(timestamp, arrival)`. Reading a run's history is therefore a single in-order
cursor walk with no sort step.

**Every write is a new entry, never an overwrite.** blkit does not store "the current
value of a field" in place. Each `ValueWrite` appends a new `values` entry with
`status: pending`, and the field's current value is *derived* from these entries at
read time. The lifecycle lives entirely in the `status` field of the record:

- **A task's outputs appear all at once.** While a task runs, each value it writes is a
  `pending` entry and is invisible to the current state. Each pending entry also gets a
  small index row in the `pending` bucket, keyed by `task_id` followed by the value's
  event key.
- **Committing a task is one prefix scan.** When the task finishes, a `StatusFlip`
  seeks the `pending` bucket by the `task_id` prefix, and for each hit rewrites the
  referenced `values` record's status to `committed` (or `aborted` on failure) and
  deletes the index row. Only that task's entries are touched.
- **Nothing is deleted.** Aborted entries stay in the `values` bucket for audit; they
  simply never satisfy the current-state read.

Two reads sit on top of these entries:

- **Current state** — a single walk of the `values` bucket, folding the latest
  `committed` entry per `(task_id, field)`. Because the walk is already in `(ts, seq)`
  order, the last committed entry seen for a field wins.
- **Full history** — a walk of the `values` and `history` buckets in key order, each
  entry carrying its `status`, so an aborted attempt is visible next to the committed
  write that superseded it.

**A whole batch is one transaction.** A `WriteBatch` — a task's value writes, its
status flip, and its history entries — is applied inside one bbolt `Update`
transaction. bbolt fsyncs on commit, so the batch is atomic and durable the moment
`WriteBatch` returns, which is why `Flush` is a no-op for this backend.

## Configuration

```go
type Config struct {
    Path string // path to the database file; required
}
```

- **`Path`** — the file the database lives in. blkit creates it (and its parent state,
  a fresh bbolt file) on first open. There is no address and no credentials: the file
  is opened directly by the process, and the same path reopened later restores the
  same runs. Give each independent deployment its own file.

## Consistency

bbolt is strongly consistent: it serialises writes through a single writer and fsyncs
each committed transaction, so a committed write is always visible to the next read.
Within the one process, a worker always sees its own prior writes and any earlier
task's committed outputs. The scope of that guarantee is the local file — there is no
replication and no second reader on another machine to lag behind.

## What to keep in mind

- **It is local to one machine.** The file lives on that machine's disk, so runs cannot
  be shared with workers elsewhere. For shared runs, use a server-based backend such as
  [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One process owns the file** at a time (the exclusive open lock), so bbolt suits a
  single worker process rather than several sharing the path.
- **History grows without bound** — nothing is deleted by design, so the file grows
  with every value and history entry. For long-lived deployments, plan retention or
  periodic archival, and note that bbolt reuses freed pages but does not shrink the
  file on its own.

Compared with the other embedded backends ([Badger](badger.md), [Pebble](pebble.md)),
bbolt is the simplest — one file and a B+tree that favours fast reads over heavy write
throughput. Badger and Pebble instead keep a directory of LSM files and take writes at
a higher rate. If you want the history to be queryable with SQL while staying embedded,
use [SQLite](sqlite.md) instead.

## Concurrency

Different runs are different sub-buckets under `runs`, so many runs handled by the one
worker process never collide. Parallel tasks within a run each append their own
`values` and `history` entries; bbolt serialises the underlying writes through its
single writer, so concurrent writers within a run are applied safely one after another,
and the stored `(ts, seq)` key — not the order they happened to commit in — fixes the
replay order.

## Reference

The backend's API is in the [bbolt reference](../reference/stores-bbolt.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
