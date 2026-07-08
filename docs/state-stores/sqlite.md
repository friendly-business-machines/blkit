# SQLite

> A durable, embedded state store kept in a single SQLite file — the most broadly
> useful embedded default, and the only one whose history is queryable with SQL.

The SQLite backend keeps each run's state in a SQLite database — a single file on
local disk. It is **embedded**: there is no separate server to run, just a file the
program opens directly. Data is **durable** — it survives a restart.

Among the embedded backends it has a unique advantage: the data is stored as
ordinary SQL rows, so the full history can be **queried directly with SQL** for
diagnostics and audit — the same benefit [PostgreSQL](postgres.md) offers, without
running a server.

It lives in its own module, so its driver is only pulled in by applications that
use it:

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    sqlitestore "github.com/friendly-business-machines/blkit/stores/sqlite"
)

// New opens (creating if needed) the file, applies the PRAGMAs, and creates the
// schema; the returned *Store is a bl.StateStore used like any other backend. It
// panics on an unopenable file rather than returning an error.
var store = sqlitestore.New(sqlitestore.Config{Path: "/var/lib/blkit/state.db"})
```

The backend is built on the pure-Go [modernc.org/sqlite](https://pkg.go.dev/modernc.org/sqlite)
driver, driven through Go's `database/sql`. The pure-Go driver is chosen
deliberately over the CGO-based `mattn/go-sqlite3`: worker binaries are built with
`CGO_ENABLED=0` for static, distroless containers, and a CGO driver would break
that build. It runs in **WAL mode** (`journal_mode=WAL`) so readers are never
blocked by an in-progress write, funnels all writes through a **single writer
connection** (SQLite permits one writer at a time) while a separate pooled handle
serves reads, and sets a **busy timeout** (`busy_timeout=5000`) so a write that
briefly contends simply waits rather than failing. With `synchronous=FULL` every
committed transaction is durable, so `Flush` is a no-op.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Queryable audit without a server** — the run history is ordinary SQL rows;
  inspect it with any SQLite client (`sqlite3 state.db`).
- **Simple operations** — the whole state is one file to back up or move.

## No server to run

There is nothing to provision: the store is a single file the process opens
directly, so the only operational concerns are the file itself. Because SQLite
permits **one writer at a time**, the file is meant to be owned by a single worker
process — do not point two processes at the same file. Back it up as you would any
file, but with WAL mode active, copy the database together with its `-wal` and
`-shm` sidecar files (or use SQLite's own `.backup`/`VACUUM INTO`) so a checkpoint
in flight is not lost.

## Data model

SQLite uses the **same three tables** as the [PostgreSQL](postgres.md#data-model)
backend, so the shape and its rationale carry over unchanged — only the SQL dialect
differs. Unlike Postgres, the table names are **fixed** (`blkit_runs`,
`blkit_values`, `blkit_history`); there is no table-prefix option, because a SQLite
file holds one deployment. The tables are created on first use.

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
| `value` | `TEXT` | The encoded value as JSON text, queryable with SQLite's built-in JSON functions (`json_extract` etc.). |
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

- **Current state** — the latest `committed` row per `(task_id, field)`, resolved
  with a `ROW_NUMBER()` window partitioned by `(task_id, field)` and ordered by
  `(ts, id)` descending. The newest committed write wins.
- **Full history** — every row for the run in `(ts, id)` order, each carrying its
  `status`, so a failed attempt is visible as an aborted write next to the
  committed one that superseded it.

Two indexes keep both reads cheap: a **replay** index on `(run_id, ts, id)` and a
**partial** index on `(run_id, task_id) WHERE status = 'pending'` — SQLite supports
partial indexes directly — so committing a task's writes touches only its own
pending rows.

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
every read sorts on `(ts, id)`. Because SQLite has no native datetime type,
timestamps are stored as `INTEGER` Unix nanoseconds; the ordering is otherwise
identical to Postgres. Parallel tasks within a run each append their own rows, and
the stored `(ts, id)` pair — not insertion order — defines replay order, which is
what makes the history stable and reproducible across backends.

A `WriteBatch` executes as a **single transaction** on the writer connection, so a
task's outputs land together, and the status flip settles all of a task's pending
rows in one statement — exactly as in the PostgreSQL backend.

## Configuration

```go
type Config struct {
    Path string // path to the database file; required
}
```

- **`Path`** — the path to the database file, opened directly by the program and
  created if it does not exist. There is no server address and no credentials.
  Because the data lives in one local file, it cannot be shared with workers on
  other machines; for shared runs use a server-based backend such as
  [PostgreSQL](postgres.md) or [NATS](nats.md).

## Consistency

SQLite is strongly consistent: a single local file with ACID transactions, where a
committed write is always visible to the next read. There is no replication, so the
stale-read concerns that apply to server-based backends in HA setups do not arise.

## What to keep in mind

- **It is local to one machine.** The file lives on that machine's disk, so runs
  cannot be shared with workers elsewhere. For shared runs, use a server-based
  backend such as [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One worker process per file.** SQLite permits one writer at a time; the backend
  is designed for a single worker process owning the file.
- **History grows without bound** — nothing is deleted by design. For long-lived
  deployments, plan a retention or archival policy against the tables directly.

Compared with the other embedded backends ([bbolt](bbolt.md), [Badger](badger.md),
[Pebble](pebble.md)), the key-value stores are faster at raw writes, but SQLite is
the only one whose history you can query with SQL. When in doubt among the embedded
options, SQLite is the most broadly useful default.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs handled
by the one worker process do not interfere. Parallel tasks within a run each write
their own rows; writes are funnelled through the single writer connection and
applied one after another, while WAL mode keeps reads flowing while that happens.

## Reference

The backend's API is in the [SQLite reference](../reference/stores-sqlite.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
