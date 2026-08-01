---
name: SqliteStateStore
description: A durable, embedded state-store backend that keeps each run's ProcessState in a single SQLite file on local disk — its own module built on the pure-Go modernc.org/sqlite driver, so audit stays queryable with SQL and static builds keep working
status: implemented
code:
  - stores/sqlite/
implements: specs/state-stores/overview.spec.md
---

# SqliteStateStore

The SQLite backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a **SQLite** database — a
single file on local disk. It is **embedded**: there is no separate server to run,
just a file the program opens directly. Data is durable — it survives a restart.

Among the embedded backends it has a unique advantage: the data is stored as
ordinary SQL rows, so the full history can be **queried directly with SQL** for
diagnostics and audit — the same benefit the [PostgreSQL](./postgres-state-store.spec.md)
backend offers, without running a server.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/sqlite`,
so its dependency is only pulled in by applications that actually use it.

```go
import (
    bl          "github.com/friendly-business-machines/blkit"
    sqlitestore "github.com/friendly-business-machines/blkit/stores/sqlite"
)

var store = sqlitestore.New(sqlitestore.Config{Path: "/var/lib/blkit/state.db"})
```

---

## Implementation

The backend is built on **`modernc.org/sqlite`** — the pure-Go SQLite driver — used
through Go's `database/sql` package. The pure-Go driver is chosen deliberately over
the CGO-based `mattn/go-sqlite3`: worker binaries are built with `CGO_ENABLED=0` for
static, distroless containers (see the Dockerfile in
[worker.spec.md](../worker/worker.spec.md#example-containerization--deployment)),
and a CGO driver would break that build. The pure-Go driver is somewhat slower, but
a state store's writes are small and batched, so driver speed is not the bottleneck.

SQLite-specific choices the backend makes:

- **WAL mode** (`journal_mode=WAL`) — write-ahead logging, so readers are not
  blocked while a write is in progress.
- **`busy_timeout`** set on the connection, so a write that briefly contends with
  another simply waits instead of failing.
- **One writer connection** — SQLite allows one writer at a time, so the backend
  funnels writes through a single connection and lets `database/sql` pool readers.
- Writes for one task are applied inside a single transaction, so a task's outputs
  land together (matching the "all at once" write described in
  [process-state.spec.md](../processes/process-state.spec.md#writing-a-tasks-outputs-when-it-finishes)).

---

## Storage layout

The same three tables as the
[PostgreSQL layout](./postgres-state-store.spec.md#storage-layout) — `blkit_runs`,
`blkit_values`, `blkit_history` — with the same columns, write-op mapping, and read
queries, differing only in dialect:

- `INTEGER PRIMARY KEY` (SQLite's rowid) supplies the arrival-order id.
- Timestamps are stored as **`INTEGER` Unix nanoseconds** (SQLite has no native
  datetime type); replay sorts on `(ts, id)` as everywhere else.
- Encoded `Bl` values are stored as **`TEXT` JSON**, queryable with SQLite's built-in
  JSON functions (`json_extract` etc.).
- The pending lookup uses a **partial index**
  (`CREATE INDEX ... WHERE status = 'pending'`), which SQLite supports directly.
- The current-version query uses `ROW_NUMBER() OVER (PARTITION BY task_id, field
  ORDER BY ts DESC, id DESC)` in place of `DISTINCT ON`.

A `WriteBatch` executes as a single transaction on the writer connection; the
`StatusFlip` update settles all of a task's pending rows in one statement, exactly
as in the PostgreSQL backend.

---

## Configuration

The backend is constructed with the **path to the database file**. There is no
server address and no credentials — the file is opened directly by the program.

---

## Consistency

SQLite is **strongly consistent**: it is a single local file with ACID transactions,
and a committed write is always visible to the next read. There is no replication
and therefore no stale-read risk — the concerns that apply to server-based backends
in HA setups do not arise here.

---

## What it is good for

- **Single-node deployments** that want durability without running a separate
  database server.
- **Queryable audit without a server** — the run history is ordinary SQL rows;
  inspect it with any SQLite client (`sqlite3 state.db`), no infrastructure needed.
- **Simple operations** — the whole state is one file to back up or move.

---

## What to keep in mind

- **It is local to one machine.** The file lives on that machine's disk, so runs
  cannot be shared with workers on other machines. For runs shared across machines,
  use a server-based backend such as [PostgreSQL](./postgres-state-store.spec.md) or
  [NATS](./nats-state-store.spec.md).
- **One worker process per file.** SQLite permits one writer at a time; the backend
  is designed for a single worker process owning the file, not several processes
  sharing it.

Compared with the other embedded backends
([bbolt](./bbolt-state-store.spec.md), [Badger](./badger-state-store.spec.md),
[Pebble](./pebble-state-store.spec.md)): the key-value stores are faster at raw
writes, but SQLite is the only one whose history you can query with SQL. When in
doubt among the embedded options, SQLite is the most broadly useful default.

---

## Concurrency

- **Different runs** are independent rows keyed by different run ids, so many runs
  handled by the one worker process do not interfere.
- **Parallel tasks within one run** each write their own rows. Writes are funnelled
  through the single writer connection and applied one after another; WAL mode keeps
  reads flowing while that happens.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a store
opened in a **temporary directory** that is removed when the test finishes, so it
needs no external system and runs as part of the module's normal `go test` run.
Reopening the store mid-suite verifies the data survives a close/open cycle.

Verified by [`store_test.go`](../../stores/sqlite/store_test.go).
