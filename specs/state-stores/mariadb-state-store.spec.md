---
name: MariadbStateStore
description: A durable, strongly consistent state-store backend that keeps each run's ProcessState in MariaDB — its own module built on the go-sql-driver/mysql driver, using MariaDB-specific features
targets:
  - ../../stores/mariadb/store.go
---

# MariadbStateStore

> **Status:** Work in progress. See
> [overview.spec.md](./overview.spec.md) for how backends are laid out, and
> [process-state.spec.md](../processes/process-state.spec.md) for what a
> `ProcessState` is.

The MariaDB backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **MariaDB** database. On a
single primary it is **durable** (survives restarts), **strongly consistent** (a
write is always visible to the next read), and **shareable** (workers on different
machines can all reach the same database). Choose it when your organisation runs
MariaDB.

MariaDB began as a fork of MySQL and shares its wire protocol, but the two have
diverged — so this is a **separate backend from [MySQL](./mysql-state-store.spec.md)**,
tuned for MariaDB's own features, clustering, and managed services.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/mariadb`,
so its dependency is only pulled in by applications that actually use it.

```go
import (
    bl           "github.com/friendly-business-machines/blkit"
    mariadbstore "github.com/friendly-business-machines/blkit/stores/mariadb"
)

var store = mariadbstore.New(mariadbstore.Config{
    DSN: "user:pass@tcp(host:3306)/blkit?parseTime=true",
})
```

---

## Implementation

The backend is built on **`github.com/go-sql-driver/mysql`** — the standard pure-Go
driver, which is MariaDB-aware and the best-supported way to reach MariaDB from Go —
used through Go's `database/sql` package.

MariaDB-specific choices the backend makes, which is why it does not simply reuse the
MySQL backend:

- **InnoDB tables** — the transactional, crash-safe engine — with the **`utf8mb4`**
  character set.
- **`RETURNING` clause** (MariaDB 10.5+) — an `INSERT ... RETURNING` returns the
  stored row in a single round-trip, so the backend confirms a write without a
  follow-up read. (Standard MySQL has no `RETURNING`.)
- **Value storage** — JSON-encoded values are stored in `LONGTEXT` columns with a
  `JSON_VALID` CHECK constraint. MariaDB's `JSON` type is an alias for exactly that
  (not a native binary type as in MySQL 8), so the backend does not depend on a
  native JSON column.
- **Authentication** — MariaDB's `ed25519` and `mysql_native_password` plugins are
  both supported through the driver.
- Writes for one task are applied inside a single `database/sql` transaction, so a
  task's outputs land together (matching the "all at once" write in
  [process-state.spec.md](../processes/process-state.spec.md#writing-a-tasks-outputs-when-it-finishes)).

---

## Storage layout

The same three tables as the
[PostgreSQL layout](./postgres-state-store.spec.md#storage-layout) — `blkit_runs`,
`blkit_values`, `blkit_history` — with the same columns, write-op mapping, and read
queries, differing only in dialect:

- `BIGINT AUTO_INCREMENT` in place of `BIGSERIAL` for the arrival-order id.
- `DATETIME(6)` (microsecond precision, stored as UTC) in place of `TIMESTAMPTZ`.
- Encoded `Bl` values are stored in **`LONGTEXT` with a `JSON_VALID` CHECK
  constraint** (MariaDB's `JSON` type is an alias for exactly that), queryable with
  MariaDB's JSON functions.
- MariaDB has no partial indexes, so the pending lookup uses a plain composite index
  on `(run_id, task_id, status)`.
- The current-version query uses `ROW_NUMBER() OVER (PARTITION BY task_id, field
  ORDER BY ts DESC, id DESC)` (MariaDB 10.2+ window functions) in place of
  `DISTINCT ON`.
- Inserts use **`INSERT ... RETURNING`** (MariaDB 10.5+) to confirm the stored row
  in one round-trip, per the [Implementation](#implementation) notes above.

A `WriteBatch` executes as a single transaction; the `StatusFlip` update settles all
of a task's pending rows in one statement, exactly as in the PostgreSQL backend.

---

## Configuration

The backend is constructed with a connection string (`DSN`) in the
`go-sql-driver/mysql` format — `user:pass@tcp(host:3306)/dbname?params` — pointing at
the MariaDB server and database to use. Because the data is in a shared database,
these same details can be handed to workers on other processes or machines so they
can all work on the same runs.

### Managed and cloud MariaDB

Managed MariaDB is less widely offered than MySQL, and this is one reason MariaDB has
its own backend. The backend works — with no code change, just a different `DSN` —
against:

- **AWS** — Amazon RDS for MariaDB.
- **MariaDB SkySQL** — the vendor's own managed cloud service.

Note the gaps: **Azure's managed MariaDB service (Azure Database for MariaDB) has
been retired**, and **GCP Cloud SQL does not offer MariaDB**. On those clouds MariaDB
must be self-managed (for example on virtual machines or Kubernetes), and the backend
connects to it the same way.

---

## Consistency

- **On a single primary (InnoDB), MariaDB is strongly consistent** — read-your-writes,
  no stale reads. This is the recommended setup, and it is enough to meet the state
  store's needs.
- **Asynchronous read replicas can serve stale data.** Route reads to the primary;
  do not read run state from an async replica.
- **Asynchronous replication with automatic failover can lose acknowledged writes.**
  To avoid this, use **semi-synchronous replication**.
- **MariaDB Galera Cluster** is virtually synchronous — writes are certified across
  all nodes, but a read on another node may not have *applied* the latest write yet
  and can be stale. Set **`wsrep_sync_wait`** to force causal (read-your-writes)
  reads.

---

## What it is good for

- **Production runs** that must survive restarts and crashes.
- **Organisations already running MariaDB**, so state lives in familiar infrastructure.
- **Auditing and reporting** — the history is ordinary rows, queryable with SQL.

---

## Concurrency

- **Different runs** are independent rows keyed by different run ids, so many runs —
  and many workers — can use the same database at once without interfering.
- **Parallel tasks within one run** each write their own rows, so their writes do not
  overwrite each other. The order is sorted out from the timestamps at read time.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a **real
MariaDB server**: the test reads the server address from the `BLKIT_TEST_MARIADB_DSN`
environment variable and skips when it is unset. CI provides the server in a
container; locally, point the variable at any disposable instance. Each subtest
uses its own table prefix, so runs are isolated and repeatable, and the tables are
dropped afterwards.

`[@test] ../../stores/mariadb/store_test.go`
