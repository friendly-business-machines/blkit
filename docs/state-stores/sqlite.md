# SQLite

> A durable, embedded state store kept in a single SQLite file — the most broadly
> useful embedded default, and the only one whose history is queryable with SQL.

The SQLite backend keeps each run's state in a SQLite database — a single file on
local disk. It is **embedded**: there is no separate server to run, just a file the
program opens directly. Data is **durable** — it survives a restart.

Among the embedded backends it has a unique advantage: the data is stored as
ordinary SQL rows, so the full history can be **queried directly with SQL** for
diagnostics and audit — the same benefit [PostgreSQL](postgres.md) offers, without
running a server.

It lives in its own module, so its driver is only pulled in by applications that
use it:

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    sqlitestore "github.com/friendly-business-machines/blkit/stores/sqlite"
)

var store = sqlitestore.New(sqlitestore.Config{Path: "/var/lib/blkit/state.db"})
```

The backend is built on the pure-Go [modernc.org/sqlite](https://pkg.go.dev/modernc.org/sqlite)
driver — chosen over the CGO-based driver so worker binaries can be built with
`CGO_ENABLED=0` for static, distroless containers. It runs in WAL mode so readers are
not blocked by writes, funnels writes through a single writer connection, and sets a
busy timeout so brief contention waits rather than fails.

## What it's good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Queryable audit without a server** — the run history is ordinary SQL rows;
  inspect it with any SQLite client (`sqlite3 state.db`).
- **Simple operations** — the whole state is one file to back up or move.

## Storage layout

The backend uses the same three tables as the
[PostgreSQL layout](postgres.md#storage-layout) — `blkit_runs`, `blkit_values`,
`blkit_history` — with the same columns and behaviour, differing only in dialect:
SQLite's rowid for the arrival-order id, `INTEGER` Unix-nanosecond timestamps, `TEXT`
JSON for encoded values (queryable with SQLite's JSON functions), a partial index for
the pending lookup, and a `ROW_NUMBER()` window function for the
latest-committed-per-field read. A batch of writes executes as a single transaction,
and a task finishing settles all of its pending rows in one statement.

## Configuration

Construct the backend with the **path to the database file**. There is no server
address and no credentials — the file is opened directly by the program.

## Consistency

SQLite is strongly consistent: a single local file with ACID transactions, where a
committed write is always visible to the next read. There is no replication, so the
stale-read concerns that apply to server-based backends in HA setups do not arise.

## What to keep in mind

- **It is local to one machine.** The file lives on that machine's disk, so runs
  cannot be shared with workers elsewhere. For shared runs, use a server-based
  backend such as [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One worker process per file.** SQLite permits one writer at a time; the backend
  is designed for a single worker process owning the file.

Compared with the other embedded backends ([bbolt](bbolt.md), [Badger](badger.md),
[Pebble](pebble.md)), the key-value stores are faster at raw writes, but SQLite is
the only one whose history you can query with SQL. When in doubt among the embedded
options, SQLite is the most broadly useful default.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs handled
by the one worker process do not interfere. Parallel tasks within a run each write
their own rows; writes are funnelled through the single writer connection, and WAL
mode keeps reads flowing while that happens.

## Reference

The backend's API is in the [SQLite reference](../reference/stores-sqlite.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
