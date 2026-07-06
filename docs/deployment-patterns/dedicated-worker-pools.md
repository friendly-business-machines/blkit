# Dedicated Worker Pools

> Multiple specialised worker binaries, each running only a subset of your
> processes, with the broker routing the right work to the right pool.

!!! warning "Placeholder — pattern in progress"
    The worker package this pattern relies on is still being built. This page is a
    stub so the section's shape is visible; it will be filled in once the worker
    package lands. The intended behaviour lives in the specs under `specs/worker/`
    and `specs/message-brokers/`.

## What this pattern is

An extension of the [Distributed Worker Pool](distributed-worker-pool.md). A
worker's capability set is determined entirely by **which process packages are
compiled into its binary** (via blank imports in `main`). By building several
worker binaries — each importing a different subset of processes — you create
distinct fleets, and the broker's **selective consumption** delivers each job only
to a fleet that can run it. All fleets share one broker and one state store.

## When to use it

- **Resource isolation** — pin GPU-heavy or memory-heavy processes to their own
  machine pool, away from lightweight ones.
- **Blast-radius control** — a bad deploy of one process fleet cannot stall the
  others.
- **Independent scaling and rollout** — scale and release each process family on
  its own schedule.
- **Tenancy or compliance** — run certain processes only on specific,
  appropriately-governed infrastructure.

## How it's wired

- One `main` package per fleet under `cmd/`, each blank-importing only the process
  packages that fleet should run.
- Each binary containerised and deployed as its own group of replicas.
- A single shared [external broker](../message-brokers/overview.md) whose
  selective-consumption routing sends each job to a fleet registered for it, and a
  shared [state store](../state-stores/overview.md).

Because routing follows the binary's registry contents, there is no central router
to configure — deployment shape *is* the routing. This composes with the
[Distributed Worker Pool](distributed-worker-pool.md): the producer tier is
unchanged; only the worker side is partitioned.
