# MariaDB

> A durable, strongly consistent, shareable state store backed by MariaDB — a
> separate backend from MySQL, tuned for MariaDB.

The MariaDB backend keeps each run's state in a MariaDB database. On a single primary
it is **durable**, **strongly consistent**, and **shareable** across workers on
different machines. Choose it when your organisation runs MariaDB.

MariaDB began as a fork of MySQL and shares its wire protocol, but the two have
diverged — so this is a **separate backend from [MySQL](mysql.md)**, tuned for
MariaDB's own features, clustering, and managed services.

It lives in its own module, so its driver is only pulled in by applications that
use it:

```go
import (
    bl           "github.com/friendly-business-machines/blkit"
    mariadbstore "github.com/friendly-business-machines/blkit/stores/mariadb"
)

var store = mariadbstore.New(mariadbstore.Config{
    DSN: "user:pass@tcp(host:3306)/blkit?parseTime=true",
})
```

The backend is built on [go-sql-driver/mysql](https://github.com/go-sql-driver/mysql),
which is MariaDB-aware, through `database/sql`. It uses transactional InnoDB tables
with `utf8mb4`, MariaDB's `INSERT ... RETURNING` (10.5+) to confirm writes in one
round-trip, and `LONGTEXT` columns with a `JSON_VALID` constraint for encoded values
— so it does not depend on a native JSON column type.

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Organisations already running MariaDB**, so state lives in familiar
  infrastructure.
- **Auditing and reporting** — the history is ordinary rows, queryable with SQL.

## Storage layout

The backend uses the same three tables as the
[PostgreSQL layout](postgres.md#storage-layout) — `blkit_runs`, `blkit_values`,
`blkit_history` — with the same columns and behaviour, differing only in dialect:
`BIGINT AUTO_INCREMENT` for the arrival-order id, `DATETIME(6)` timestamps stored as
UTC, a plain composite index for the pending lookup, a `ROW_NUMBER()` window function
for the latest-committed-per-field read, and `INSERT ... RETURNING` on inserts. A
batch of writes executes as a single transaction, and a task finishing settles all
of its pending rows in one statement.

## Configuration

Construct the backend with a connection string (`DSN`) in the driver's format,
pointing at the MariaDB server and database. Because the data is shared, the same
details can be handed to workers in other processes or on other machines.

### Managed and cloud MariaDB

Managed MariaDB is less widely offered than MySQL, which is one reason it has its own
backend. The backend works — with no code change, just a different `DSN` — against:

- **AWS** — Amazon RDS for MariaDB.
- **MariaDB SkySQL** — the vendor's own managed cloud service.

Note the gaps: **Azure's managed MariaDB service has been retired**, and **GCP Cloud
SQL does not offer MariaDB**. On those clouds MariaDB must be self-managed (for
example on virtual machines or Kubernetes), and the backend connects to it the same
way.

## Consistency

On a single primary (InnoDB), MariaDB is strongly consistent — read-your-writes, no
stale reads — and that is the recommended setup. A few things to keep in mind in HA
topologies:

- **Asynchronous read replicas can serve stale data.** Route reads to the primary.
- **Asynchronous replication with automatic failover can lose acknowledged writes.**
  Use semi-synchronous replication.
- **Galera Cluster** is virtually synchronous, but a read on another node may not
  have applied the latest write yet. Set `wsrep_sync_wait` to force causal
  (read-your-writes) reads.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs and
workers can share one database without interfering. Parallel tasks within a run each
write their own rows, and the order is sorted out from the timestamps at read time.

## Reference

The backend's API is in the [MariaDB reference](../reference/stores-mariadb.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
