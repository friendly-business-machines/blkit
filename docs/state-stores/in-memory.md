# In-memory

> The built-in state store — zero dependencies, not durable — the default for
> tests, examples, and local single-process runs.

The in-memory store keeps each run's state in ordinary Go data structures held in
the process's memory. It is **built into core**, so there is nothing extra to
import and no server to run, which makes it the default choice for tests, examples,
and local single-process runs.

Construct it with no arguments — there is nothing to configure:

```go
import bl "github.com/friendly-business-machines/blkit"

// Zero dependencies, no connection, no options. The rest of blkit uses the
// returned *InMemoryStateStore the same way as any other StateStore.
store := bl.NewInMemoryStateStore()
```

Despite living entirely in memory, it is **not** a simplified stand-in: it is held
to the same [conformance suite](overview.md#conformance) as PostgreSQL, NATS, and
every other backend, so it observes the same visibility, ordering, and read
semantics. Code that works against the in-memory store behaves the same way when
you later point it at a durable backend.

## What it's good for

- **Tests** — fast, with no setup and nothing to clean up.
- **Examples and local development** — run a process end to end with no database.
- **Short-lived, single-process runs** where losing the data when the program
  exits is acceptable.

Because there is no external storage, saving is immediate — there is nothing to
send over a network and nothing to wait for.

## Data model

Everything is held in a single `map[string]*inMemoryRun` from run id to a per-run
record, guarded by a `sync.RWMutex`. Each run's record holds exactly what the other
backends persist, only in memory:

| Field | Holds |
|---|---|
| metadata | The run's `RunMetadata`, replaced by `Save`. |
| values | An **append-only slice** of value records (`{task_id, execution_id, field, value, status, ts}`). |
| history | An **append-only slice** of execution-history records. |
| seq | A per-run counter stamped onto each appended record as the arrival-order tiebreak. |

As with every backend, blkit never overwrites a field — each write is a new entry,
and the current value of a field is derived from the entries rather than stored in
place:

- **A write** (`ValueWrite`) appends a record with `status: pending` and the next
  `seq`. While a task runs, its outputs sit in the slice as pending records and are
  invisible to the current-state read.
- **Settling** (`StatusFlip`) walks the run's value slice and flips the finishing
  task's pending records in place — to `committed` when the task succeeds, to
  `aborted` when it fails. Aborted records are kept, not removed, so a failed
  attempt stays visible in the history.
- **History** (`HistoryEntry`) appends a record to the history slice with the next
  `seq`.

A whole `WriteBatch` is applied synchronously **under the write lock**, so it lands
atomically with respect to any reader: a task's outputs never appear half-written.
That synchronous, in-memory apply is also why **`Flush` is a no-op** — there is no
buffer or network round-trip left to complete once `WriteBatch` returns.

Two reads sit on top of the slices, both taking the read lock:

- **Current state** folds the value slice, in replay order, down to the latest
  `committed` record per `(task_id, field)`; pending and aborted records are
  skipped.
- **Full history** returns copies of both slices — every record, including pending
  and aborted ones — sorted into replay order by `(Timestamp, seq)`, so equal
  timestamps fall back to append order and parallel tasks' writes stay stable.

Value payloads are copied in on write and out on read, so a caller can never mutate
stored state by holding onto a slice.

## Configuration

There is nothing to configure. `NewInMemoryStateStore` takes no arguments — no
connection string, no bucket, no options — because there is no external system to
point at. This is the one backend whose `Config()` returns an **error**: in-memory
state cannot be reached from another process, so there are no connection details to
hand out. `Close()` exists only to satisfy the interface and releases nothing.

## Consistency

The store is strongly consistent within the one process that owns it: writes are
applied under a lock and are immediately visible to the next read on any goroutine.
There is nothing to be inconsistent *with* — the data lives in a single process's
memory and is never replicated or shared.

## What to keep in mind

- **It is not durable.** Everything is lost when the program exits; an unfinished
  run cannot be recovered afterwards. Use it only where losing the data on exit is
  acceptable.
- **It cannot be shared.** The data lives inside one running program's memory, so
  it cannot be handed to a worker in another process or on another machine. Asking
  it for connection details is an error — there is nothing to connect to.
- **Memory is the only bound.** Nothing is ever deleted — pending, committed, and
  aborted records all accumulate — so a very long-lived or high-volume process will
  grow the heap without limit.

For durable or shared runs, use a server-based backend such as
[PostgreSQL](postgres.md) or [NATS](nats.md).

## Concurrency

Within a single run, several tasks can run in parallel and write at once. The store
is safe to use from those parallel tasks — the `sync.RWMutex` guards every read and
write, so concurrent writes within a run do not corrupt each other, and the record
each write appends carries its own `seq` so replay order stays well-defined
regardless of which goroutine won the lock first. Different runs are independent
map entries and never contend beyond that shared lock.

## Reference

`NewInMemoryStateStore` and the `StateStore` interface it implements are in the
core API [Reference](../reference/blkit.md).
