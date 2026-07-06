# Single-Node Server

> One binary that serves clients *and* runs processes — a REST or MCP server with
> an embedded worker. Self-contained, with no separate worker fleet to operate.

!!! warning "Placeholder — pattern in progress"
    The server and worker packages this pattern relies on are still being built.
    This page is a stub so the section's shape is visible; it will be filled in
    once those packages land. The intended behaviour lives in the specs under
    `specs/rest/`, `specs/mcp/`, and `specs/worker/`.

## What this pattern is

A single binary runs a producer — a [REST server](../architecture/worker-pools.md)
(`blkit/restserver`) or an [MCP server](mcp-server.md) (`blkit/mcp`) — with an
**embedded worker** in the same process. The server accepts submissions and streams
events; the embedded worker consumes them and drives each process to completion.
Both share one broker and are governed by one lifecycle: cancelling the server's
context drains the worker too.

## When to use it

- **Small deployments** that want an HTTP (or MCP) API without operating a separate
  worker tier.
- **Getting started** — the shortest path from process definitions to a running
  service.
- Workloads where producer and worker capacity scale **together**, so co-locating
  them is simpler than splitting them.

## How it's wired

- A REST or MCP server started with its `Run(...)` entry point and the
  `EmbeddedWorker` option set, so `worker.Run` spawns inside the same process.
- A [message broker](../message-brokers/overview.md) — the in-memory broker for a
  truly self-contained single node, or an external broker if you plan to add more
  binaries later.
- A durable [state store](../state-stores/overview.md) so runs survive a restart.

Because the embedded worker registers its processes on the broker at startup, the
server advertises exactly the processes compiled into the binary. When you need
producer and worker to scale independently, drop the `EmbeddedWorker` option and
move the workers into their own [worker pool](distributed-worker-pool.md) — the
server code barely changes.
