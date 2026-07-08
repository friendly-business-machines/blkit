# Turso

> A durable, embedded state store kept in a Turso Database file — Turso Database
> is the ground-up rewrite of SQLite in Rust, in-process and compatible with
> SQLite's query language and file format. Beta engine.

The Turso backend keeps each run's state in a **Turso Database** file on local
disk. Turso Database is the ground-up **rewrite of SQLite in Rust** (formerly
"Limbo"): an **in-process, embedded** engine — no server to run, just a file the
program opens directly — compatible with SQLite in both query language and file
format. Data is **durable** — it survives a restart.

!!! warning "Beta engine"

    Turso Database is in **beta**. blkit's conformance suite passes against it,
    but the vendor's own guidance applies: use caution with production data and
    keep backups. For a battle-tested embedded SQL store, use the
    [SQLite backend](sqlite.md); choose this backend to run on the Rust engine.

A naming note: this backend is built on **Turso Database, the Rust engine** — not
on libSQL, Turso's earlier C fork of SQLite that powers their managed cloud
service.

It lives in its own module, so its dependency is only pulled in by applications
that use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    tursostore "github.com/friendly-business-machines/blkit/stores/turso"
)

// New opens (creating if needed) the file; the returned *Store is a bl.StateStore
// used like any other backend. It panics on an invalid config or an unopenable
// file rather than returning an error.
var store = tursostore.New(tursostore.Config{Path: "/var/lib/blkit/state.db"})
```

The backend is built on Turso's official Go bindings
([turso.tech/database/tursogo](https://pkg.go.dev/turso.tech/database/tursogo)),
registered with `database/sql` as the `"turso"` driver. They call the Rust core
through [purego](https://github.com/ebitengine/purego) with a bundled platform
library — **no CGO at build time**, so `CGO_ENABLED=0` builds keep working. One
runtime caveat follows: the bundled library is extracted to a temporary directory
and loaded dynamically, so a container image needs a libc-based dynamic loader (for
example `distroless/base`); a fully static `scratch` or `distroless/static` image
cannot load it. Because the engine is in beta, the backend serialises **all** access
through a single connection (`SetMaxOpenConns(1)`), keeping its concurrency surface
out of play.

## What it's good for

- **Running blkit state on the Rust engine** — for teams adopting Turso Database,
  with the same operational shape as the SQLite backend.
- **Single-node deployments** that want durability without running a separate
  database server.
- **SQLite-compatible file format** — the database file follows SQLite's format,
  keeping the state inspectable with familiar tooling.

## No server to run

There is nothing to provision: the store is a single file the process opens
directly. All access is serialised through **one connection**, so the file is meant
to be owned by a single worker process — do not point two processes at the same
file. Because the file format is SQLite-compatible, back it up as you would any
SQLite database. Remember the runtime requirement above: the process needs a
dynamic loader available to load the bundled Rust library.

## Data model

Turso uses the **same three tables** as the [PostgreSQL](postgres.md#data-model)
backend, in SQLite's dialect exactly as the [SQLite backend](sqlite.md#data-model)
does — so the shape and its rationale carry over unchanged. Unlike SQLite, the
table names carry a **configurable prefix** (`blkit_` by default, see
[Configuration](#configuration)), so the schema shown here is `blkit_runs`,
`blkit_values`, and `blkit_history`. The tables are created on first use.

### `blkit_runs` — one row per run

The run's identity and lifecycle: which process it is an instance of, where it has
got to, and when.

| Column | Type | Purpose |
|---|---|---|
| `run_id` | `TEXT PRIMARY KEY` | The run's id — the key every other table joins on. |
| `process_id` | `TEXT` | The process this run is an instance of. |
| `process_version` | `TEXT` | The version of that process definition. |
| `status` | `TEXT` | Lifecycle phase of the run. |
| `published_at` / `started_at` / `completed_at` | `INTEGER` | Lifecycle timestamps as Unix nanoseconds (nullable until each is reached). |
| `evaluation_count` | `INTEGER` | How many times the run has been advanced — the optimistic-progress counter. |

**Example row**, for a run of an `order-approval` process that has finished (the
`INTEGER` columns are Unix nanoseconds; the comment shows the same instant as a
timestamp for readability):

| run_id | process_id | process_version | status | published_at | started_at | completed_at | evaluation_count |
|---|---|---|---|---|---|---|---|
| `run_8f2c1a90` | `order-approval` | `v1` | `completed` | `1783501919900000000` (`09:11:59.900`) | `1783501920000000000` (`09:12:00.000`) | `1783501920820000000` (`09:12:00.820`) | `3` |

The metadata upsert is written as **UPDATE-then-INSERT inside a transaction** rather
than `ON CONFLICT` — plain UPDATE/INSERT is the most conservative SQLite surface,
and the single connection makes it raceless.

### `blkit_values` — one row per value a task writes

This is the heart of the model. blkit never overwrites a field; **every write is a
new row**, and the current value of a field is *derived* from these rows rather
than stored in place.

| Column | Type | Purpose |
|---|---|---|
| `id` | `INTEGER PRIMARY KEY` | SQLite's rowid, supplying the monotonic arrival order — the tie-breaker for two rows with equal timestamps. |
| `run_id` | `TEXT` | The run this write belongs to. |
| `task_id` | `TEXT` | The task that produced it. |
| `execution_id` | `TEXT` | The specific execution attempt of that task (a retried task runs under a new execution id). |
| `field` | `TEXT` | The field path being written. |
| `value` | `TEXT` | The encoded value as JSON text, queryable with SQLite's built-in JSON functions. |
| `status` | `TEXT` | `pending`, `committed`, or `aborted` (see below). |
| `ts` | `INTEGER` | When the write was made, as Unix nanoseconds. |

**Example rows**, continuing the same run — `check-inventory` succeeded on its
first attempt, but `approve-order` failed once (`exec_b1`) and committed on retry
(`exec_b2`):

| id | run_id | task_id | execution_id | field | value | status | ts |
|---|---|---|---|---|---|---|---|
| `1` | `run_8f2c1a90` | `check-inventory` | `exec_a1` | `in_stock` | `true` | `committed` | `1783501920340000000` (`09:12:00.340`) |
| `2` | `run_8f2c1a90` | `approve-order` | `exec_b1` | `approved` | `true` | `aborted` | `1783501920610000000` (`09:12:00.610`) |
| `3` | `run_8f2c1a90` | `approve-order` | `exec_b2` | `approved` | `true` | `committed` | `1783501920780000000` (`09:12:00.780`) |

Row 2 is the failed attempt's write — it stays in the table but never satisfies a
current-state read. The current state for this run is therefore `in_stock: true`
and `approved: true`, resolved from rows 1 and 3 only.

Two behaviours defined by the [shared conformance suite](overview.md#conformance)
are implemented entirely through the `status` column:

- **A task's outputs become visible all at once, when it finishes.** While a task
  runs, each value it writes lands as a `pending` row and is invisible to the
  current state. When the task completes, all of its pending rows flip to
  `committed` in a **single `UPDATE`**; if it fails, they flip to `aborted`. The
  outputs never appear half-written.
- **Nothing is ever deleted.** Aborted rows stay in the table for diagnostics and
  audit — they simply never satisfy the current-state read.

Two reads sit on top of these rows:

- **Current state** — the latest `committed` value per `(task_id, field)`. Where
  SQLite resolves this with a `ROW_NUMBER()` window, Turso deliberately stays inside
  the beta engine's well-supported SQL: it scans the committed rows in `(ts, id)`
  order and **folds in Go**, letting the last write per field win. No window
  function is used.
- **Full history** — every row for the run in `(ts, id)` order, each carrying its
  `status`, so a failed attempt is visible as an aborted write next to the committed
  one that superseded it.

Two indexes back these reads: a **replay** index on `(run_id, ts, id)` and — again
staying conservative — a **plain composite** index on `(run_id, task_id, status)`
for the pending lookup, rather than the partial index the SQLite backend uses.

### `blkit_history` — one row per execution-history entry

The execution history — the record of what the engine did, step by step.

| Column | Type | Purpose |
|---|---|---|
| `id` | `INTEGER PRIMARY KEY` | Arrival-order tie-breaker (rowid), as in `blkit_values`. |
| `run_id` | `TEXT` | The run this entry belongs to. |
| `kind` | `TEXT` | The kind of history entry. |
| `node_id` | `TEXT` | The graph node it concerns (nullable). |
| `execution_id` | `TEXT` | The execution attempt it belongs to. |
| `payload` | `TEXT` | The entry's detail, as JSON text. |
| `ts` | `INTEGER` | When it was recorded, as Unix nanoseconds. |

**Example rows**, for the same run:

| id | run_id | kind | node_id | execution_id | payload | ts |
|---|---|---|---|---|---|---|
| `1` | `run_8f2c1a90` | `task_started` | `check-inventory` | `exec_a1` | `{}` | `1783501920120000000` (`09:12:00.120`) |
| `2` | `run_8f2c1a90` | `task_completed` | `check-inventory` | `exec_a1` | `{}` | `1783501920410000000` (`09:12:00.410`) |
| `3` | `run_8f2c1a90` | `task_failed` | `approve-order` | `exec_b1` | `{"error": "validation timeout"}` | `1783501920640000000` (`09:12:00.640`) |
| `4` | `run_8f2c1a90` | `task_completed` | `approve-order` | `exec_b2` | `{}` | `1783501920820000000` (`09:12:00.820`) |

Unlike the current-state read over `blkit_values`, this table keeps row 3's failed
attempt visible right alongside the retry that superseded it — nothing here is
ever discarded.

Like the values table it is append-only and read back sorted on `(ts, id)`.

### Ordering under parallelism

Both event tables carry a timestamp **and** an auto-increment id (the rowid), and
every read sorts on `(ts, id)`. As in SQLite, timestamps are stored as `INTEGER`
Unix nanoseconds. Parallel tasks within a run each append their own rows, and the
stored `(ts, id)` pair — not insertion order — defines replay order, which is what
makes the history stable and reproducible across backends.

A `WriteBatch` executes as a **single transaction**, so a task's outputs land
together, and the status flip settles all of a task's pending rows in one statement.

## Configuration

```go
type Config struct {
    Path        string // path to the database file; required
    TablePrefix string // table-name prefix; defaults to "blkit_"
}
```

- **`Path`** — the path to the database file, opened directly by the program and
  created if it does not exist. There is no server address and no credentials.
- **`TablePrefix`** — the prefix on the three table names (default `blkit_`). Change
  it to keep more than one independent blkit deployment inside a single file without
  their tables colliding. It must match `^[a-z_][a-z0-9_]*$`; an invalid prefix
  panics at construction.

Because the data lives in one local file, it cannot be shared with workers on other
machines; for shared runs use a server-based backend such as
[PostgreSQL](postgres.md) or [NATS](nats.md).

## Consistency

Turso Database is an embedded engine with transactions on a single local file: a
committed write is always visible to the next read. There is no replication, so the
stale-read concerns that apply to server-based backends in HA setups do not arise.
(The driver's remote-sync features are not used by this backend.)

## What to keep in mind

- **Beta engine** — see the warning above. The [SQLite backend](sqlite.md) is the
  conservative choice; this one is for the Rust engine.
- **It is local to one machine.** The file lives on that machine's disk, so runs
  cannot be shared with workers elsewhere. For shared runs, use a server-based
  backend such as [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One worker process per file**, with all access serialised through a single
  connection.
- **Container images need a dynamic loader** for the bundled runtime library (see
  the intro above).
- **History grows without bound** — nothing is deleted by design. For long-lived
  deployments, plan a retention or archival policy against the tables directly.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs handled
by the one worker process do not interfere. Parallel tasks within a run each write
their own rows; all access is funnelled through the single connection and applied
one statement after another.

## Reference

The backend's API is in the [Turso reference](../reference/stores-turso.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
