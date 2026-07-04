# State Stores

> Where blkit keeps the state of a running process — pick the backend that fits
> your deployment, from a zero-dependency in-memory store to PostgreSQL, NATS, or
> an embedded file.

blkit workers are **stateless**: everything they know about a run they load from a
**state store** and write straight back to it. The state store is the source of
truth for a [process](../processes/processes.md) instance — the values its tasks
produce and the execution history of every step — so a run survives a worker
restart and can be picked up by another worker.

blkit ships one built-in store and a family of pluggable backends. They are
**interchangeable**: your process code does not change when you swap one for
another, because every backend implements the same interface and is held to the
same behaviour by a [shared conformance suite](#conformance).

## What every state store does

For each run of a process, a state store must:

- **Create** a fresh run when it starts — its id and its start values.
- **Save** changes as the run progresses — the values tasks write and the
  execution-history entries — so nothing is lost if a worker stops.
- **Load** the current state for a run, so a worker can see where it got to and
  carry it on.
- **Read back** the full history — every value ever written and the whole
  execution history — for diagnostics and audit.

Two rules hold across every backend:

- **A task's outputs become visible all at once, when it finishes.** While a task
  runs, its writes are held pending; they appear in the current state together the
  moment the task completes, and are discarded from the current view if it fails.
- **Nothing is ever deleted.** Failed attempts stay in the full history — the
  record is kept for diagnostics and audit — even though they never show up in the
  current state.

## Choosing a backend

You pick a backend by constructing it and handing it to blkit. The built-in
in-memory store needs nothing extra:

```go
var store = bl.NewInMemoryStateStore()
```

Every other backend lives in its own module, so you only pull in its driver if you
use it. Import it and construct it — the rest of your code is identical:

```go
import (
    "os"

    bl      "github.com/friendly-business-machines/blkit"
    pgstore "github.com/friendly-business-machines/blkit/stores/postgres"
)

var store = pgstore.New(pgstore.Config{DSN: os.Getenv("DATABASE_URL")})
```

## The backends

The backends fall into three families. Which fits depends on whether you need
durability, and whether runs must be shared across more than one machine.

**Not durable — built into core:**

| Backend | Use it for |
|---|---|
| [In-memory](in-memory.md) | Tests, examples, and local runs. Zero dependencies; lost when the program exits. |

**Durable and shared across machines — a server you run separately:**

| Backend | Use it for |
|---|---|
| [PostgreSQL](postgres.md) | The default for production: durable, strongly consistent, shareable. |
| [SQL Server](mssql.md) | Shops standardised on Microsoft SQL Server. |
| [MySQL](mysql.md) | Shops running MySQL. |
| [MariaDB](mariadb.md) | Shops running MariaDB — a separate backend from MySQL. |
| [NATS](nats.md) | When NATS is already your message broker: store state in the same system. |

**Durable but local to one machine — embedded, no server to run:**

| Backend | Use it for |
|---|---|
| [SQLite](sqlite.md) | The most broadly useful embedded default — a single file whose history is queryable with SQL. |
| [Turso](turso.md) | The ground-up rewrite of SQLite in Rust (beta) — in-process, SQLite-compatible file format. |
| [bbolt](bbolt.md) | The simplest durable key-value option; a single file tuned for reads. |
| [Badger](badger.md) | A directory of files tuned for heavy writes. |
| [Pebble](pebble.md) | A directory of files with balanced read-and-write performance. |

## Conformance

Every backend is verified against a **shared conformance suite** that lives in core,
so they all behave identically — a value written through any of them is read back
the same way, and the strong-consistency guarantee is checked for each. The suite
covers the full behaviour: pending writes stay invisible until a task finishes,
aborted writes are retained in the full history, the latest committed value wins
per field, replay ordering is stable, and reads see prior writes after a flush.

How it runs depends on the backend: the in-memory and embedded backends run it
in-process with no setup; NATS runs it against a real JetStream server embedded in
the test; and the SQL-server backends run it against a real server whose address
comes from an environment variable, skipping when it is unset.

## Reference

The `StateStore` interface and the built-in in-memory store are in the core API
[Reference](../reference/blkit.md); each pluggable backend has its own reference
page (for example, [PostgreSQL](../reference/stores-postgres.md)).
