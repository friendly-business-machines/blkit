---
name: StateStoreBackends
description: How blkit's pluggable state-store backends are laid out in the codebase — a lean core module plus one module per backend, so an application imports only the backend it uses and pulls in only that backend's dependencies
targets:
  - ../../core/state_store.go
  - ../../core/state_store_conformance.go
---

# State Store Backends

> **Status:** This spec is a work in progress. The exact shape of the state-store
> interface is being reworked around [ProcessState](../processes/process-state.spec.md).
> This document is about **how the backends are laid out in the codebase** and how
> an application picks one — not the interface method details, which live in
> [state-store.spec.md](../data/state-store.spec.md).

A **state store** is where a [ProcessState](../processes/process-state.spec.md) is
kept. blkit supports different storage backends — an in-memory one, PostgreSQL,
NATS, and more — and lets each application use whichever one it wants.

---

## What every state store does

Every backend does the same job, just against a different storage system. Framed in
terms of the `ProcessState`, for each run of a process a state store must:

- **Create** a fresh `ProcessState` when a run starts — its id and the start values.
- **Save** changes as the run progresses — the values tasks write, and the
  execution-history entries — so nothing is lost if a worker stops.
- **Load** the current `ProcessState` for a run, so a worker can see where the run
  got to and carry it on.
- **Read back** the full history — every value ever written, and the whole
  execution history — for diagnostics and audit.

The exact Go interface a backend implements lives in
[state-store.spec.md](../data/state-store.spec.md). The write contract below —
what events a backend receives and how it must treat them — is shared by every
backend and is normative for the per-backend specs in this directory.

---

## The write contract

As a run progresses, the worker's writer pool (see
[worker.spec.md § Writer Pool](../worker/worker.spec.md#writer-pool)) streams
**write ops** to the store in batches. There are three kinds:

```go
type WriteOp struct {
    RunID string      // the ProcessState's run id (process instance id)
    Kind  WriteOpKind // OpValueWrite | OpStatusFlip | OpHistoryEntry

    ValueWrite   *ValueWrite   // when Kind == OpValueWrite
    StatusFlip   *StatusFlip   // when Kind == OpStatusFlip
    HistoryEntry *HistoryEntry // when Kind == OpHistoryEntry
}

// One field written by a task. Arrives with status Pending.
type ValueWrite struct {
    TaskID      string    // node path, e.g. "screen" or "verify-step.check-docs"
    ExecutionID string    // distinct per task execution (loop iterations differ)
    Field       string    // output field name, e.g. "Score"
    Value       []byte    // the Bl value, encoded (see each backend's layout)
    Timestamp   time.Time // set by the worker when the write was produced
}

// Settles every Pending ValueWrite for TaskID in this run, atomically.
type StatusFlip struct {
    TaskID    string
    NewStatus ValueStatus // Committed (task finished) or Aborted (task failed)
    Timestamp time.Time
}

// One execution-history entry (task scheduled / started / finished / failed,
// path chosen, run started / finished). Mirrors ExecutionStep in
// execution-history.spec.md.
type HistoryEntry struct {
    Kind        string    // e.g. "NODE_STARTED", "GATEWAY_RESOLVED"
    NodeID      *string
    ExecutionID string
    Timestamp   time.Time
    Payload     []byte    // remaining step fields (error, iteration, ...), encoded
}
```

Rules every backend must follow:

- **Value writes land as Pending** and are settled later by a `StatusFlip` for the
  same `(RunID, TaskID)` — flipped to **Committed** when the task finishes
  successfully, or **Aborted** when it fails. A flip settles *all* of that task's
  Pending writes together, so a task's outputs become visible all at once (matching
  [process-state.spec.md](../processes/process-state.spec.md#writing-a-tasks-outputs-when-it-finishes)).
- **Nothing is ever deleted.** Aborted writes stay in storage — the history is kept
  purely for diagnostics and audit, and failed attempts are part of the story.
- **Current-version reads return only Committed values** — the latest committed
  write per `(TaskID, Field)`. Pending and Aborted writes never appear there.
- **Full-history reads return everything**, including Pending and Aborted, each with
  its status.
- **A batch is applied atomically where the backend supports it** (transaction,
  write batch); backends with no batching primitive apply ops one by one, which is
  acceptable because every op is self-describing and idempotent to re-apply.

## Ordering and replay

Ordering follows the same rule as
[execution-history.spec.md](../data/execution-history.spec.md#durability-and-the-statestore):
every event carries its own timestamp, so **arrival order at the backend does not
matter**. When a run is read back, the backend returns events sorted by
`(Timestamp, arrival order)` — the arrival tiebreak is whatever monotonic ordering
the backend assigns on insert (an auto-increment id, a store sequence number), and
it only matters for events that share a timestamp. Display sequence numbers are
computed at render time, not stored.

## The StateStore interface

Every backend implements one Go interface, defined in core (`core/state_store.go`)
alongside the write-op types above. This — plus the record types below — is the
entire surface a backend module has to provide, and the entire surface the rest of
blkit uses to talk to storage. It exists so that backends are interchangeable (the
process code does not change when you swap backends), so the conformance suite can
hold every backend to identical behaviour, and so backend modules have a single
thing to implement.

```go
type StateStore interface {
    // Save persists (upserts) run metadata. Called directly by the executor
    // at evaluation boundaries; also how a fresh run's metadata first lands.
    Save(meta RunMetadata) error

    // WriteBatch applies a batch of write ops. Atomic where the storage
    // engine supports it; engines with no batching primitive apply the ops
    // one by one.
    WriteBatch(ops []WriteOp) error

    // Flush is a per-run durability barrier: when it returns, every
    // previously accepted write op for the run is durable.
    Flush(runID string) error

    // CurrentVersion loads the current version of the run's ProcessState —
    // the latest committed value per (task, field), plus the run metadata —
    // so a worker can pick the run up and carry on.
    CurrentVersion(runID string) (*CurrentVersion, error)

    // FullHistory reads everything stored for the run — every value write
    // (any status) and every history entry, in replay order — for
    // diagnostics and audit.
    FullHistory(runID string) (*FullHistory, error)

    // Config returns the connection details another process needs to reach
    // the same store, or an error for stores that cannot be shared (such as
    // the in-memory store).
    Config() (map[string]string, error)

    // Close releases the store's resources.
    Close() error
}
```

The record types the reads return:

```go
// The current version: what the run works with.
type CurrentVersion struct {
    Meta   RunMetadata
    Values map[string]map[string][]byte // taskID -> field -> JSON-encoded value
}

// The full history: everything, for diagnostics and audit.
type FullHistory struct {
    Meta    RunMetadata
    Values  []ValueRecord   // replay order; includes Pending and Aborted
    History []HistoryRecord // replay order
}

// A stored ValueWrite plus its settled status and arrival number.
type ValueRecord struct {
    TaskID      string
    ExecutionID string
    Field       string
    Value       []byte
    Status      ValueStatus // Pending, Committed, or Aborted
    Timestamp   time.Time
    Seq         uint64      // backend-assigned arrival order; replay tiebreak
}

// A stored HistoryEntry plus its arrival number.
type HistoryRecord struct {
    Kind        string
    NodeID      *string
    ExecutionID string
    Payload     []byte
    Timestamp   time.Time
    Seq         uint64
}

// Run-level metadata, written via Save (bypassing the writer pool).
type RunMetadata struct {
    RunID           string
    ProcessID       string
    ProcessVersion  string
    Status          string
    PublishedAt     *time.Time
    StartedAt       *time.Time
    CompletedAt     *time.Time
    EvaluationCount int
}
```

Semantics every implementation must honour:

- **Unknown run ids are not errors.** `CurrentVersion` and `FullHistory` return
  `(nil, nil)` for a run the store has never seen.
- **Goroutine-safe.** Parallel tasks within one run and many runs at once all write
  through the same store value.
- **Replay order** is `(Timestamp, Seq)` — see
  [Ordering and replay](#ordering-and-replay). `Seq` is whatever monotonic
  arrival number the backend assigns on insert.
- **`Save` is an upsert** keyed by `RunID`; a run may also come into existence
  through its first `WriteBatch` (its metadata is then zero-valued until `Save`).
- Core also provides `ValidateWriteOp` (structural checks backends call at the top
  of `WriteBatch`, so malformed ops fail identically everywhere) and
  `SortValueRecords` / `SortHistoryRecords` (the replay sort, for engines whose
  storage does not already iterate in replay order).

---

## One lean core, one module per backend

The backends are laid out so that **an application only pulls in the dependencies of
the backend it actually uses**.

- The **core** blkit module holds the state-store interface, the shared types, and
  one built-in backend: the **in-memory** store, which needs nothing extra.
- Every **other backend** is its **own separate module**, under `stores/<name>/`,
  which depends on the backend's own driver (a database driver, a messaging client,
  and so on).

Why separate modules: a PostgreSQL backend needs a database driver; a NATS backend
needs a NATS client. If all the backends lived together in one module, then
**everyone** using blkit would pull in **every** driver — even the ones they never
touch. Giving each backend its own module means an application only pulls in the
driver for the backend it imports.

### Layout

```
blkit/                                 the core module
  state_store.go                       the state-store interface + shared types
  in_memory_state_store.go             the built-in in-memory backend (no extra deps)
  ... rest of core ...

  stores/                              each subfolder is its OWN module
    postgres/                          the PostgreSQL backend + its driver dependency
    nats/                              the NATS backend + its client dependency
    ... more backends ...
```

Dependencies only ever point **inward**: a backend module depends on core; core never
depends on a backend. That one-way rule is what keeps core's dependency list small.

---

## Choosing a backend

An application picks a backend by importing it and constructing it, then handing the
result to blkit. You only import the backend you use.

Using the built-in in-memory store — nothing extra to import:

```go
var store = bl.NewInMemoryStateStore()
```

Using the PostgreSQL backend — import its module and construct it:

```go
import (
    "os"

    bl      "github.com/friendly-business-machines/blkit"
    pgstore "github.com/friendly-business-machines/blkit/stores/postgres"
)

var store = pgstore.New(pgstore.Config{DSN: os.Getenv("DATABASE_URL")})
```

Either way, `store` is a state store that the rest of blkit uses the same way — the
process code does not change when you swap backends.

---

## Available backends

The backends fall into three families. Which one fits depends on whether you need
durability, and whether the runs must be shared across more than one machine.

**Not durable — built into core:**

- **In-memory** — [in-memory-state-store.spec.md](./in-memory-state-store.spec.md) —
  no extra dependencies, lost when the program exits. For tests and local runs.

**Durable, shared across machines — a server you run separately:**

- **PostgreSQL** — [postgres-state-store.spec.md](./postgres-state-store.spec.md) —
  durable, strongly consistent, shareable across workers. For production.
- **SQL Server** — [mssql-state-store.spec.md](./mssql-state-store.spec.md) —
  durable, strongly consistent; for shops standardised on Microsoft SQL Server.
- **MySQL** — [mysql-state-store.spec.md](./mysql-state-store.spec.md) — durable,
  strongly consistent on a single primary; for shops running MySQL.
- **MariaDB** — [mariadb-state-store.spec.md](./mariadb-state-store.spec.md) —
  durable, strongly consistent on a single primary; a separate backend from MySQL,
  tuned for MariaDB.
- **NATS** — [nats-state-store.spec.md](./nats-state-store.spec.md) — durable and
  strongly consistent via JetStream; a natural fit when NATS is already your message
  broker.

**Durable, local to one machine — embedded, no server to run:**

- **SQLite** — [sqlite-state-store.spec.md](./sqlite-state-store.spec.md) — a single
  file, and the only embedded option whose history is queryable with SQL. The most
  broadly useful embedded default.
- **bbolt** — [bbolt-state-store.spec.md](./bbolt-state-store.spec.md) — a single
  file, tuned for reads. The simplest durable key-value option.
- **Badger** — [badger-state-store.spec.md](./badger-state-store.spec.md) — a
  directory of files, tuned for heavy writes.
- **Pebble** — [pebble-state-store.spec.md](./pebble-state-store.spec.md) — a
  directory of files, balanced read-and-write performance.

---

## Testing

Every backend is verified against a **shared conformance suite**, so they all behave
identically. The suite lives in core, and each backend module runs it against its own
store. It checks the whole [write contract](#the-write-contract):

- **Roundtrip** — create a run, write values and history entries, load the run back;
  an unknown run id is reported as not found, not an error.
- **Pending invisibility** — a `ValueWrite` is absent from the current version until
  its `StatusFlip(Committed)` arrives, then all of the task's fields appear together.
- **Abort retention** — after a `StatusFlip(Aborted)`, the values never appear in the
  current version but remain in the full history, marked aborted.
- **Latest-per-field** — when a task runs more than once (loopbacks, loops), the
  current version returns the latest committed write per field; earlier ones stay in
  the full history.
- **Replay ordering** — the full history comes back sorted by
  `(Timestamp, arrival order)`, regardless of the order batches arrived.
- **Read-your-writes** — after `Flush` returns, a fresh load (including from a new
  store handle) sees every prior write: the strong-consistency guarantee.
- **Metadata roundtrip** — run metadata written via Save comes back on load.
- **Concurrent batches** — parallel `WriteBatch` calls for the same run (parallel
  tasks) and for different runs are applied safely.

How the suite is run depends on the backend:

- **In-memory** runs the suite in-process — no setup, part of the normal test run.
- **Embedded backends** (SQLite, bbolt, Badger, Pebble) run the suite against a
  store opened in a **temporary directory** that is removed afterwards — no external
  system, part of the normal test run.
- **NATS** runs the suite against a **real JetStream server embedded in the test
  process** (`nats-server` is importable as a Go library) — the genuine engine,
  part of the normal test run, no container needed.
- **SQL server backends** (PostgreSQL, SQL Server, MySQL, MariaDB) run the suite
  against a **real server**: each test reads the server address from a
  `BLKIT_TEST_<NAME>_DSN` environment variable and skips when it is unset. CI
  provides the server in a container; locally, point the variable at any
  disposable instance. Each subtest uses its own table prefix, so runs are
  isolated and repeatable.

Because every backend runs the same suite, a value written through any of them is
read back the same way, and the strong-consistency guarantee is checked for each.

---

## Writing a new backend

To add a backend, create a new module under `stores/<name>/` that:

- depends on core (and nothing in core depends on it),
- depends on whatever driver its storage system needs,
- implements the state-store interface — the create / save / load / read-back
  behaviour described in [What every state store does](#what-every-state-store-does),
- maps the storing of a `ProcessState` (its values and its execution history) onto
  that storage system,
- **passes the shared conformance suite** (see [Testing](#testing)).

Because it is its own module, its driver dependency stays with it and never reaches
applications that do not import it.
