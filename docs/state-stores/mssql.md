# SQL Server

> A durable, strongly consistent, shareable state store backed by Microsoft SQL
> Server — for shops standardised on SQL Server.

The SQL Server backend keeps each run's state in a Microsoft SQL Server database.
Like the [PostgreSQL backend](postgres.md) it is **durable**, **strongly
consistent**, and **shareable** across workers on different machines. Choose it when
your organisation is standardised on SQL Server.

It lives in its own module, so its driver is only pulled in by applications that
use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    mssqlstore "github.com/friendly-business-machines/blkit/stores/mssql"
)

var store = mssqlstore.New(mssqlstore.Config{
    DSN: "sqlserver://user:pass@host:1433?database=blkit",
})
```

The backend is built on [go-mssqldb](https://github.com/microsoft/go-mssqldb),
Microsoft's official Go driver, through the standard `database/sql` package. It uses
`DATETIMEOFFSET` for timestamps and stores encoded values as JSON-validated
`NVARCHAR(MAX)`, so the audit history stays queryable with SQL Server's JSON
functions.

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Organisations already running SQL Server**, so state lives in familiar,
  already-operated infrastructure.
- **Auditing and reporting** — the history is ordinary rows, queryable with SQL.

## Storage layout

The backend uses the same three tables as the
[PostgreSQL layout](postgres.md#storage-layout) — `blkit_runs`, `blkit_values`,
`blkit_history` — with the same columns and behaviour, differing only in dialect:
`BIGINT IDENTITY` for the arrival-order id, `DATETIMEOFFSET` timestamps, a filtered
index for the pending lookup, and a `ROW_NUMBER()` window function for the
latest-committed-per-field read. A batch of writes executes as a single
transaction, and a task finishing settles all of its pending rows in one statement.

## Configuration

Construct the backend with a connection string (`DSN`) pointing at the SQL Server
instance and database. Because the data is shared, the same details can be handed to
workers in other processes or on other machines.

### Managed and cloud SQL Server

This backend speaks to any SQL Server-compatible server, so it also works — with no
code change, just a different `DSN` — against the managed services:

- **Azure** — Azure SQL Database, Azure SQL Managed Instance, and SQL Server on
  Azure virtual machines.
- **AWS** — Amazon RDS for SQL Server.
- **GCP** — Cloud SQL for SQL Server.

Point the `DSN` at the managed endpoint; use a primary (read-write) endpoint, since
an asynchronous read replica can return stale data.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs and
workers can share one database without interfering. Parallel tasks within a run each
write their own rows, and the order is sorted out from the timestamps at read time.

## Reference

The backend's API is in the [SQL Server reference](../reference/stores-mssql.md);
the `StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
