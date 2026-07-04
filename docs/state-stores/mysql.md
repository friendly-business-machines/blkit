# MySQL

> A durable, strongly consistent, shareable state store backed by MySQL — for shops
> running MySQL.

The MySQL backend keeps each run's state in a MySQL database. On a single primary it
is **durable**, **strongly consistent**, and **shareable** across workers on
different machines, like the [PostgreSQL backend](postgres.md). Choose it when your
organisation runs MySQL.

It lives in its own module, so its driver is only pulled in by applications that
use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    mysqlstore "github.com/friendly-business-machines/blkit/stores/mysql"
)

var store = mysqlstore.New(mysqlstore.Config{
    DSN: "user:pass@tcp(host:3306)/blkit?parseTime=true",
})
```

The backend is built on [go-sql-driver/mysql](https://github.com/go-sql-driver/mysql),
the standard pure-Go MySQL driver, through `database/sql`. It uses transactional
InnoDB tables, the `utf8mb4` character set, and MySQL 8's native `JSON` column type
for encoded values, so the audit history stays queryable with SQL.

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Organisations already running MySQL**, so state lives in familiar infrastructure.
- **Auditing and reporting** — the history is ordinary rows, queryable with SQL.

## Storage layout

The backend uses the same three tables as the
[PostgreSQL layout](postgres.md#storage-layout) — `blkit_runs`, `blkit_values`,
`blkit_history` — with the same columns and behaviour, differing only in dialect:
`BIGINT AUTO_INCREMENT` for the arrival-order id, `DATETIME(6)` timestamps stored as
UTC, a plain composite index for the pending lookup (MySQL has no partial indexes),
and a `ROW_NUMBER()` window function for the latest-committed-per-field read. A batch
of writes executes as a single transaction, and a task finishing settles all of its
pending rows in one statement.

## Configuration

Construct the backend with a connection string (`DSN`) in the driver's format,
pointing at the MySQL server and database. Because the data is shared, the same
details can be handed to workers in other processes or on other machines.

### Managed and cloud MySQL

The backend works — with no code change, just a different `DSN` — against the managed
services:

- **AWS** — Amazon RDS for MySQL, and Amazon Aurora MySQL-Compatible Edition (use the
  cluster **writer** endpoint).
- **GCP** — Cloud SQL for MySQL.
- **Azure** — Azure Database for MySQL.

## Consistency

On a single primary (InnoDB), MySQL is strongly consistent — read-your-writes, no
stale reads — and that is the recommended setup. A few things to keep in mind in HA
topologies:

- **Asynchronous read replicas can serve stale data.** Route reads to the primary.
- **Asynchronous replication with automatic failover can lose acknowledged writes.**
  Use semi-synchronous replication with the lossless `AFTER_SYNC` wait point.
- **Group Replication / InnoDB Cluster** defaults to eventual consistency for
  secondary reads. Set `group_replication_consistency` to `BEFORE` or `AFTER` for
  strongly consistent reads.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs and
workers can share one database without interfering. Parallel tasks within a run each
write their own rows, and the order is sorted out from the timestamps at read time.

## Reference

The backend's API is in the [MySQL reference](../reference/stores-mysql.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
