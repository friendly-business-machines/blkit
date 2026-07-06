# Distributed Worker Pool

> A producer and a separately-scaled worker fleet, talking over a shared external
> broker. The standard shape for production: submit from one tier, execute on
> another, scale each independently.

!!! warning "Placeholder — pattern in progress"
    The server and worker packages this pattern relies on are still being built.
    This page is a stub so the section's shape is visible; it will be filled in
    once those packages land. The intended behaviour lives in the specs under
    `specs/rest/`, `specs/worker/`, and `specs/message-brokers/`.

## What this pattern is

The producer and the workers run as **separate binaries**. A broker-only
[REST server](../architecture/worker-pools.md) (or MCP server) accepts submissions
and streams events but runs no processes itself; a pool of standalone
[workers](../architecture/worker-pools.md) consumes the shared
[external broker](../message-brokers/overview.md) and executes the work. Any number
of worker replicas can run — each is an independent consumer of the broker's job
queue.

## When to use it

- **Production deployments** that need to scale execution capacity independently of
  the request-serving tier.
- Workloads where processes are **long-running** or resource-heavy, so workers want
  their own machines and lifecycle.
- Systems that need **rolling worker restarts** without dropping in-flight runs —
  a job whose worker dies is redelivered to another.

## How it's wired

- A producer binary running the REST or MCP server **without** an embedded worker.
- One or more worker binaries running `worker.Run` against the same broker and a
  shared [state store](../state-stores/overview.md).
- An [external message broker](../message-brokers/overview.md) — Redis, NATS,
  RabbitMQ, or a managed cloud service — as the only channel between the tiers.

Workers are stateless and horizontally scalable: run as many replicas as you need,
each with a unique worker id (a pod name works well). To split execution across
specialised fleets, see [Dedicated Worker Pools](dedicated-worker-pools.md).
