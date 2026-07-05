---
name: MssqlStateStore
description: A durable, strongly consistent state-store backend that keeps each run's ProcessState in Microsoft SQL Server — its own module, so only applications that use it pull in the database driver
targets:
  - ../../stores/mssql/store.go
---

# MssqlStateStore

> **Status:** Work in progress. See
> [overview.spec.md](./overview.spec.md) for how backends are laid out, and
> [process-state.spec.md](../processes/process-state.spec.md) for what a
> `ProcessState` is.

The SQL Server backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **Microsoft SQL Server**
database. Like the [PostgreSQL backend](./postgres-state-store.spec.md), it is
**durable** (the data survives restarts), **strongly consistent** (a write is always
visible to the next read), and **shareable** (workers on different machines can all
reach the same database). Choose it when your organisation is standardised on SQL
Server.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/mssql`,
so the database driver it needs is only pulled in by applications that actually use
it.

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    mssqlstore "github.com/friendly-business-machines/blkit/stores/mssql"
)

var store = mssqlstore.New(mssqlstore.Config{
    DSN: "sqlserver://user:pass@host:1433?database=blkit",
})
```

---

## Implementation

The backend is built on **`github.com/microsoft/go-mssqldb`** — Microsoft's official
Go driver — used through Go's `database/sql` package. SQL Server-specific choices:

- **A `WriteBatch` executes as a single ordered transaction.** The driver's TDS
  bulk-copy (`mssql.CopyIn`) is deliberately not used for batches: op order within
  a batch is significant (a status flip settles the pending writes that precede
  it, possibly in the same batch), and bulk copy does not preserve that
  interleaving.
- **`DATETIMEOFFSET`** columns for timestamps (time-zone-preserving, sub-microsecond).
- Encoded `Bl` values are stored in **`NVARCHAR(MAX)` validated with `ISJSON`**, so
  the audit history remains queryable with SQL Server's JSON functions
  (`JSON_VALUE`, `OPENJSON`). Because a `Bl` value encodes to a single JSON value —
  which may be a top-level scalar such as `88`, `"high"`, or `true`, and SQL Server's
  `ISJSON` rejects bare scalars — the value is wrapped in an array for the check
  (`ISJSON(N'[' + value + N']') = 1`). This accepts any single JSON value (scalar,
  object, or array) while still rejecting malformed JSON.

## Storage layout

The same three tables as the
[PostgreSQL layout](./postgres-state-store.spec.md#storage-layout) — `blkit_runs`,
`blkit_values`, `blkit_history` — with the same columns, write-op mapping, and
read queries, differing only in dialect:

- `BIGINT IDENTITY(1,1)` in place of `BIGSERIAL` for the arrival-order id.
- `DATETIMEOFFSET` in place of `TIMESTAMPTZ`; `NVARCHAR` in place of `TEXT`/`JSONB`.
- The pending lookup uses a **filtered index**
  (`CREATE INDEX ... WHERE status = 'pending'`), which SQL Server supports directly.
- The current-version query uses `ROW_NUMBER() OVER (PARTITION BY task_id, field
  ORDER BY ts DESC, id DESC)` in place of `DISTINCT ON`.

A `WriteBatch` executes as a single transaction; the `StatusFlip` update settles all
of a task's pending rows in one statement, exactly as in the PostgreSQL backend.

---

## Configuration

The backend is constructed with the connection details for the database — most
importantly a connection string (`DSN`) pointing at the SQL Server instance and
database to use.

Because the data is in a shared database, these same connection details can be
handed to workers running in other processes or on other machines, so they can all
work on the same runs.

### Managed and cloud SQL Server

This backend speaks to any Microsoft SQL Server-compatible server, so it also works —
with no code change, just a different `DSN` — against the popular managed SQL Server
services on the major clouds:

- **Azure** — Azure SQL Database, Azure SQL Managed Instance, and SQL Server on Azure
  virtual machines.
- **AWS** — Amazon RDS for SQL Server.
- **GCP** — Cloud SQL for SQL Server.

There is no separate backend for these — point this backend's `DSN` at the managed
endpoint. Use a primary (read-write) endpoint; reading from an asynchronous read
replica can return stale data.

---

## What it is good for

- **Production runs** that must survive restarts and crashes.
- **Organisations already running SQL Server**, so state lives in familiar,
  already-operated infrastructure.
- **Auditing and reporting** — because the history is stored as ordinary database
  rows, it can be queried directly with SQL.

---

## Concurrency

- **Different runs** are completely independent rows keyed by different run ids, so
  many runs — and many workers — can use the same database at once without
  interfering.
- **Parallel tasks within one run** each write their own rows, so their writes do
  not overwrite each other. The order is sorted out from the timestamps at read time.
  Each `WriteBatch` takes an **exclusive, transaction-scoped application lock on its
  run** (`sp_getapplock`, one per distinct run in the batch, acquired in sorted
  order) before writing, so concurrent batches for the same run serialise rather than
  deadlock on SQL Server's lock ordering — a hazard the other SQL backends do not
  hit. Different runs still write in parallel. As a safety net, a batch that SQL
  Server still picks as a deadlock victim (error 1205) is retried.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a **real
SQL Server instance started on demand**: the test spins up a throwaway SQL Server
container with [testcontainers-go](https://golang.testcontainers.org/) and removes
it afterwards, so `go test` needs only a working Docker daemon. Setting
`BLKIT_TEST_MSSQL_DSN` overrides this and points the suite at an already-running
server instead; the test skips only when neither a DSN nor a reachable Docker daemon
is available. Each subtest uses its own table prefix, so runs are isolated and
repeatable, and the tables are dropped afterwards.

`[@test] ../../stores/mssql/store_test.go`
