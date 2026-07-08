# SQL Server

> A durable, strongly consistent, shareable state store backed by Microsoft SQL
> Server — for shops standardised on SQL Server.

The SQL Server backend keeps each run's state in a Microsoft SQL Server database.
Like the [PostgreSQL backend](postgres.md) it is **durable** (the data survives
restarts), **strongly consistent** (a write is always visible to the next read),
and **shareable** (workers on different machines can all reach the same database).
Choose it when your organisation is standardised on SQL Server and you want run
state to live in the same infrastructure you already operate.

It lives in its own module, so the database driver it needs is only pulled in by
applications that use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    mssqlstore "github.com/friendly-business-machines/blkit/stores/mssql"
)

// The DSN points at the server and database; the same string handed to workers
// in other processes or on other machines lets them all work on the same runs.
var store = mssqlstore.New(mssqlstore.Config{
    DSN: "sqlserver://user:pass@host:1433?database=blkit",
})
```

The backend is built on [go-mssqldb](https://github.com/microsoft/go-mssqldb),
Microsoft's official Go driver (it registers the `sqlserver` driver), used through
the standard `database/sql` package. Timestamps are stored as `DATETIMEOFFSET` and
encoded values as JSON-validated `NVARCHAR(MAX)`, so the audit history stays
queryable with SQL Server's JSON functions (`JSON_VALUE`, `OPENJSON`).

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Runs shared across many workers**, on one machine or many.
- **Organisations already running SQL Server**, so state lives in familiar,
  already-operated infrastructure.
- **Auditing and reporting** — because the history is ordinary database rows, it
  can be queried directly with SQL.

## Running the server

The backend needs a reachable SQL Server instance; it does not embed one. In rough
order of operational weight:

- **Throwaway container for development** — run a `mcr.microsoft.com/mssql/server`
  container and point the `DSN` at it. The [conformance suite](#what-to-keep-in-mind)
  starts one automatically with testcontainers-go, so you do not need a running
  server just to test.
- **Sidecar or shared container** — in a compose file or Kubernetes pod, run a SQL
  Server container next to your workers and point the `DSN` at it. Give it a durable
  volume so run state survives a restart.
- **Managed service** — the backend speaks plain SQL Server, so it works unchanged
  against a managed instance. Point the `DSN` at the managed endpoint and supply
  credentials and TLS as the provider requires:
  - **Azure** — Azure SQL Database, Azure SQL Managed Instance, or SQL Server on
    Azure virtual machines.
  - **AWS** — Amazon RDS for SQL Server.
  - **GCP** — Cloud SQL for SQL Server.

## Data model

The SQL Server backend uses the **same three-table model** as the
[PostgreSQL backend](postgres.md#data-model) — `<prefix>runs`, `<prefix>values`, and
`<prefix>history`, created on first use, their names carrying a configurable prefix
(`blkit_` by default, see [Configuration](#configuration)). The shape and the reads
are identical; what differs is the SQL Server dialect the schema is expressed in. The
columns are shown in full below so you can query them directly.

### `blkit_runs` — one row per run

The run's identity and lifecycle: which process it is an instance of, where it has
got to, and when.

| Column | Type | Purpose |
|---|---|---|
| `run_id` | `NVARCHAR(255) PRIMARY KEY` | The run's id — the key every other table joins on. |
| `process_id` | `NVARCHAR(255)` | The process this run is an instance of. |
| `process_version` | `NVARCHAR(64)` | The version of that process definition. |
| `status` | `NVARCHAR(32)` | Lifecycle phase of the run. |
| `published_at` / `started_at` / `completed_at` | `DATETIMEOFFSET(7)` | Lifecycle timestamps (nullable until each is reached). |
| `evaluation_count` | `INT` | How many times the run has been advanced — the optimistic-progress counter. |

The row is upserted with a `MERGE` statement, so saving a run's metadata inserts it
the first time and updates it thereafter.

### `blkit_values` — one row per value a task writes

This is the heart of the model. blkit never overwrites a field; **every write is a
new row**, and the current value of a field is *derived* from these rows rather than
stored in place.

| Column | Type | Purpose |
|---|---|---|
| `id` | `BIGINT IDENTITY(1,1) PRIMARY KEY` | Monotonic arrival order — the tie-breaker for two rows with equal timestamps. SQL Server's `IDENTITY` generates it. |
| `run_id` | `NVARCHAR(255)` | The run this write belongs to. |
| `task_id` | `NVARCHAR(255)` | The task that produced it. |
| `execution_id` | `NVARCHAR(255)` | The specific execution attempt of that task (a retried task runs under a new execution id). |
| `field` | `NVARCHAR(255)` | The field path being written. |
| `value` | `NVARCHAR(MAX)` | The encoded value, stored as JSON text so audit queries can reach inside it. |
| `status` | `NVARCHAR(16)` | `pending`, `committed`, or `aborted` (see below); defaults to `pending`. |
| `ts` | `DATETIMEOFFSET(7)` | When the write was made. |

The `value` column carries a `CHECK` constraint that validates the JSON on the way
in. Because a blkit value encodes to a single JSON value — which may be a top-level
scalar such as `88`, `"high"`, or `true`, and SQL Server's `ISJSON` rejects bare
scalars — the value is wrapped in an array for the check (`ISJSON(N'[' + value + N']')`).
That accepts any single JSON value while still rejecting malformed JSON.

Two behaviours are implemented entirely through the `status` column:

- **A task's outputs become visible all at once, when it finishes.** While a task
  runs, each value it writes lands as a `pending` row and is invisible to the current
  state. When the task completes, all of its pending rows flip to `committed` in a
  **single `UPDATE`**; if it fails, they flip to `aborted`. The outputs never appear
  half-written.
- **Nothing is ever deleted.** Aborted rows stay in the table for diagnostics and
  audit — they simply never satisfy the current-state read.

Two reads sit on top of these rows:

- **Current state** — the latest `committed` row per `(task_id, field)`, resolved
  with a `ROW_NUMBER() OVER (PARTITION BY task_id, field ORDER BY ts DESC, id DESC)`
  window; the newest committed write wins.
- **Full history** — every row for the run, each carrying its `status`, so a failed
  attempt is visible as an aborted write next to the committed one that superseded it.

Two indexes keep both reads cheap: a **replay** index on `(run_id, ts, id)` and a
**filtered** index on `(run_id, task_id) WHERE status = 'pending'` — SQL Server
supports filtered indexes directly — so committing a task's writes touches only its
own pending rows.

### `blkit_history` — one row per execution-history entry

The execution history — the record of what the engine did, step by step.

| Column | Type | Purpose |
|---|---|---|
| `id` | `BIGINT IDENTITY(1,1) PRIMARY KEY` | Arrival-order tie-breaker, as in `blkit_values`. |
| `run_id` | `NVARCHAR(255)` | The run this entry belongs to. |
| `kind` | `NVARCHAR(64)` | The kind of history entry. |
| `node_id` | `NVARCHAR(255)` | The graph node it concerns (nullable). |
| `execution_id` | `NVARCHAR(255)` | The execution attempt it belongs to. |
| `payload` | `NVARCHAR(MAX)` | The entry's detail, as JSON-validated text. |
| `ts` | `DATETIMEOFFSET(7)` | When it was recorded. |

Like the values table it is append-only and read back sorted on `(ts, id)`.

### Ordering under parallelism

Both event tables carry a timestamp **and** an `IDENTITY` id, and every read sorts
on `(ts, id)`. Parallel tasks within a run each append their own rows concurrently;
the order they happen to reach the database in does not matter, because the stored
`(ts, id)` pair — not insertion order — defines replay order. This is what makes the
history stable and reproducible across backends.

## Configuration

```go
type Config struct {
    DSN         string // sqlserver:// connection string; required
    TablePrefix string // table-name prefix; defaults to "blkit_"
}
```

- **`DSN`** — the connection string for the server and database, in the driver's
  `sqlserver://user:pass@host:1433?database=blkit` form. Because the data lives in a
  shared database, the same `DSN` handed to workers in other processes or on other
  machines lets them all work on the same runs.
- **`TablePrefix`** — the prefix on the three table names. Change it to run more than
  one independent blkit deployment inside a single database without their tables
  colliding. It is also part of the application-lock resource name (see
  [Concurrency](#concurrency)), so unrelated deployments sharing a database do not
  serialise against each other.

### Managed and cloud SQL Server

This backend speaks to any Microsoft SQL Server-compatible server, so it also works —
with no code change, just a different `DSN` — against the managed SQL Server services
on the major clouds: **Azure SQL Database**, **Azure SQL Managed Instance**, and SQL
Server on **Azure virtual machines**; **Amazon RDS for SQL Server**; and **Cloud SQL
for SQL Server**. There is no separate backend for these — point the `DSN` at the
managed endpoint. Use a primary (read-write) endpoint; reading from an asynchronous
read replica can return stale data.

## Consistency

SQL Server is strongly consistent against a single primary: a committed write is
visible to the next read, so a worker always sees its own prior writes and those of
any other worker that finished first. The one caveat is **read replicas** — an
asynchronous replica (an Always On readable secondary, or a read-scale replica) can
lag the primary, so blkit must always be pointed at the primary (read-write)
endpoint, never a replica.

## What to keep in mind

- **Point at the primary, not a replica** — see Consistency above.
- **The backend creates its own schema** on first use, so the connecting login needs
  permission to create tables and indexes in the target database the first time it
  runs; afterwards only `SELECT`/`INSERT`/`UPDATE` on the three tables are used.
- **History grows without bound** — nothing is deleted by design. For long-lived
  deployments, plan a retention or archival policy against the tables directly.
- **Local testing** — the [conformance suite](overview.md#conformance) starts a throwaway
  SQL Server container with testcontainers-go and removes it afterwards, so `go test`
  needs only a working Docker daemon. Set `BLKIT_TEST_MSSQL_DSN` to point it at an
  already-running server instead; the test skips only when neither a DSN nor a
  reachable Docker daemon is available.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs — and
many workers — can share one database without interfering. Parallel tasks within a
run each write their own rows, and the order is sorted out from the timestamps at
read time.

SQL Server needs one extra guard the other SQL backends do not. A batch's value
`INSERT` and its pending-status `UPDATE` can take locks in an order that lets two
concurrent batches for the **same run** deadlock. To avoid it, each `WriteBatch`
takes an **exclusive, transaction-scoped application lock per run it touches**
(`sp_getapplock`, one per distinct run, acquired in sorted order) before writing, so
concurrent batches for one run serialise rather than deadlock. Different runs still
write in parallel, and the lock resource is namespaced by table prefix so unrelated
deployments do not serialise. As a safety net, a batch that SQL Server still picks as
a deadlock victim (error 1205) is retried with a small backoff.

## Reference

The backend's API is in the [SQL Server reference](../reference/stores-mssql.md);
the `StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
