# Proposal

## Why

Processes can durably pause until a time but cannot ask a client for new information and resume the same instance when it arrives. Clients also need prompt notification without repeatedly polling instance status, while distributed workers and API replicas continue to coordinate only through PostgreSQL.

## What Changes

- Add a typed `.bl` input-request task node whose response ports can be bound to downstream nodes, including repeated, parallel, and subprocess activations.
- Persist outstanding requests and responses with the instance checkpoint; expose requests through instance status, accept replies through HTTP POST, and resume the same instance from its saved state.
- Expose an SSE instance stream with a current snapshot and notifications; retain GET for recovery. Local mode signals in-process. Distributed API replicas use per-replica PostgreSQL LISTEN channels and leased client-interest registrations, with selective commit-time database notifications and resynchronization after disconnects.
- Preserve typed validation, cancellation, deadlines, claim fencing, and safe handling of late or duplicate replies. Document operator-managed authenticated ingress for remote clients; mobile background push, webhooks, WebSockets, and direct worker-to-API calls are out of scope.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Define source syntax and typed bindings for input-request task nodes.
- `process-runtime`: Define durable request/response execution, status, POST and SSE contracts in local and distributed modes.
- `distributed-execution`: Define fenced request transitions and selective, reconnect-safe PostgreSQL notification routing for API replicas.

## Impact

Parser, graph validation and code generation in `crates/blkit-transpiler`; compiled graph, scheduler, subprocess handling, local and PostgreSQL stores, and Axum router in `crates/blkit-core`; generated API binary templates and examples/tests/docs. The distributed API remains graph-free and needs persisted portable request type metadata. One dedicated PostgreSQL listener connection per API replica sits outside its bounded request pool; database schema, role connection budgets, and load testing are affected.
