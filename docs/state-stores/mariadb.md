# MariaDB

> A durable, strongly consistent, shareable state store backed by MariaDB — a
> separate backend from MySQL, tuned for MariaDB.

The MariaDB backend keeps each run's state in a MariaDB database. On a single primary
it is **durable** (the data survives restarts), **strongly consistent** (a write is
always visible to the next read), and **shareable** (workers on different machines can
all reach the same database). Choose it when your organisation runs MariaDB and you
want run state to live in the same infrastructure you already operate.

MariaDB began as a fork of MySQL and shares its wire protocol, but the two have
diverged — so this is a **separate backend from [MySQL](mysql.md)**, tuned for
MariaDB's own features, clustering, and managed services. It lives in its own module,
so the database driver it needs is only pulled in by applications that use it:

```go
import (
    bl           "github.com/friendly-business-machines/blkit"
    mariadbstore "github.com/friendly-business-machines/blkit/stores/mariadb"
)

// The DSN points at the server and database; the same string handed to workers
// in other processes or on other machines lets them all work on the same runs.
var store = mariadbstore.New(mariadbstore.Config{
    DSN: "user:pass@tcp(host:3306)/blkit?parseTime=true",
})
```

The backend is built on [go-sql-driver/mysql](https://github.com/go-sql-driver/mysql),
which is MariaDB-aware and the best-supported way to reach MariaDB from Go, through
`database/sql`; the constructor forces `parseTime` and UTC so timestamp columns
round-trip as UTC `time.Time` values. Where it diverges from the [MySQL
backend](mysql.md) is in the MariaDB features it leans on: transactional **InnoDB**
tables with `utf8mb4`, MariaDB's **`INSERT ... RETURNING`** (10.5+) to confirm a
stored row in one round-trip, and **`LONGTEXT` columns with a `JSON_VALID` CHECK**
for encoded values (MariaDB's `JSON` type is an alias for exactly that, not a native
binary type as in MySQL 8), so the backend does not depend on a native JSON column.
MariaDB's `ed25519` and `mysql_native_password` authentication plugins are both
supported through the driver.

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Runs shared across many workers**, on one machine or many.
- **Organisations already running MariaDB**, so state lives in familiar
  infrastructure.
- **Auditing and reporting** — because the history is ordinary database rows, it
  can be queried directly with SQL.

## Running the server

The backend needs a reachable MariaDB server; it does not embed one. In rough order
of operational weight:

- **Throwaway container for development** — run a `mariadb:latest` container and point
  the `DSN` at it. The [conformance suite](#what-to-keep-in-mind) starts one
  automatically with testcontainers-go, so you do not need a running server just to
  test.
- **Sidecar or shared container** — in a compose file or Kubernetes pod, run a
  `mariadb` container next to your workers and point the `DSN` at it. Give it a
  durable volume so run state survives a restart.
- **Managed service** — managed MariaDB is less widely offered than MySQL, and this
  is one reason MariaDB has its own backend. The backend works unchanged against a
  managed instance where one exists:
  - **AWS** — Amazon RDS for MariaDB.
  - **MariaDB SkySQL** — the vendor's own managed cloud service.
  - Note the gaps: **Azure Database for MariaDB has been retired**, and **GCP Cloud
    SQL does not offer MariaDB**. On those clouds MariaDB must be self-managed (for
    example on virtual machines or Kubernetes), and the backend connects to it the
    same way.

## Data model

The MariaDB backend uses the **same three-table model** as the
[PostgreSQL backend](postgres.md#data-model) — `<prefix>runs`, `<prefix>values`, and
`<prefix>history`, created on first use, their names carrying a configurable prefix
(`blkit_` by default, see [Configuration](#configuration)). The shape and the reads
are identical; what differs is the MariaDB dialect the schema is expressed in. The
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
| `id` | `BIGINT AUTO_INCREMENT PRIMARY KEY` | Monotonic arrival order — the tie-breaker for two rows with equal timestamps. MariaDB's `AUTO_INCREMENT` generates it, and `INSERT ... RETURNING id` reads it back in the same round-trip. |
| `run_id` | `VARCHAR(255)` | The run this write belongs to. |
| `task_id` | `VARCHAR(255)` | The task that produced it. |
| `execution_id` | `VARCHAR(255)` | The specific execution attempt of that task (a retried task runs under a new execution id). |
| `field` | `VARCHAR(255)` | The field path being written. |
| `value` | `LONGTEXT` (`JSON_VALID` CHECK) | The encoded value, stored as JSON text with a validity constraint, so audit queries can reach inside it. |
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
  window (MariaDB 10.2+ window functions); the newest committed write wins.
- **Full history** — every row for the run, each carrying its `status`, so a failed
  attempt is visible as an aborted write next to the committed one that superseded it.

Two indexes keep both reads cheap: a **replay** index on `(run_id, ts, id)` and,
because MariaDB has no partial indexes, a plain **composite** index on
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
| `payload` | `LONGTEXT` (`JSON_VALID` CHECK) | The entry's detail, as JSON-validated text. |
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

### Managed and cloud MariaDB

Managed MariaDB is less widely offered than MySQL, which is one reason it has its own
backend. The backend works — with no code change, just a different `DSN` — against
**Amazon RDS for MariaDB** and **MariaDB SkySQL**, the vendor's own managed cloud
service. Mind the gaps: **Azure Database for MariaDB has been retired**, and **GCP
Cloud SQL does not offer MariaDB**, so on those clouds MariaDB must be self-managed
and the backend connects to it the same way.

## Consistency

On a single primary (InnoDB), MariaDB is strongly consistent — read-your-writes, no
stale reads — so a worker always sees its own prior writes and those of any other
worker that finished first. This is the recommended setup and is enough to meet the
state store's needs. A few things to keep in mind in HA topologies:

- **Asynchronous read replicas can serve stale data.** Route reads to the primary; do
  not read run state from an async replica.
- **Asynchronous replication with automatic failover can lose acknowledged writes.**
  Use **semi-synchronous replication**.
- **Galera Cluster** is virtually synchronous — writes are certified across all nodes,
  but a read on another node may not have *applied* the latest write yet and can be
  stale. Set **`wsrep_sync_wait`** to force causal (read-your-writes) reads.

## What to keep in mind

- **Route reads to the primary** — see Consistency above.
- **The backend creates its own schema** on first use, so the connecting user needs
  permission to create tables and indexes in the target database the first time it
  runs; afterwards only `SELECT`/`INSERT`/`UPDATE` on the three tables are used.
- **`INSERT ... RETURNING` requires MariaDB 10.5+**, and window functions 10.2+; the
  backend targets a reasonably current MariaDB.
- **History grows without bound** — nothing is deleted by design. For long-lived
  deployments, plan a retention or archival policy against the tables directly.
- **Local testing** — the [conformance suite](overview.md#conformance) starts a throwaway
  MariaDB container with testcontainers-go and removes it afterwards, so `go test`
  needs only a working Docker daemon. Set `BLKIT_TEST_MARIADB_DSN` to point it at an
  already-running server instead; the test skips only when neither a DSN nor a
  reachable Docker daemon is available.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs — and
many workers — can share one database without interfering. Parallel tasks within a
run each write their own rows in a single transaction per batch, so their outputs
land together and never overwrite each other, and the order is sorted out from the
timestamps at read time.

## Reference

The backend's API is in the [MariaDB reference](../reference/stores-mariadb.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
