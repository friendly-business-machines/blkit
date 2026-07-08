# MySQL

> A durable, strongly consistent, shareable state store backed by MySQL — for shops
> running MySQL.

The MySQL backend keeps each run's state in a MySQL database. On a single primary it
is **durable** (the data survives restarts), **strongly consistent** (a write is
always visible to the next read), and **shareable** (workers on different machines
can all reach the same database), like the [PostgreSQL backend](postgres.md). Choose
it when your organisation runs MySQL and you want run state to live in the same
infrastructure you already operate.

It lives in its own module, so the database driver it needs is only pulled in by
applications that use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    mysqlstore "github.com/friendly-business-machines/blkit/stores/mysql"
)

// The DSN points at the server and database; the same string handed to workers
// in other processes or on other machines lets them all work on the same runs.
var store = mysqlstore.New(mysqlstore.Config{
    DSN: "user:pass@tcp(host:3306)/blkit?parseTime=true",
})
```

The backend is built on [go-sql-driver/mysql](https://github.com/go-sql-driver/mysql),
the standard pure-Go MySQL driver, through `database/sql`; the constructor forces
`parseTime` and UTC so timestamp columns round-trip as UTC `time.Time` values. It
uses transactional **InnoDB** tables, the `utf8mb4` character set, and MySQL 8's
**native `JSON`** column type for encoded values, so the audit history stays
queryable with SQL.

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Runs shared across many workers**, on one machine or many.
- **Organisations already running MySQL**, so state lives in familiar infrastructure.
- **Auditing and reporting** — because the history is ordinary database rows, it
  can be queried directly with SQL.

## Running the server

The backend needs a reachable MySQL server; it does not embed one. In rough order of
operational weight:

- **Throwaway container for development** — run a `mysql:8` container and point the
  `DSN` at it. The [conformance suite](#what-to-keep-in-mind) starts one
  automatically with testcontainers-go, so you do not need a running server just to
  test.
- **Sidecar or shared container** — in a compose file or Kubernetes pod, run a
  `mysql` container next to your workers and point the `DSN` at it. Give it a durable
  volume so run state survives a restart.
- **Managed service** — the backend speaks plain MySQL, so it works unchanged against
  a managed instance. Point the `DSN` at the managed endpoint and supply credentials
  and TLS as the provider requires:
  - **AWS** — Amazon RDS for MySQL, and Amazon Aurora MySQL-Compatible Edition (use
    the cluster **writer** endpoint).
  - **GCP** — Cloud SQL for MySQL.
  - **Azure** — Azure Database for MySQL.

## Data model

The MySQL backend uses the **same three-table model** as the
[PostgreSQL backend](postgres.md#data-model) — `<prefix>runs`, `<prefix>values`, and
`<prefix>history`, created on first use, their names carrying a configurable prefix
(`blkit_` by default, see [Configuration](#configuration)). The shape and the reads
are identical; what differs is the MySQL dialect the schema is expressed in. The
columns are shown in full below so you can query them directly.

### `blkit_runs` — one row per run

The run's identity and lifecycle: which process it is an instance of, where it has
got to, and when.

| Column | Type | Purpose |
|---|---|---|
| `run_id` | `VARCHAR(255) PRIMARY KEY` | The run's id — the key every other table joins on. |
| `process_id` | `VARCHAR(255)` | The process this run is an instance of. |
| `process_version` | `VARCHAR(64)` | The version of that process definition. |
| `status` | `VARCHAR(32)` | Lifecycle phase of the run. |
| `published_at` / `started_at` / `completed_at` | `DATETIME(6)` | Lifecycle timestamps (nullable until each is reached), stored as UTC. |
| `evaluation_count` | `INT` | How many times the run has been advanced — the optimistic-progress counter. |

**Example row**, for a run of an `order-approval` process that has finished:

| run_id | process_id | process_version | status | published_at | started_at | completed_at | evaluation_count |
|---|---|---|---|---|---|---|---|
| `run_8f2c1a90` | `order-approval` | `v1` | `completed` | `2026-07-08 09:11:59.900000` | `2026-07-08 09:12:00.000000` | `2026-07-08 09:12:00.820000` | `3` |

The row is upserted with `INSERT ... ON DUPLICATE KEY UPDATE`, so saving a run's
metadata inserts it the first time and updates it thereafter.

### `blkit_values` — one row per value a task writes

This is the heart of the model. blkit never overwrites a field; **every write is a
new row**, and the current value of a field is *derived* from these rows rather than
stored in place.

| Column | Type | Purpose |
|---|---|---|
| `id` | `BIGINT AUTO_INCREMENT PRIMARY KEY` | Monotonic arrival order — the tie-breaker for two rows with equal timestamps. MySQL's `AUTO_INCREMENT` generates it. |
| `run_id` | `VARCHAR(255)` | The run this write belongs to. |
| `task_id` | `VARCHAR(255)` | The task that produced it. |
| `execution_id` | `VARCHAR(255)` | The specific execution attempt of that task (a retried task runs under a new execution id). |
| `field` | `VARCHAR(255)` | The field path being written. |
| `value` | `JSON` | The encoded value — MySQL 8's native `JSON` type, so audit queries can reach inside it. |
| `status` | `VARCHAR(16)` | `pending`, `committed`, or `aborted` (see below); defaults to `pending`. |
| `ts` | `DATETIME(6)` | When the write was made, stored as UTC. |

**Example rows**, continuing the same run — `check-inventory` succeeded on its
first attempt, but `approve-order` failed once (`exec_b1`) and committed on retry
(`exec_b2`):

| id | run_id | task_id | execution_id | field | value | status | ts |
|---|---|---|---|---|---|---|---|
| `1` | `run_8f2c1a90` | `check-inventory` | `exec_a1` | `in_stock` | `true` | `committed` | `2026-07-08 09:12:00.340000` |
| `2` | `run_8f2c1a90` | `approve-order` | `exec_b1` | `approved` | `true` | `aborted` | `2026-07-08 09:12:00.610000` |
| `3` | `run_8f2c1a90` | `approve-order` | `exec_b2` | `approved` | `true` | `committed` | `2026-07-08 09:12:00.780000` |

Row 2 is the failed attempt's write — it stays in the table but never satisfies a
current-state read. The current state for this run is therefore `in_stock: true`
and `approved: true`, resolved from rows 1 and 3 only.

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
  window (MySQL 8 window functions); the newest committed write wins.
- **Full history** — every row for the run, each carrying its `status`, so a failed
  attempt is visible as an aborted write next to the committed one that superseded it.

Two indexes keep both reads cheap: a **replay** index on `(run_id, ts, id)` and,
because MySQL has no partial indexes, a plain **composite** index on
`(run_id, task_id, status)` so committing a task's writes finds only its own pending
rows.

### `blkit_history` — one row per execution-history entry

The execution history — the record of what the engine did, step by step.

| Column | Type | Purpose |
|---|---|---|
| `id` | `BIGINT AUTO_INCREMENT PRIMARY KEY` | Arrival-order tie-breaker, as in `blkit_values`. |
| `run_id` | `VARCHAR(255)` | The run this entry belongs to. |
| `kind` | `VARCHAR(64)` | The kind of history entry. |
| `node_id` | `VARCHAR(255)` | The graph node it concerns (nullable). |
| `execution_id` | `VARCHAR(255)` | The execution attempt it belongs to. |
| `payload` | `JSON` | The entry's detail. |
| `ts` | `DATETIME(6)` | When it was recorded, stored as UTC. |

**Example rows**, for the same run:

| id | run_id | kind | node_id | execution_id | payload | ts |
|---|---|---|---|---|---|---|
| `1` | `run_8f2c1a90` | `task_started` | `check-inventory` | `exec_a1` | `{}` | `2026-07-08 09:12:00.120000` |
| `2` | `run_8f2c1a90` | `task_completed` | `check-inventory` | `exec_a1` | `{}` | `2026-07-08 09:12:00.410000` |
| `3` | `run_8f2c1a90` | `task_failed` | `approve-order` | `exec_b1` | `{"error": "validation timeout"}` | `2026-07-08 09:12:00.640000` |
| `4` | `run_8f2c1a90` | `task_completed` | `approve-order` | `exec_b2` | `{}` | `2026-07-08 09:12:00.820000` |

Unlike the current-state read over `blkit_values`, this table keeps row 3's failed
attempt visible right alongside the retry that superseded it — nothing here is
ever discarded.

Like the values table it is append-only and read back sorted on `(ts, id)`.

### Ordering under parallelism

Both event tables carry a timestamp **and** an `AUTO_INCREMENT` id, and every read
sorts on `(ts, id)`. Parallel tasks within a run each append their own rows
concurrently; the order they happen to reach the database in does not matter, because
the stored `(ts, id)` pair — not insertion order — defines replay order. This is what
makes the history stable and reproducible across backends.

## Configuration

```go
type Config struct {
    DSN         string // go-sql-driver DSN, e.g. "user:pass@tcp(host:3306)/blkit"; required
    TablePrefix string // table-name prefix; defaults to "blkit_"
}
```

- **`DSN`** — the connection string in the `go-sql-driver/mysql` format,
  `user:pass@tcp(host:3306)/dbname?params`, pointing at the server and database.
  Because the data lives in a shared database, the same `DSN` handed to workers in
  other processes or on other machines lets them all work on the same runs. The
  constructor forces `parseTime` and UTC regardless of what the DSN specifies, so
  `DATETIME(6)` columns round-trip correctly.
- **`TablePrefix`** — the prefix on the three table names. Change it to run more than
  one independent blkit deployment inside a single database without their tables
  colliding.

### Managed and cloud MySQL

This backend speaks to any MySQL-compatible server, so it also works — with no code
change, just a different `DSN` — against the managed MySQL services on the major
clouds: **Amazon RDS for MySQL** and **Amazon Aurora MySQL-Compatible Edition** (use
the cluster **writer** endpoint), **Cloud SQL for MySQL**, and **Azure Database for
MySQL**. There is no separate backend for these — point the `DSN` at the managed
endpoint.

## Consistency

On a single primary (InnoDB), MySQL is strongly consistent — read-your-writes, no
stale reads — so a worker always sees its own prior writes and those of any other
worker that finished first. This is the recommended setup and is enough to meet the
state store's needs. A few things to keep in mind in HA topologies:

- **Asynchronous read replicas can serve stale data.** Route reads to the primary; do
  not read run state from an async replica.
- **Asynchronous replication with automatic failover can lose acknowledged writes.**
  Use **semi-synchronous replication** with the lossless `AFTER_SYNC` wait point.
- **Group Replication / InnoDB Cluster** defaults to
  `group_replication_consistency = EVENTUAL`, under which a secondary read (or a read
  just after failover) can be stale. Set it to `BEFORE` or `AFTER` for strongly
  consistent reads.

## What to keep in mind

- **Route reads to the primary** — see Consistency above.
- **The backend creates its own schema** on first use, so the connecting user needs
  permission to create tables and indexes in the target database the first time it
  runs; afterwards only `SELECT`/`INSERT`/`UPDATE` on the three tables are used.
- **InnoDB is assumed** — the tables rely on transactional, crash-safe InnoDB; do not
  force the non-transactional MyISAM engine.
- **History grows without bound** — nothing is deleted by design. For long-lived
  deployments, plan a retention or archival policy against the tables directly.
- **Local testing** — the [conformance suite](overview.md#conformance) starts a throwaway
  MySQL container with testcontainers-go and removes it afterwards, so `go test`
  needs only a working Docker daemon. Set `BLKIT_TEST_MYSQL_DSN` to point it at an
  already-running server instead; the test skips only when neither a DSN nor a
  reachable Docker daemon is available.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs — and
many workers — can share one database without interfering. Parallel tasks within a
run each write their own rows in a single transaction per batch, so their outputs
land together and never overwrite each other, and the order is sorted out from the
timestamps at read time.

## Reference

The backend's API is in the [MySQL reference](../reference/stores-mysql.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
