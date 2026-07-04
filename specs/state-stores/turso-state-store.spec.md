---
name: TursoStateStore
description: A durable, embedded state-store backend that keeps each run's ProcessState in a Turso Database file on local disk — Turso Database is the ground-up rewrite of SQLite in Rust (beta); its own module, pure-Go driver, no CGO at build time
targets:
  - ../../stores/turso/store.go
---

# TursoStateStore

> **Status:** Work in progress. See
> [overview.spec.md](./overview.spec.md) for how backends are laid out, and
> [process-state.spec.md](../processes/process-state.spec.md) for what a
> `ProcessState` is.

The Turso backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **Turso Database** file on
local disk. Turso Database is the ground-up **rewrite of SQLite in Rust**
(formerly "Limbo"): an **in-process, embedded** engine — no server to run, just a
file the program opens directly — compatible with SQLite in both **query
language and file format**. Data is durable — it survives a restart.

> **Beta engine.** Turso Database is in **BETA**. The conformance suite passes
> against it (see [Testing](#testing)), but the vendor's own guidance applies:
> use caution with production data and keep backups. For a battle-tested
> embedded SQL store, use the [SQLite backend](./sqlite-state-store.spec.md);
> choose this backend to run on the Rust engine.

Naming note: this backend is built on **Turso Database, the Rust engine** — not
on libSQL (Turso's earlier C fork of SQLite that powers their managed cloud
service). It is an embedded backend in the same family as
[SQLite](./sqlite-state-store.spec.md), [bbolt](./bbolt-state-store.spec.md),
[Badger](./badger-state-store.spec.md), and [Pebble](./pebble-state-store.spec.md).

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/turso`,
so its dependency is only pulled in by applications that actually use it.

```go
import (
    bl         "github.com/friendly-business-machines/blkit"
    tursostore "github.com/friendly-business-machines/blkit/stores/turso"
)

var store = tursostore.New(tursostore.Config{Path: "/var/lib/blkit/state.db"})
```

---

## Implementation

The backend is built on **`turso.tech/database/tursogo`** — Turso's official Go
bindings (developed in the `tursodatabase/turso` repository) — registered with
`database/sql` as the `"turso"` driver.

- **No CGO at build time.** The bindings call the Rust core through
  [purego](https://github.com/ebitengine/purego) and a **bundled platform
  library** that is extracted to a temporary directory and loaded at runtime.
  `CGO_ENABLED=0` builds keep working.
- **Runtime caveat that follows from that:** loading the extracted library
  needs a dynamic loader. A container image must have a libc-based dynamic
  loader available (e.g. `distroless/base`); a fully static `scratch` /
  `distroless/static` image cannot load it.
- **One connection** (`SetMaxOpenConns(1)`) — all access is serialised through
  a single connection, keeping the beta engine's concurrency surface out of
  play.
- **Conservative SQL.** The backend deliberately stays inside the beta
  engine's well-supported SQLite surface:
  - the metadata upsert is UPDATE-then-INSERT inside a transaction, not
    `ON CONFLICT`;
  - indexes are plain composite indexes, no partial indexes;
  - the latest-committed-per-field read folds in Go over a replay-ordered
    scan, no window functions.
- Writes for one task are applied inside a single transaction, so a task's
  outputs land together.

## Storage layout

The same three tables as the
[PostgreSQL layout](./postgres-state-store.spec.md#storage-layout) —
`blkit_runs`, `blkit_values`, `blkit_history` — in SQLite's dialect, as in the
[SQLite backend](./sqlite-state-store.spec.md#storage-layout): rowid
(`INTEGER PRIMARY KEY`) for the arrival-order id, `INTEGER` Unix-nanosecond
timestamps, and `TEXT` JSON values. The two deliberate differences (per the
conservative-SQL notes above): the pending lookup uses a plain composite index
on `(run_id, task_id, status)` instead of a partial index, and the
current-version read is a Go-side fold instead of a `ROW_NUMBER()` window.

A `WriteBatch` executes as a single transaction; the `StatusFlip` update
settles all of a task's pending rows in one statement.

---

## Configuration

The backend is constructed with the **path to the database file** (and an
optional `TablePrefix`, defaulting to `blkit_`). There is no server address and
no credentials — the file is opened directly by the program.

---

## Consistency

Turso Database is an embedded engine with transactions on a single local file:
a committed write is always visible to the next read. There is no replication,
so the stale-read concerns that apply to server-based backends in HA setups do
not arise. (The driver's remote-sync features are not used by this backend.)

---

## What it is good for

- **Running blkit state on the Rust engine** — for teams adopting Turso
  Database, with the same operational shape as the SQLite backend.
- **Single-node deployments** that want durability without running a separate
  database server.
- **SQLite-compatible file format** — the database file follows SQLite's
  format, keeping the state inspectable with familiar tooling.

---

## What to keep in mind

- **Beta engine** — see the banner above. The SQLite backend is the
  conservative choice; this one is for the Rust engine.
- **It is local to one machine.** The file lives on that machine's disk, so
  runs cannot be shared with workers on other machines. For runs shared across
  machines, use a server-based backend such as
  [PostgreSQL](./postgres-state-store.spec.md) or [NATS](./nats-state-store.spec.md).
- **One worker process per file**, with all access serialised through a single
  connection.
- **Container images need a dynamic loader** for the bundled runtime library
  (see [Implementation](#implementation)).

---

## Concurrency

- **Different runs** are independent rows keyed by different run ids, so many
  runs handled by the one worker process do not interfere.
- **Parallel tasks within one run** each write their own rows. All access is
  funnelled through the single connection and applied one statement after
  another.

---

## Testing

This backend is verified against the shared state-store **conformance suite**
(see [overview.spec.md](./overview.spec.md#testing)). The suite runs against
the **real Turso Database engine, in-process**, on a database file in a
temporary directory that is removed when the test finishes — no external
system, part of the module's normal `go test` run. Reopening the store
mid-suite verifies the data survives a close/open cycle.

`[@test] ../../stores/turso/store_test.go`
