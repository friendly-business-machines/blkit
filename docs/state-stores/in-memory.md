# In-memory

> The built-in state store — zero dependencies, not durable — the default for
> tests, examples, and local single-process runs.

The in-memory store keeps each run's state in ordinary in-memory data structures.
It is **built into core**, so it needs nothing extra to import, which makes it the
default choice for tests, examples, and local single-process runs.

```go
var store = bl.NewInMemoryStateStore()
```

## What it's good for

- **Tests** — fast, with no setup and nothing to clean up.
- **Examples and local development** — run a process end to end with no database.
- **Short-lived, single-process runs** where losing the data when the program
  exits is acceptable.

Because there is no external storage, saving is immediate — there is nothing to
send over a network and nothing to wait for.

## What it's not for

- **It is not durable.** Everything is lost when the program exits; an unfinished
  run cannot be recovered afterwards.
- **It cannot be shared.** The data lives inside one running program's memory, so
  it cannot be handed to a worker in another process or on another machine. Asking
  it for connection details is an error — there is nothing to connect to.

For durable or shared runs, use a backend such as [PostgreSQL](postgres.md) or
[NATS](nats.md).

## Concurrency

Within a single run, several tasks can run in parallel and write at once. The
in-memory store is safe to use from those parallel tasks — its data structures are
guarded so concurrent writes within a run do not corrupt each other.

## Reference

`NewInMemoryStateStore` and the `StateStore` interface it implements are in the
core API [Reference](../reference/blkit.md).
