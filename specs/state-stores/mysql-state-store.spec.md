---
name: MysqlStateStore
description: A durable, strongly consistent state-store backend that keeps each run's ProcessState in MySQL — its own module built on the go-sql-driver/mysql driver
status: implemented
code:
  - stores/mysql/
implements: specs/state-stores/overview.spec.md
---

# MysqlStateStore

The MySQL backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **MySQL** database. Like the
[PostgreSQL backend](./postgres-state-store.spec.md), on a single primary it is
**durable** (survives restarts), **strongly consistent** (a write is always visible
to the next read), and **shareable** (workers on different machines can all reach the
same database). Choose it when your organisation runs MySQL.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/mysql`,
so its dependency is only pulled in by applications that actually use it.

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    mysqlstore "github.com/friendly-business-machines/blkit/stores/mysql"
)

var store = mysqlstore.New(mysqlstore.Config{
    DSN: "user:pass@tcp(host:3306)/blkit?parseTime=true",
})
```

---

## Implementation

The backend is built on **`github.com/go-sql-driver/mysql`** — the standard, pure-Go
MySQL driver — used through Go's `database/sql` package. This is the best-supported
way to talk to MySQL from Go, and it gives connection pooling and prepared statements
out of the box.

MySQL-specific choices the backend makes:

- **InnoDB tables** — InnoDB is the transactional, crash-safe engine. The backend
  relies on it; the non-transactional MyISAM engine is not used.
- **`utf8mb4`** character set for text.
- Each value a task writes is encoded to bytes and stored in a binary column. Where a
  structured column is preferable, MySQL 8's **native `JSON` type** is used.
- Writes for one task are applied inside a single `database/sql` transaction, so a
  task's outputs land together (matching the "all at once" write described in
  [process-state.spec.md](../processes/process-state.spec.md#writing-a-tasks-outputs-when-it-finishes)).

---

## Storage layout

The same three tables as the
[PostgreSQL layout](./postgres-state-store.spec.md#storage-layout) — `blkit_runs`,
`blkit_values`, `blkit_history` — with the same columns, write-op mapping, and read
queries, differing only in dialect:

- `BIGINT AUTO_INCREMENT` in place of `BIGSERIAL` for the arrival-order id.
- `DATETIME(6)` (microsecond precision, stored as UTC) in place of `TIMESTAMPTZ`.
- Encoded `Bl` values use MySQL 8's native **`JSON`** column type.
- MySQL has no partial indexes, so the pending lookup uses a plain composite index
  on `(run_id, task_id, status)`.
- The current-version query uses `ROW_NUMBER() OVER (PARTITION BY task_id, field
  ORDER BY ts DESC, id DESC)` (MySQL 8 window functions) in place of `DISTINCT ON`.

A `WriteBatch` executes as a single transaction; the `StatusFlip` update settles all
of a task's pending rows in one statement, exactly as in the PostgreSQL backend.

---

## Configuration

The backend is constructed with a connection string (`DSN`) in the
`go-sql-driver/mysql` format — `user:pass@tcp(host:3306)/dbname?params` — pointing at
the MySQL server and database to use. Because the data is in a shared database, these
same details can be handed to workers on other processes or machines so they can all
work on the same runs.

### Managed and cloud MySQL

The backend works — with no code change, just a different `DSN` — against the popular
managed MySQL services:

- **AWS** — Amazon RDS for MySQL, and Amazon Aurora MySQL-Compatible Edition (use the
  cluster **writer** endpoint).
- **GCP** — Cloud SQL for MySQL.
- **Azure** — Azure Database for MySQL.

Point this backend's `DSN` at the managed endpoint; there is no separate backend for
these.

---

## Consistency

- **On a single primary (InnoDB), MySQL is strongly consistent** — read-your-writes,
  no stale reads. This is the recommended setup, and it is enough to meet the state
  store's needs.
- **Asynchronous read replicas can serve stale data.** Route reads to the primary;
  do not read run state from an async replica.
- **Asynchronous replication with automatic failover can lose acknowledged writes.**
  To avoid this, use **semi-synchronous replication** with the lossless
  `AFTER_SYNC` wait point.
- **MySQL Group Replication / InnoDB Cluster** defaults to
  **`group_replication_consistency = EVENTUAL`**, under which a secondary read (or a
  read just after failover) can be stale. Set it to **`BEFORE`** or **`AFTER`** for
  strongly consistent reads.

---

## What it is good for

- **Production runs** that must survive restarts and crashes.
- **Organisations already running MySQL**, so state lives in familiar infrastructure.
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
MySQL server started on demand**: the test spins up a throwaway MySQL container with
[testcontainers-go](https://golang.testcontainers.org/) and removes it afterwards,
so `go test` needs only a working Docker daemon. Setting `BLKIT_TEST_MYSQL_DSN`
overrides this and points the suite at an already-running server instead; the test
skips only when neither a DSN nor a reachable Docker daemon is available. Each
subtest uses its own table prefix, so runs are isolated and repeatable, and the
tables are dropped afterwards.

Verified by [`store_test.go`](../../stores/mysql/store_test.go).
