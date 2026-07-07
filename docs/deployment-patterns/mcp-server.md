# MCP Server for AI Agents

> Exposing your processes as MCP tools so AI agents can discover, submit, and
> observe them — over a stdio Model Context Protocol server.

!!! warning "Placeholder — pattern in progress"
    The MCP server and worker packages this pattern relies on are still being
    built. This page is a stub so the section's shape is visible; it will be
    filled in once those packages land. The intended behaviour lives in the specs
    under `specs/mcp/` and `specs/worker/`.

## What this pattern is

The MCP server (`blkit/mcp`) presents each process registered on the broker as an
**MCP tool**, over the stdio transport an AI agent expects. A tool call becomes a
submit-plus-observe round-trip on the [broker](../message-brokers/overview.md); a
built-in `describe_process` tool lets an agent inspect a process's input shape and
documentation before calling it. Like the REST server, it can embed a worker or
run producer-only against a remote [worker pool](distributed-worker-pool.md).

## When to use it

- Giving **AI agents** a governed, typed way to run business processes instead of
  ad-hoc code.
- **Low-code / assistant integrations** where the agent host speaks MCP.
- Pairing a conversational front end with blkit's validated
  [data contracts](../decisions/overview.md) so inputs are checked before a run
  starts.

## How it's wired

- An MCP server started with its `Run(...)` entry point on the stdio transport.
- Either the `EmbeddedWorker` option for a self-contained single-node agent
  tool, or producer-only against a
  shared broker and a remote [worker pool](distributed-worker-pool.md).
- A [message broker](../message-brokers/overview.md) and
  [state store](../state-stores/overview.md) as in the other patterns.

The transport is stdio only; HTTP-based MCP transports are out of scope. For a
browser- or service-facing HTTP API instead, use the REST server in the
[Single-Node REST Server](single-node-rest-server.md) or
[Distributed Worker Pool](distributed-worker-pool.md) patterns.
