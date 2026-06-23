# Worker Pools

> How blkit runs processes to completion — pools of workers that pull ready
> tasks, persist state, and handle retries.

!!! warning "Placeholder — chapter in progress"
    The worker-pool subsystem is still being built. This page is a stub so the
    Architecture section's shape is visible and stable; it will be filled in
    once the process and worker packages land. For the subsystem that is
    documented today, see [Expressions](expressions.md).

## What this chapter will cover

Where the [Expressions](expressions.md) chapter explains the language a single
rule is written in, this chapter will explain how blkit executes whole
**processes** — long-running graphs of tasks — by handing their ready work to a
pool of **workers**.

Planned topics:

- **The execution model** — how a process graph is turned into a stream of ready
  tasks, and how workers pull and execute them.
- **Concurrency** — how the pool is sized and scheduled, and how independent
  branches of a process run in parallel.
- **State and durability** — where process state lives between steps, so a
  process can be paused, resumed, and survive a restart.
- **Failure handling** — retries, back-off, and how a failed task affects the
  rest of the process.

Until then, the authoritative description of the intended behaviour lives in the
worker spec under `specs/worker/`.
