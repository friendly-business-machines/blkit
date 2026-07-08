# Badger

> A durable, embedded state store kept in a local BadgerDB directory — tuned for
> heavy write throughput.

The Badger backend keeps each run's state in a BadgerDB store — a pure-Go key-value
store that lives in a **directory of files on local disk**. It is **embedded**: there
is no separate server to run, just a directory the process opens directly. Data is
**durable** — it survives a restart.

It lives in its own module, so the dependency is only pulled in by applications that
use it:

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    badgerstore "github.com/friendly-business-machines/blkit/stores/badger"
)

// New opens (creating if needed) the store and panics if it cannot; the returned
// *Store is a bl.StateStore the rest of blkit uses like any other backend.
store := badgerstore.New(badgerstore.Config{Path: "/var/lib/blkit/badger"})
defer store.Close()
```

The backend is built on [dgraph-io/badger](https://github.com/dgraph-io/badger) — a
pure-Go, no-CGO LSM-tree store, which is what makes it write-fast: an LSM absorbs a
high rate of writes as sequential appends rather than in-place B+tree updates. Records
are encoded as JSON, and arrival-order numbers come from Badger's own durable monotonic
counter (`DB.GetSequence`), so they keep advancing across a reopen.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Write-heavy workloads** — Badger's LSM design is built to take a high rate of
  writes and handles large values well.

## No server to run

There is nothing to provision: Badger is not a server, it is a set of files in a
directory that the process opens. `Config.Path` names that directory, and blkit
creates it on first use. Two operational notes follow:

- **One process owns the directory.** Badger takes a directory lock on open, so exactly
  one process at a time may have the store open. This suits one worker process per
  machine; it is not a way to share state between processes.
- **Back up the whole directory.** State is spread across the LSM's value log and SST
  files under `Path`, so a backup is a copy of the entire directory (taken while no
  process holds it open), not a single file.

## Data model

Badger has a single flat keyspace rather than tables or buckets, so runs and record
kinds are separated by **key prefixes**. Every key is built so that Badger's
lexicographic byte order **is** the replay order:

```
m|{runID}                       →  run metadata record (written by Save)
v|{runID}|{ts}{seq}             →  {task_id, execution_id, field, value, status}
h|{runID}|{ts}{seq}             →  {kind, node_id, execution_id, payload}
p|{runID}|{task_id}|{ts}{seq}   →  the v| key of a pending write (flip index)
```

The `{ts}{seq}` suffix is 16 fixed-width bytes: the event timestamp as a big-endian
`uint64` of Unix nanoseconds, then the `GetSequence` counter as a big-endian `uint64`.
Because the prefix (`v|{runID}|`) groups a run's values together and the suffix is
fixed-width big-endian, iterating the `v|{runID}|` prefix yields entries already in
`(timestamp, arrival)` order.

**Example keys**, for a run of an `order-approval` process where `check-inventory`
succeeded on its first attempt and `approve-order` failed once (`exec_b1`) before
committing on retry (`exec_b2`) — shown with the `{ts}{seq}` suffix decoded to
`<timestamp>.<seq>` for readability (on disk it is 16 raw big-endian bytes, not this
delimited text):

```
m|run_8f2c1a90                                     →  {"process_id":"order-approval","process_version":"v1","status":"completed", …}

v|run_8f2c1a90|1783501920340000000.1               →  {"task_id":"check-inventory","execution_id":"exec_a1","field":"in_stock","value":true,"status":"committed"}
v|run_8f2c1a90|1783501920610000000.2               →  {"task_id":"approve-order","execution_id":"exec_b1","field":"approved","value":true,"status":"aborted"}
v|run_8f2c1a90|1783501920780000000.3               →  {"task_id":"approve-order","execution_id":"exec_b2","field":"approved","value":true,"status":"committed"}

h|run_8f2c1a90|1783501920120000000.4               →  {"kind":"task_started","node_id":"check-inventory","execution_id":"exec_a1"}
h|run_8f2c1a90|1783501920410000000.5               →  {"kind":"task_completed","node_id":"check-inventory","execution_id":"exec_a1"}
h|run_8f2c1a90|1783501920640000000.6               →  {"kind":"task_failed","node_id":"approve-order","execution_id":"exec_b1","payload":{"error":"validation timeout"}}
h|run_8f2c1a90|1783501920820000000.7               →  {"kind":"task_completed","node_id":"approve-order","execution_id":"exec_b2"}
```

The second `v|` entry is the failed attempt's write. It stays under the `v|` prefix,
but its `aborted` status means the current-state read skips it, folding down to
`in_stock: true` and `approved: true` from the first and third entries; a full
history read returns all three, plus the `h|` entries, with the failed attempt
visible next to the retry that superseded it. While `approve-order`'s pending write
existed (between `exec_b1` starting and being flipped to `aborted`), a companion
`p|run_8f2c1a90|approve-order|1783501920610000000.2` index entry pointed back at it;
`StatusFlip` deleted that index entry as part of settling the attempt.

**Every write is a new key, never an overwrite.** blkit does not keep "the current
value of a field" in a mutable slot; each `ValueWrite` sets a new `v|` key with
`status: pending`, and a field's current value is derived from these entries at read
time. The lifecycle lives in the record's `status`:

- **A task's outputs appear all at once.** Each value a running task writes is a
  `pending` `v|` entry, invisible to current state, and gets a companion `p|` index
  entry keyed by the run and task id.
- **Committing a task is one prefix iteration.** A `StatusFlip` iterates the
  `p|{runID}|{task_id}|` prefix, and for each hit rewrites the referenced `v|` record's
  status to `committed` (or `aborted` on failure) and deletes the index entry. Only
  that task's entries are touched. (The index entries are collected first, then
  mutated, because a Badger transaction does not write while an iterator is open.)
- **Nothing is deleted.** Aborted entries stay under the `v|` prefix for audit; they
  simply never satisfy the current-state read.

Two reads sit on top of these entries:

- **Current state** — one iteration of the `v|{runID}|` prefix, folding the latest
  `committed` entry per `(task_id, field)`.
- **Full history** — an iteration of the `v|{runID}|` and `h|{runID}|` prefixes, each
  entry carrying its `status`, so an aborted attempt is visible next to the committed
  write that superseded it.

**A whole batch is one transaction.** A `WriteBatch` — a task's value writes, its
status flip, and its history entries — is applied inside one Badger `DB.Update`
transaction, so a task's outputs land together. For write throughput the store runs
with **`SyncWrites` off**, and durability is provided at the flush barrier: `Flush`
calls Badger's `DB.Sync`, which fsyncs the write-ahead log so every batch applied
before it is durable when it returns.

One subtlety of Badger's counter: it leases sequence numbers in bands (256 at a time),
so arrival numbers are monotonic but two entries with the same timestamp can be
assigned numbers from different bands. Full-history reads therefore pass the records
through blkit's shared `(ts, seq)` replay sort to normalise that, keeping the ordering
identical to the other backends.

## Configuration

```go
type Config struct {
    Path string // directory for Badger's files; required
}
```

- **`Path`** — the directory BadgerDB keeps its value log and SST files in. blkit
  creates it on first open. There is no address and no credentials: the store is opened
  directly by the process, and the same path reopened later restores the same runs
  (including the durable arrival counter). Give each independent deployment its own
  directory.

## Consistency

Badger is strongly consistent: transactions are serialisable, and a committed write is
visible to the next read within the process. The one thing to be aware of is the
durability boundary — because the store runs with `SyncWrites` off for throughput, a
batch is in memory and the WAL but not necessarily fsynced until a `Flush` (or Badger's
own periodic sync); blkit issues that `Flush` where the write contract requires
durability. The scope of the guarantee is the local directory: there is no replication
and no reader on another machine.

## What to keep in mind

- **It is local to one machine.** The files live on that machine's disk, so runs cannot
  be shared with workers elsewhere. For shared runs, use a server-based backend such as
  [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One process owns the directory** at a time (the directory lock), so Badger suits a
  single worker process rather than several sharing the path.
- **History grows without bound** — nothing is deleted by design. Badger reclaims space
  through LSM compaction and value-log garbage collection over time, but the logical
  record count only grows, so plan retention or archival for long-lived deployments.

Compared with the other embedded backends: [bbolt](bbolt.md) is a single-file B+tree
tuned for reads; Badger and [Pebble](pebble.md) keep a directory of LSM files and take
writes at a higher rate, Badger leaning hardest toward write throughput and large
values. If you want the history to be queryable with SQL while staying embedded, use
[SQLite](sqlite.md) instead.

## Concurrency

Different runs use disjoint key prefixes (`…|{runID}|…`) in the same store, so many runs
handled by the one worker process never collide. Parallel tasks within a run each set
their own `v|` and `h|` keys; Badger applies concurrent transactions safely, and the
stored `(ts, seq)` suffix — not the order the transactions happened to commit in —
fixes the replay order once the shared sort has normalised any band interleaving.

## Reference

The backend's API is in the [Badger reference](../reference/stores-badger.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
