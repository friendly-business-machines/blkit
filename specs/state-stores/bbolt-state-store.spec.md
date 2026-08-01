---
name: BboltStateStore
description: A durable, embedded state-store backend that keeps each run's ProcessState in a single bbolt file on local disk — its own module, no external server
status: implemented
code:
  - stores/bbolt/
implements: specs/state-stores/overview.spec.md
---

# BboltStateStore

The bbolt backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **bbolt** database — a small,
pure-Go key-value store that lives in a **single file on local disk**. It is
**embedded**: there is no separate server to run, just a file the program opens
directly. Data is durable — it survives a restart.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/bbolt`,
so its dependency is only pulled in by applications that actually use it.

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    bboltstore "github.com/friendly-business-machines/blkit/stores/bbolt"
)

var store = bboltstore.New(bboltstore.Config{Path: "/var/lib/blkit/state.db"})
```

---

## Implementation

The backend is built on **`go.etcd.io/bbolt`** — the maintained etcd fork of Bolt,
pure Go, no CGO. Records are encoded as JSON, so the file stays inspectable with the
`bbolt` CLI during debugging.

## Key layout

bbolt's nested buckets map naturally onto runs. One top-level `runs` bucket, one
sub-bucket per run:

```
runs                         (top-level bucket)
└── {runID}                  (bucket per run)
    ├── meta                 → run metadata record (written via Save)
    ├── values               (bucket)
    │   └── {ts}{seq}        → {task_id, execution_id, field, value, status}
    ├── history              (bucket)
    │   └── {ts}{seq}        → {kind, node_id, execution_id, payload}
    └── pending              (bucket; index for status flips)
        └── {task_id}{ts}{seq} → key of the pending value record
```

`{ts}` is the event timestamp as a big-endian `uint64` of Unix nanoseconds and
`{seq}` is the bucket's `NextSequence()` — so bbolt's natural key order **is** the
replay order `(Timestamp, arrival order)`, and reading a run's history is a single
in-order cursor walk with no sorting step.

How the write ops map onto it:

- **`ValueWrite`** → put a record with `status: pending` into `values`, and an index
  entry into `pending`.
- **`StatusFlip`** → cursor over `pending` with prefix `{task_id}`; for each entry,
  rewrite the referenced value record's status in place, then delete the index
  entry. All of a task's pending records settle inside one transaction.
- **`HistoryEntry`** → put a record into `history`.
- A `WriteBatch` is applied in **one bbolt `Update` transaction** — atomic and
  durable when it returns (bbolt fsyncs on commit), so `Flush` is a no-op.
- **Current version** — walk `values` once, folding the latest committed record per
  `(task_id, field)`. **Full history** — walk `values` and `history` in key order;
  pending and aborted records are included with their status.

---

## Configuration

The backend is constructed with the **path to the database file**. There is no server
address and no credentials — the file is opened directly by the program.

---

## What it is good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Read-heavy workloads** — bbolt is a B+tree store that is very fast to read from.
- **Simple operations** — the whole state is one file to back up or move.

---

## What to keep in mind

- **It is local to one machine.** The file lives on that machine's disk, so runs
  cannot be shared with workers on other machines. For runs shared across machines,
  use a server-based backend such as [PostgreSQL](./postgres-state-store.spec.md) or
  [NATS](./nats-state-store.spec.md).
- **One program at a time** opens the file. bbolt allows a single writer at a time,
  so it suits one worker process on the machine rather than several sharing the file.

Compared with the other embedded backends
([Badger](./badger-state-store.spec.md), [Pebble](./pebble-state-store.spec.md)),
bbolt is the simplest — a single file and a B+tree that favours fast reads over heavy
write throughput.

---

## Concurrency

- **Different runs** use different keys within the same file, so many runs handled by
  the one worker process do not interfere.
- **Parallel tasks within one run** each write their own keys. bbolt serialises
  writes internally, so concurrent writes within a run are applied safely one after
  another.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a store
opened in a **temporary directory** that is removed when the test finishes, so it
needs no external system and runs as part of the module's normal `go test` run.
Reopening the store mid-suite verifies the data survives a close/open cycle.

Verified by [`store_test.go`](../../stores/bbolt/store_test.go).
