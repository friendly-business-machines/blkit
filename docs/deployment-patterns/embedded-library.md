# Embedded Library

> blkit running entirely in-process — no server, no external infrastructure. The
> simplest way to execute processes: inside your own application or a CLI, over
> the built-in in-memory broker.

!!! warning "Placeholder — pattern in progress"
    The worker package this pattern relies on is still being built. This page is a
    stub so the section's shape is visible; it will be filled in once the worker
    package lands. The intended behaviour lives in the spec under `specs/worker/`.

## What this pattern is

In the embedded pattern there is no producer/worker split and no network. Your
application constructs a [`NewInMemoryMessageBroker()`](../message-brokers/in-memory.md)
and a [state store](../state-stores/overview.md), runs a worker in a goroutine,
and submits runs directly through the broker — all in one process. For the very
simplest cases you can skip the broker entirely and call `process.Evaluate(...)`
in-line.

## When to use it

- **Unit and integration tests** that exercise real processes without standing up
  infrastructure.
- **CLI tools** and batch jobs that run a process and exit.
- **Single-node applications** embedding business logic with no need to scale
  workers separately.

## How it's wired

- The built-in [in-memory broker](../message-brokers/in-memory.md) — zero
  dependencies, single process.
- A [state store](../state-stores/overview.md) — the in-memory store for
  throwaway runs, or an embedded durable store (SQLite, bbolt) to survive a
  restart.
- A worker loop in a goroutine, sharing the same broker value the producer code
  submits through.

Nothing here crosses a process boundary, so nothing survives the process exiting
unless you pick a durable state store. When you outgrow a single process, move to
the [Single-Node REST Server](single-node-rest-server.md) or
[Distributed Worker Pool](distributed-worker-pool.md) patterns — your process code
does not change.
