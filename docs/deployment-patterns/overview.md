# Overview

> How to assemble blkit into a running application — from a single self-contained
> binary to a distributed producer-and-worker-fleet — by combining the same four
> building blocks in different shapes.

!!! warning "Status — subsystem in progress"
    The server and worker packages these patterns describe are still being built.
    These pages sketch the intended deployment shapes so the section is visible
    and stable. The authoritative requirements live in the specs under
    `specs/worker/`, `specs/rest/`, and `specs/mcp/`.

Every blkit deployment is built from the same four pieces. A pattern is just a
particular way of arranging them across one or more binaries.

- **A producer** — where work comes *from*. A [REST server](single-node-rest-server.md),
  an [MCP server](mcp-server.md), a CLI, or an admin UI submits process runs and
  observes their events.
- **A worker** — where processes actually *run*. A worker fetches jobs, drives
  each process to completion, and is the only component that touches the state
  store. See [worker pools](../architecture/worker-pools.md).
- **A [message broker](../message-brokers/overview.md)** — the channel between
  producers and workers. In-memory for a single process; an external broker
  (Redis, NATS, …) across machines.
- **A [state store](../state-stores/overview.md)** — where each run's state
  lives, owned exclusively by workers.

## The axes of choice

Picking a pattern comes down to three questions:

- **One binary or many?** Co-locate everything for simplicity, or split the
  producer from the workers so each scales independently.
- **Embedded or remote worker?** Both the REST and MCP servers can embed a worker
  in the same process, or run producer-only and rely on a separate worker fleet.
- **In-memory or external broker?** The built-in in-memory broker needs no infra
  but stays within one process; an external broker is what lets separate binaries
  and machines talk.

## The patterns

Ordered from the simplest, single-process shape to the most distributed. Later
patterns build on the vocabulary of the earlier ones.

| Pattern | Binaries | Broker | Use it for |
|---|---|---|---|
| [Embedded Library](embedded-library.md) | One | In-memory | blkit inside your app or CLI — no server, no external infra. |
| [Single-Node REST Server](single-node-rest-server.md) | One | In-memory / local | A self-contained REST server with an embedded worker. |
| [Distributed Worker Pool](distributed-worker-pool.md) | Many | External | A producer plus a horizontally-scaled worker fleet over a shared broker. |
| [Dedicated Worker Pools](dedicated-worker-pools.md) | Many | External | Specialised worker binaries, each running a subset of processes. |
| [MCP Server for AI Agents](mcp-server.md) | One or many | Any | Exposing processes as MCP tools for AI agents over stdio. |

## Choosing infrastructure

Once you have picked a shape, the [Message Brokers](../message-brokers/overview.md)
and [State Stores](../state-stores/overview.md) sections cover the backend choices
that fill in the broker and state-store slots — they are interchangeable, so a
pattern does not dictate which you use.
