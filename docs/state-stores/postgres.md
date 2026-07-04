# PostgreSQL

> A durable, strongly consistent, shareable state store backed by PostgreSQL — a
> good default for production.

The PostgreSQL backend keeps each run's state in a PostgreSQL database. It is
**durable** (the data survives restarts), **strongly consistent** (a write is
always visible to the next read), and **shareable** (workers on different machines
can all reach the same database), which makes it a good default for production.

It lives in its own module, so the database driver it needs is only pulled in by
applications that use it:

```go
import (
    "os"

    bl      "github.com/friendly-business-machines/blkit"
    pgstore "github.com/friendly-business-machines/blkit/stores/postgres"
)

var store = pgstore.New(pgstore.Config{DSN: os.Getenv("DATABASE_URL")})
```

The backend is built on [pgx](https://github.com/jackc/pgx), the actively
maintained PostgreSQL driver for Go: a whole batch of writes is sent as one
pipelined round-trip inside a single transaction, and encoded values are stored as
`JSONB` so the audit history is queryable with SQL down into the values.

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Runs shared across many workers**, on one machine or many.
- **Auditing and reporting** — because the history is ordinary database rows, it
  can be queried directly with SQL.

## Storage layout

On first use the backend creates three tables (their names carry a configurable
prefix, `blkit_` by default):

- **`blkit_runs`** — one row per run: its id, process id and version, status, and
  timestamps.
- **`blkit_values`** — one row per value a task writes, each with a `pending` /
  `committed` / `aborted` status. A task finishing flips all of its pending rows to
  committed in a single statement; a failure flips them to aborted. The current
  state reads the latest committed row per field; the full history returns every
  row, with its status.
- **`blkit_history`** — one row per execution-history entry.

Both event tables carry a timestamp and an auto-increment id, and reads sort on
`(timestamp, id)`, so the order events arrived in does not matter.

## Configuration

Construct the backend with a connection string (`DSN`) pointing at the PostgreSQL
server and database. Because the data is in a shared database, the same connection
details can be handed to workers in other processes or on other machines, so they
can all work on the same runs.

### Managed and cloud PostgreSQL

This backend speaks to any PostgreSQL-compatible server, so it also works — with no
code change, just a different `DSN` — against the managed PostgreSQL services on the
major clouds:

- **AWS** — Amazon RDS for PostgreSQL, and Amazon Aurora PostgreSQL-Compatible Edition.
- **GCP** — Cloud SQL for PostgreSQL, and AlloyDB for PostgreSQL.
- **Azure** — Azure Database for PostgreSQL.

There is no separate backend for these — point the `DSN` at the managed endpoint.
Use a primary (read-write) endpoint; reading from an asynchronous read replica can
return stale data.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs — and
many workers — can share one database without interfering. Parallel tasks within a
run each write their own rows, and the order is sorted out from the timestamps at
read time.

## Reference

The backend's API is in the [PostgreSQL reference](../reference/stores-postgres.md);
the `StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
