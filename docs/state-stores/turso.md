# Turso

> A durable, embedded state store kept in a Turso Database file — Turso Database
> is the ground-up rewrite of SQLite in Rust, in-process and compatible with
> SQLite's query language and file format. Beta engine.

The Turso backend keeps each run's state in a **Turso Database** file on local
disk. Turso Database is the ground-up **rewrite of SQLite in Rust** (formerly
"Limbo"): an **in-process, embedded** engine — no server to run, just a file the
program opens directly — compatible with SQLite in both query language and file
format. Data is **durable** — it survives a restart.

!!! warning "Beta engine"

    Turso Database is in **beta**. blkit's conformance suite passes against it,
    but the vendor's own guidance applies: use caution with production data and
    keep backups. For a battle-tested embedded SQL store, use the
    [SQLite backend](sqlite.md); choose this backend to run on the Rust engine.

A naming note: this backend is built on **Turso Database, the Rust engine** — not
on libSQL, Turso's earlier C fork of SQLite that powers their managed cloud
service.

It lives in its own module, so its dependency is only pulled in by applications
that use it:

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    tursostore "github.com/friendly-business-machines/blkit/stores/turso"
)

var store = tursostore.New(tursostore.Config{Path: "/var/lib/blkit/state.db"})
```

The backend is built on Turso's official Go bindings
([turso.tech/database/tursogo](https://pkg.go.dev/turso.tech/database/tursogo)),
which call the Rust core through purego with a bundled platform library — **no
CGO at build time**, so `CGO_ENABLED=0` builds keep working. One runtime caveat
follows: the bundled library is extracted and loaded dynamically, so a container
image needs a libc-based dynamic loader (for example `distroless/base`); a fully
static `scratch` image cannot load it.

## What it's good for

- **Running blkit state on the Rust engine** — for teams adopting Turso
  Database, with the same operational shape as the SQLite backend.
- **Single-node deployments** that want durability without running a separate
  database server.
- **SQLite-compatible file format** — the database file follows SQLite's format,
  keeping the state inspectable with familiar tooling.

## Storage layout

The backend uses the same three tables as the
[PostgreSQL layout](postgres.md#storage-layout) — `blkit_runs`, `blkit_values`,
`blkit_history` — in SQLite's dialect, as in the
[SQLite backend](sqlite.md#storage-layout): rowid for the arrival-order id,
`INTEGER` Unix-nanosecond timestamps, and `TEXT` JSON for encoded values. To stay
well inside the beta engine's supported SQL surface, the backend keeps its SQL
deliberately conservative: plain composite indexes rather than partial indexes,
and the latest-committed-per-field read folds in Go over an ordered scan rather
than using a window function. A batch of writes executes as a single
transaction, and a task finishing settles all of its pending rows in one
statement.

## Configuration

Construct the backend with the **path to the database file**. There is no server
address and no credentials — the file is opened directly by the program.

## Consistency

Turso Database is an embedded engine with transactions on a single local file: a
committed write is always visible to the next read. There is no replication, so
the stale-read concerns that apply to server-based backends in HA setups do not
arise.

## What to keep in mind

- **Beta engine** — see the warning above.
- **It is local to one machine.** The file lives on that machine's disk, so runs
  cannot be shared with workers elsewhere. For shared runs, use a server-based
  backend such as [PostgreSQL](postgres.md) or [NATS](nats.md).
- **One worker process per file**, with all access serialised through a single
  connection.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs
handled by the one worker process do not interfere. Parallel tasks within a run
each write their own rows; all access is funnelled through the single connection
and applied one statement after another.

## Reference

The backend's API is in the [Turso reference](../reference/stores-turso.md); the
`StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
