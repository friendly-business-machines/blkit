# Pebble

> A durable, embedded state store kept in a local Pebble directory — strong,
> balanced read-and-write performance.

The Pebble backend keeps each run's state in a Pebble store — a pure-Go key-value store
that lives in a **directory of files on local disk**. It is **embedded**: there is no
separate server to run, just a directory the process opens directly. Data is
**durable** — it survives a restart.

It lives in its own module, so the dependency is only pulled in by applications that
use it:

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    pebblestore "github.com/friendly-business-machines/blkit/stores/pebble"
)

// New opens (creating if needed) the store and panics if it cannot; the returned
// *Store is a bl.StateStore the rest of blkit uses like any other backend.
store := pebblestore.New(pebblestore.Config{Path: "/var/lib/blkit/pebble"})
defer store.Close()
```

The backend is built on [cockroachdb/pebble](https://github.com/cockroachdb/pebble) —
the pure-Go LSM store that underpins CockroachDB. Being an LSM it takes writes as fast
sequential appends, but Pebble is tuned for balanced read-and-write performance rather
than write throughput above all, which makes it a good general-purpose embedded choice.
Records are encoded as JSON, and arrival-order numbers come from an in-process atomic
counter, seeded at open from the highest sequence already persisted in the store.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Mixed read-and-write workloads** — Pebble is a general-purpose store with strong
  all-round performance, a middle ground between bbolt's read focus and Badger's write
  focus.

## No server to run

There is nothing to provision: Pebble is not a server, it is a set of files in a
directory that the process opens. `Config.Path` names that directory, and blkit creates
it on first use. Two operational notes follow:

- **One process owns the directory.** Pebble takes a directory lock on open, so exactly
  one process at a time may have the store open. This suits one worker process per
  machine; it is not a way to share state between processes.
- **Back up the whole directory.** State is spread across the LSM's WAL and SST files
  under `Path`, so a backup is a copy of the entire directory (taken while no process
  holds it open), not a single file.

## Data model

Pebble uses the **same key scheme as the [Badger backend](badger.md#data-model)** — a
single flat keyspace with run and record kinds separated by key prefixes, and every key
built so that lexicographic byte order **is** the replay order:

```
m|{runID}                       →  run metadata record (written by Save)
v|{runID}|{ts}{seq}             →  {task_id, execution_id, field, value, status}
h|{runID}|{ts}{seq}             →  {kind, node_id, execution_id, payload}
p|{runID}|{task_id}|{ts}{seq}   →  the v| key of a pending write (flip index)
```

The `{ts}{seq}` suffix is 16 fixed-width bytes: the event timestamp as a big-endian
`uint64` of Unix nanoseconds, then the arrival counter as a big-endian `uint64`. The
record shapes, the pending→committed/aborted lifecycle in each value record's `status`,
and the two reads are identical to Badger:

**Example keys** — the same `order-approval` run scenario as the
[Badger example](badger.md#data-model) produces exactly the same keys and values
here, since the layout is identical:

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

Unlike Badger, Pebble's arrival counter is a single strictly-monotonic atomic rather
than leased bands, so under concurrent load its `.1`, `.2`, `.3`, … suffixes can
never arrive out of order the way Badger's occasionally can — no re-sort is needed
to normalise them at read time.

- **Every write is a new key.** Each `ValueWrite` sets a `v|` key with `status:
  pending` plus a companion `p|` index entry; a field's current value is derived, never
  overwritten.
- **Committing a task** is a `StatusFlip` that iterates the `p|{runID}|{task_id}|`
  prefix and rewrites each referenced `v|` record's status to `committed` (or
  `aborted`), deleting the index entry — the entries are collected first, then mutated,
  because the indexed batch's iterator does not see writes made after it was created.
- **Nothing is deleted**; aborted entries remain for audit.
- **Current state** folds the latest `committed` entry per `(task_id, field)` from a
  single scan of the `v|{runID}|` prefix; **full history** iterates the `v|{runID}|` and
  `h|{runID}|` prefixes in key order.

What differs from Badger is the engine idiom and the counter:

- **A whole batch is one `pebble.Batch`** — a task's value writes, its status flip, and
  its history entries — applied atomically. The batch also writes the arrival counter's
  current value under a reserved `!seq` key, so the counter survives a reopen.
- **Batches are applied with `pebble.NoSync`** for throughput; **`Flush` applies an
  empty synced batch** (a `LogData` marker with `pebble.Sync`), the standard Pebble WAL
  sync barrier — when it returns, every previously applied batch is durable. This is
  exactly the grouped-durability shape the write contract's `Flush` requires.
- **The arrival counter is a single in-process atomic**, and writes are serialised by a
  store-level mutex so it can never regress. Because it is one strictly monotonic
  counter (not Badger's leased bands), full-history reads need **no re-sort** — the
  keys already come back in exact replay order. Reads take no lock.

## Configuration

```go
type Config struct {
    Path string // directory for Pebble's files; required
}
```

- **`Path`** — the directory Pebble keeps its WAL and SST files in. blkit creates it on
  first open. There is no address and no credentials: the store is opened directly by
  the process, and the same path reopened later restores the same runs (including the
  persisted arrival counter, reseeded from the `!seq` key). Give each independent
  deployment its own directory.

## Consistency

Pebble is strongly consistent: a committed batch is visible to the next read within the
process, and the store-level write mutex plus the monotonic counter mean reads always
see a coherent, correctly ordered view. The durability boundary is the same shape as
Badger's — batches are applied `NoSync` for throughput and made durable at a `Flush`
sync barrier, which blkit issues where the write contract requires it. The scope of the
guarantee is the local directory: there is no replication and no reader on another
machine.

## What to keep in mind

- **It is local to one machine.** The files live on that machine's disk, so runs cannot
  be shared with workers elsewhere. For shared runs, use a server-based backend such as
  [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One process owns the directory** at a time (the directory lock), so Pebble suits a
  single worker process rather than several sharing the path.
- **History grows without bound** — nothing is deleted by design. Pebble reclaims space
  through LSM compaction over time, but the logical record count only grows, so plan
  retention or archival for long-lived deployments.

Compared with the other embedded backends: [bbolt](bbolt.md) is a single-file B+tree
tuned for reads; [Badger](badger.md) and Pebble keep a directory of LSM files and take
writes at a higher rate, with Pebble aiming for balanced read-and-write performance
rather than leaning to either extreme. If you want the history to be queryable with SQL
while staying embedded, use [SQLite](sqlite.md) instead.

## Concurrency

Different runs use disjoint key prefixes (`…|{runID}|…`) in the same store, so many runs
handled by the one worker process never collide. Parallel tasks within a run each set
their own `v|` and `h|` keys; the store-level mutex serialises the batches so the
arrival counter stays monotonic, and the stored `(ts, seq)` suffix — not the order the
batches happened to apply in — fixes the replay order. Reads run concurrently with
writes without a lock.

## Reference

The backend's API is in the [Pebble reference](../reference/stores-pebble.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
