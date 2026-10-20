# Design

## Context

See proposal.md. `crates/blkit-transpiler` currently parses named process nodes and emits typed graph definitions; `crates/blkit-core` checkpoints graph activations, releases timer waits, and stores instance rows in local Turso or PostgreSQL. Remote API binaries have no linked graph definitions: only workers can initialize compiled checkpoints. The Axum router has start/status/cancel and no native auth; generated binaries and the dev harness share that router. PostgreSQL workers use fenced claims and bounded role pools. Timed waits have wake instants; input requests must instead wait for a client response. The three delta specs describe the required new behavior.

## Goals / Non-Goals

**Goals:** A typed, durable client input handoff that frees idle worker capacity; one response per graph activation; GET/POST/SSE usable from both API modes; selective wake-up across remote API replicas without worker-to-API communication. Preserve existing deadline, cancellation, retry, and subprocess safety.

**Non-Goals:** Webhooks, WebSockets, background mobile push, native end-user authentication, exactly-once external task effects, or a general-purpose message broker. A streamed hint is not a durable event log or an execution command.

## Decisions

### 1. Model input as a task node that suspends, not a blocked decision task

Parse `input_request <name> { prompt "..."; output <field>: <Type>; ... }` using the existing braced, semicolon-terminated node declaration syntax. Add task-node validation, compiled node kind, emitted response type metadata, and ordinary `flow`/`bind` support. The authored prompt is a static nonempty string in this iteration. The graph checkpoints each visit under its activation ID, including child scope, repeated visits and parallel branches; output bindings become available only after the matching response is committed. Reusing timed waits would invent a wake instant; blocking a task would occupy worker capacity and lose durable reply correlation.

### 2. Store requests separately but atomically with checkpoints

Use durable request records keyed by `(instance_id, request_id)` with an activation/scope reference, prompt, portable response schema, and outstanding/answered state plus the accepted JSON response. The checkpoint refers to stable request IDs; creating a request and its checkpoint, and consuming an answered request and advancing the graph, each commit atomically. Request IDs remain stable across retries and restart. Instance status includes outstanding requests; `waiting-for-input` applies only when no other work can proceed. While sibling tasks run, a response is stored without replacing the worker's checkpoint; the owner observes it at its next safe advancement. When idle, the worker releases its claim and sets `waiting-for-input`; a competing POST either commits before that release and prevents a stranded wait, or wins afterward and makes the row claimable. Do not use a clock wake or consume an execution retry on this handoff. Guard worker mutations by owner, lease, generation and deadline, and guard response mutations by request status and instance status in one transaction; preserve original first-claim/deadline origins on reclaim. Local mode uses the analogous serialized store updates and in-process wake-up.

A distinct request record avoids API replies being overwritten by an in-flight worker's full-checkpoint JSON write. It also makes request visibility and duplicate-response CAS independent of graph code. Compatibility must preserve reading old checkpoint versions; add backwards-compatible fields or explicitly version and migrate/guard active checkpoints before rollout.

### 3. Validate responses without compiled graphs in the API

The compiler serializes a portable descriptor of each response port's type, including resolved named dictionary shapes, to the request record. A shared runtime validator checks a JSON object with exactly those keys and compatible values at the POST boundary, using the same serialization rules as generated typed inputs; the worker validates again before consuming it. Reuse existing type/temporal validation patterns, and test API/worker agreement across every supported port type. Reject malformed/mistyped data without mutating the request. Conditional acceptance returns conflict for already answered or terminal/cancelled instances, not a second graph activation. Storing only `.bl` type names without schemas would make a graph-free remote API unable to validate named types; accepting any JSON for later worker validation would violate the immediate response contract.

### 4. Snapshot-on-connect SSE and ordinary HTTP commands

Add `GET /instances/{id}/events` for SSE and `POST /instances/{id}/requests/{request_id}/response` for replies to the shared router. GET status exposes request IDs, prompts and response schemas but never raw accepted replies to unrelated clients. On stream setup, register local interest, activate remote DB interest, read current persisted state and send a `snapshot` event. Later `changed` hints contain only the instance ID; the client GETs the latest state. Buffer or coalesce hints arriving during snapshot setup; always subscribe before the snapshot query. Every client reconnect receives a fresh snapshot; neither SSE nor PostgreSQL notifications promise replay or exactly-once delivery. Send SSE keepalives so ingress can detect a failed stream; bounded buffering closes slow consumers and lets them reconnect. For browser use, document ingress-provided cookie/session authorization, since native EventSource does not accept arbitrary authorization headers; other clients can use normal ingress credentials. Remote listeners remain loopback by default with operator-managed authenticated TLS ingress and per-instance authorization across GET/SSE/POST.

### 5. One PostgreSQL LISTEN channel per API replica, selective database fan-out

Each API replica creates a unique channel identity for its listener lifetime and holds one dedicated PostgreSQL connection outside the bounded request pool. It commits `LISTEN` before advertising any interests and avoids holding an open transaction while listening. A shared interests table holds `(instance_id, replica_id/channel, expires_at)` only while that replica has local subscribers for the instance; renew live leases, remove on last subscriber, and expire dead replicas' interests. Treat a lapsed live registration as a reconnection/resync event, not silent success. Database-side notification logic fires only after relevant request creation/answering or watched terminal/cancel transitions, looks up unexpired interests, and calls `pg_notify(replica_channel, instance_id)` within the same transaction. Ordinary task checkpoints and unwatched instances do not notify. Payload contains no request or response data; replicas check their own local subscription map and query authoritative state before publishing to their clients. No worker knows about replica identities or performs per-client routing. A dedicated connection changes total PostgreSQL session budget by one per API replica; pool checkout connections remain short-lived.

The database listener cannot be treated as a durable inbox. On listener failure, reconnect and commit LISTEN before restoring interests and rereading all locally watched instances, including those whose SSE streams remain connected. Deliver a fresh snapshot or force those streams to reconnect before declaring the listener healthy. Registration and snapshot ordering makes the race around a concurrent commit harmless; duplicate hints cause at most another read. Monitor connection health and queue usage. `LISTEN/NOTIFY` semantics and startup ordering: https://www.postgresql.org/docs/current/sql-listen.html and https://www.postgresql.org/docs/current/sql-notify.html.

### 6. Keep notification-bearing commits rare; defer batching

A `NOTIFY` in a high-frequency trigger can serialize otherwise concurrent commits: the cited case study measured contention on a global catalog lock (https://vyruss.org/computing/slides/pgdata2026_listen_carefully.pdf, slides 9–13). Per-replica routing reduces recipient traffic but does not eliminate the sender's commit cost. Only notify meaningful input-request and watched terminal transitions, deduplicate identical hints per transaction, and measure throughput, commit latency, SSE latency and `pg_notification_queue_usage()` under representative concurrency and replica fan-out. Do not copy the talk's UNLOGGED queue/try-lock batching workaround initially: a loser may commit an unsent row after the winner drained its visible rows, and UNLOGGED state can disappear on crash (slides 16–22). If this path becomes a measured bottleneck, a separately designed durable outbox/drainer and explicit recovery policy are necessary; there is no free guaranteed wake-up without traffic or recovery.

## Risks / Trade-offs

- [SSE stream or listener disconnects between changes] -> Establish listener before snapshot, resync watched state after reconnect, and never require notification replay.
- [Worker checkpoint races with client reply or cancel] -> Serialize request and instance transitions and fence owner writes; test both commit orders, including outstanding requests during parallel work.
- [Graph-free API validation diverges from generated types] -> Generate portable descriptors and cross-test API/worker validation on nested named dictionaries, temporal and other supported port types.
- [Registration lease expires while client remains connected] -> Renew based on active subscribers and resync when registration/listener health is lost; avoid false healthy streams.
- [High NOTIFY commit rate, queue saturation or slow SSE clients] -> Restrict trigger frequency, measure under load, monitor queue and listener, avoid open listener transactions, bound client buffers.
- [No built-in auth] -> Bind loopback and require per-instance authorized TLS ingress; explicitly warn operators not to expose status, streams or response POST directly.

## Migration Plan

Introduce idempotent schema additions before enabling new workers/API binaries. Roll out new API replicas with their listener and request schema support, then deploy compiled processes under a new process version so old workers cannot claim a new graph identity. Preserve old workers for their existing instances. Keep old checkpoints readable; old instances with no requests continue unchanged. On rollback, stop admitting new request-using process versions and retain capable workers/API until outstanding requests drain or are cancelled; do not drop the schema or downgrade to binaries unable to read their checkpoints. Document the extra listener connection in per-replica PostgreSQL capacity planning.
