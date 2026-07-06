---
name: REST Server
description: An HTTP REST server that exposes processes registered on a MessageBroker via REST endpoints, with Server-Sent Events for per-instance event streaming. Optionally embeds a worker in the same binary via EmbeddedWorker.
targets:
  - ../rest/server.go
---

# REST Server

The `blkit.restserver` package provides a long-running HTTP server that exposes processes registered on a [`MessageBroker`](../message-brokers/overview.spec.md) as REST endpoints. Clients submit process runs over HTTP, respond to per-instance input requests, cancel/terminate, and observe progress via Server-Sent Events.

The REST server interacts only with a `MessageBroker`. It does not hold a `StateStore` or any direct queue reference. Each `POST` to the submission endpoint runs `broker.Submit(...)`; each SSE subscription runs `broker.SubscribeToInstance(...)`. State queries (admin UIs, audit) connect to the `StateStore` directly, separately from this interface.

```go
package restserver

// Run blocks until ctx is cancelled or the underlying http.Server returns
// an error other than http.ErrServerClosed.
//
// If opts.EmbeddedWorker is non-nil, Run additionally spawns a worker.Run
// in the same process — the producer (REST server) and the consumer
// (worker) live in one binary, the REST server owns the worker's lifecycle,
// and ctx cancellation stops both together.
//
// If opts.EmbeddedWorker is nil, Run is broker-only and relies on remote
// workers to consume the broker's job queue.
func Run(ctx context.Context, broker bl.MessageBroker, opts Options) error

type Options struct {
    // HTTP listen address. Required. e.g. ":8080" or "127.0.0.1:9090".
    Addr string

    // Per-process tool description and JSON Schema. Required: clients calling
    // GET /processes need to know each process's input shape.
    Schema func(reg bl.ProcessRegistration) (description string, inputSchema map[string]any, err error)

    // Default StartID for submissions when the client doesn't specify one in
    // the URL or body.
    StartID string // default "start"

    // Optional callbacks.
    Input    func(ctx context.Context, raw map[string]any) (map[string]any, error)        // default: identity
    Response func(ctx context.Context, result *EvaluationResult) (any, error)             // default: snapshot of result.Context

    // Deprecated/no-op in v1: registry updates are pushed via
    // broker.SubscribeToProcessRegistry(...). Reserved for back-compat with
    // earlier polling designs.
    RegistryPollInterval *time.Duration

    // If non-nil, Run also runs an embedded worker in the same process.
    EmbeddedWorker *EmbeddedWorkerOpts
}

type EmbeddedWorkerOpts struct {
    StateStore StateStore // required when embedding a worker
    WorkerID   string     // required; unique per worker instance

    // Optional worker-tuning fields — same shape as worker.Options. Defaults
    // mirror worker.Options' defaults.
    HeartbeatInterval   *time.Duration
    MaxConcurrent       *int
    TaskConcurrency     *int
    PollInterval        *time.Duration
    MaxWriters          *int
    MaxBatchSize        *int
    MaxBatchWait        *time.Duration
    IdleTimeout         *time.Duration
    WriterChannelBuffer *int
    WritePolicy         worker.WritePolicy
}
```

`Run` is the long-running blocking entrypoint, analogous to `mcpserver.Run` in [../mcp/mcp-server.spec.md](../mcp/mcp-server.spec.md) and `worker.Run` in [../worker/worker.spec.md](../worker/worker.spec.md). The first parameter is `ctx` (the stdlib `context.Context`).

`Run` blocks until `ctx` is cancelled. On clean shutdown it returns `nil`. The HTTP server is shut down with a graceful drain — in-flight requests finish, then the server stops listening.

---

## Routing

Routing uses `net/http.ServeMux` (Go 1.22+) only — no third-party dependencies. Every URL is verb-led: the first path segment names the broker verb the endpoint maps to. There is one URL per verb, and the URL form mirrors the broker interface.

| Method | Path | Handler |
|---|---|---|
| `POST` | `/submit/{namespace}/{processId}/{version}` | Submit a new process run |
| `POST` | `/cancel/{instanceId}` | Cancel a pending / running / suspended instance |
| `POST` | `/terminate/{instanceId}` | Hard-stop a running/suspended instance |
| `POST` | `/respond-to-input-request/{instanceId}/{requestId}` | Respond to a `RequestInputTask` waiting on this instance |
| `GET`  | `/subscribe-to-instance/{instanceId}` | SSE stream of `InstanceEvent`s for one instance |
| `GET`  | `/subscribe-to-process-registry` | SSE stream of `RegistryUpdate`s |
| `GET`  | `/list-processes` | All registrations, summary fields only (no markdown) |
| `GET`  | `/describe-processes` | All registrations, full detail including markdown |
| `GET`  | `/describe-process/{namespace}/{processId}/{version}` | One registration, full detail including markdown |
| `GET`  | `/check-process-registration/{namespace}/{processId}/{version}` | Lightweight liveness check (always 200) |

### Routing implementation: handling `{namespace}`

`{namespace}` is a Go import path (e.g. `github.com/foo/bar`) and contains slashes. Go's `net/http.ServeMux` matches one segment per `{name}` placeholder by default, so the three endpoints that take `{namespace}/{processId}/{version}` need a trailing-wildcard pattern internally:

- `POST /submit/{rest...}`
- `GET /describe-process/{rest...}`
- `GET /check-process-registration/{rest...}`

The handler captures the wildcard, splits on `/`, treats the last two segments as `processId` / `version` and everything before as `namespace`. The user-facing URL shape is still `/{verb}/{namespace}/{processId}/{version}` — the wildcard is internal to the routing layer.

Clients construct URLs by concatenating the namespace verbatim (including its slashes); no URL-encoding of internal slashes is required.

---

## Endpoint details

### `POST /submit/{namespace}/{processId}/{version}`

Submits a new process run. The path determines `(Namespace, ProcessID, Version)`; the request body carries the submission payload:

```json
{
  "startId": "start",                // optional; overrides opts.StartID
  "input":   { "applicant": {...}, "loan_amount": 250000 },
  "correlationKey": "req-abc123"     // optional; mirrored onto every InstanceEvent
}
```

The handler:

1. Parses the path with the trailing-wildcard routing trick (see [Routing](#routing)).
2. Calls `opts.Input(ctx, body.input)` to produce the final `Input` map (default: pass through).
3. Constructs `StartRequest{Namespace, ProcessID, Version, StartID, Input, CorrelationKey}` and calls `broker.Submit(ctx, req)`.

**Response 202**: `{"instanceId": "..."}` on successful publish.

**Response 400**: returned for `ErrUnknownStartID` / `DataContractValidationError` from the broker, with the error message in the body. Validation runs against the `InputContract` carried in the broker registry's `ProcessRegistration` — the REST server does not import the process-definition packages.

**Response 404**: returned for `ErrUnknownProcess`.

**Response 500**: returned for broker-publish errors and any other unexpected errors.

The endpoint returns immediately — execution happens asynchronously on the worker. To watch the instance's progress, the client subsequently opens an SSE connection to `/subscribe-to-instance/{instanceId}`.

### `POST /cancel/{instanceId}`

Request body (optional):

```json
{ "reason": "user clicked cancel" }
```

The handler resolves the `(Namespace, ProcessID, Version)` for `{instanceId}` from a server-internal `instanceId → triple` map populated on each Submit, constructs `CancelRequest{Namespace, ProcessID, Version, InstanceID, Reason}`, and calls `broker.Cancel(ctx, req)`. The HTTP client supplies only `{instanceId}`.

**Response 202**: successful publish — the cancel was either pruned from the queue (Pending) or posted as an instruction (Running/Suspended). Outcome flows back via the SSE stream at `/subscribe-to-instance/{instanceId}`.

**Response 400**: `ErrCancelNotAllowed` (instance is Running/Suspended and the process opted out of external cancellation).

**Response 404**: `ErrUnknownProcess` — the process registration aged out, or the instance's process was never advertised to the broker. Also returned if the REST server has no `instanceId → triple` mapping for the given `{instanceId}`.

**Response 500**: broker-publish error.

There is no synchronous already-finished error — the broker holds no per-instance status record. A cancel for an instance that has already finished surfaces asynchronously as an `error` event with `code: "ALREADY_FINISHED"` on the SSE stream at `/subscribe-to-instance/{instanceId}`.

### `POST /terminate/{instanceId}`

Identical to `/cancel` but calls `broker.Terminate(ctx, TerminateRequest{...})`. Always requires `AllowExternalTerminate` (terminate has no queue-side short-circuit). Returns `400` for `ErrTerminateNotAllowed`. Terminate is fire-and-forget: an already-finished instance surfaces asynchronously as an `error` event with `code: "ALREADY_FINISHED"` on the SSE stream, exactly as for `/cancel`.

### `POST /respond-to-input-request/{instanceId}/{requestId}`

Responds to a `RequestInputTask` that this instance is currently waiting on. The client first learns about the pending request via the SSE stream (`input_request` event, which carries `requestId`). Request body:

```json
{ "payload": { "...": "..." } }
```

The handler calls `broker.RespondToInputRequest(ctx, instanceId, requestId, payload)`.

**Response 202**: empty body. The handler does not wait for the worker to consume the job; success means the response was published. Outcomes (`INSTANCE_NOT_FOUND`, `NOT_WAITING`, contract-validation failures) flow back as `error` events on the SSE stream.

**Response 500**: broker-publish error.

### `GET /subscribe-to-instance/{instanceId}`

Server-Sent Events stream of `InstanceEvent`s for one instance. The handler:

1. Sets `Content-Type: text/event-stream`, `Cache-Control: no-cache`, `Connection: keep-alive`.
2. Calls `broker.SubscribeToInstance(ctx, instanceId)` to obtain the event channel.
3. For each `InstanceEvent` received, writes one SSE frame:

```
event: <kind>
id:    <ulid or sequence number>
data:  <JSON-encoded event payload>

```

`<kind>` is the lowercase event-kind name: `lifecycle`, `input_request`, `node_completed`, `error`, `result`. The trailing blank line is required by the SSE protocol.

The connection closes when:

- The client disconnects (the request `ctx` cancels), **or**
- The instance reaches a finished status (Completed / Cancelled / Failed) and the corresponding final event is delivered (the channel from `broker.SubscribeToInstance` closes).

Heartbeats: the handler sends an SSE comment frame (`: heartbeat\n\n`) every 15s when no real events are pending, to keep idle proxies from killing the connection.

### `GET /subscribe-to-process-registry`

Server-Sent Events stream of `RegistryUpdate` messages. The handler calls `broker.SubscribeToProcessRegistry(ctx)` and emits each update as one SSE frame with `event: <kind>` where `<kind>` is the lowercase `RegistryUpdateKind` name: `snapshot`, `snapshot_complete`, `added`, `removed`, `heartbeat_lost`.

```
event: snapshot
data:  {"namespace":"...","processId":"...","version":"...", ...}

event: snapshot_complete
data:  {}

event: added
data:  {"namespace":"...","processId":"...","version":"...", ...}

event: heartbeat_lost
data:  {"namespace":"...","processId":"...","version":"..."}
```

The stream closes only on `ctx` cancellation. Same heartbeat behavior as `/subscribe-to-instance/{instanceId}`.

### `GET /list-processes`

Returns an array of `ProcessRegistration` JSON objects from the server's local registry cache (maintained by `broker.SubscribeToProcessRegistry(ctx)` — see [Startup sequence](#startup-sequence)), with **summary fields only** (no `markdown` body, no full input/output schemas — just enough to render a catalog or menu). Each entry is augmented with the description produced by `opts.Schema(reg)`.

**Response 200**:

```json
[
  {
    "namespace":   "example.com/processes/lending",
    "processId":   "loan-application",
    "version":     "1.0",
    "name":        "Loan Application",
    "description": "End-to-end loan application pipeline",
    "allowExternalCancel":    true,
    "allowExternalTerminate": false
  }
]
```

The cache is push-updated by `broker.SubscribeToProcessRegistry(ctx)`.

### `GET /describe-processes`

Returns an array of `ProcessRegistration` JSON objects with **full detail**, including each process's `markdown` body, `startEvents[].inputSchema`, and `endEvents[].outputSchema`. Use for bulk pre-fetch (caches, doc generators, MCP-style clients that want everything at startup). Payload is heavier than `/list-processes` — typically KBs per process — so prefer `/list-processes` for catalog rendering and reach for this only when the markdown / schemas are actually needed.

**Response 200**: array of objects of the shape returned by `/describe-process/{namespace}/{processId}/{version}` (below).

### `GET /describe-process/{namespace}/{processId}/{version}`

Returns the single `ProcessRegistration` matching the URL path, including the full markdown spec (`reg.Markdown`) and start-/end-event schemas.

**Response 200**:

```json
{
  "namespace":   "example.com/processes/lending",
  "processId":   "loan-application",
  "version":     "1.0",
  "name":        "Loan Application",
  "description": "End-to-end loan application pipeline",
  "startEvents": [
    { "id": "start", "name": "Start", "inputSchema": { "type": "object", ... } }
  ],
  "endEvents": [
    { "id": "approved", "name": "Approved", "outputSchema": { ... } },
    { "id": "rejected", "name": "Rejected" }
  ],
  "allowExternalCancel":    true,
  "allowExternalTerminate": false,
  "markdown": "# Loan Application\n\n..."
}
```

**Response 404**: `{"error": "unknown process"}` when no registration matches.

### `GET /check-process-registration/{namespace}/{processId}/{version}`

Lightweight existence-and-liveness check for one `(namespace, processId, version)`. Used for pre-flight checks before Submit, by rate-limiters, or by a UI that enables/disables a "submit" button based on whether any worker is currently registered for the process.

Always returns `200` — "I asked, here's the answer" semantics — never `404`, since "not registered" is a valid answer rather than an error. Reads from the same local registry cache.

**Response 200** (registered):

```json
{
  "registered": true,
  "workerCount": 3,
  "allowExternalCancel": true,
  "allowExternalTerminate": false,
  "latestHeartbeat": "2026-05-14T08:00:00Z"
}
```

**Response 200** (not registered):

```json
{ "registered": false }
```

`workerCount` is the number of distinct `workerId`s currently registered for the triple (per the broker's `ProcessRegistration` model, multiple workers can register the same triple). `latestHeartbeat` is the most recent `LastHeartbeat` across those workers. Distinct from `/describe-process` (which returns the full markdown body, schemas, end-event metadata): `/describe-process` is "show me the docs", `/check-process-registration` is "is this thing alive right now?".

---

## Startup sequence

When `Run` is called:

1. **Embedded worker (if any)** — if `opts.EmbeddedWorker != nil`, spawn a goroutine that calls `worker.Run(ctx, broker, opts.EmbeddedWorker.StateStore, workerOpts)` with the worker fields translated from `EmbeddedWorkerOpts`. Identical pattern to the MCP server's embedded mode (see [../mcp/mcp-server.spec.md](../mcp/mcp-server.spec.md)).

   If the worker's `RegisterProcesses` call fails, `Run` returns the error without listening.

2. **Registry subscription** — call `broker.SubscribeToProcessRegistry(ctx)` and spawn a goroutine that maintains a local in-memory map keyed by `(Namespace, ProcessID, Version)`. The first batch of `RegistryUpdate`s carries the snapshot (each is `RegistryUpdateSnapshot`), terminated by a single `RegistryUpdateSnapshotComplete` sentinel; after that, the goroutine applies `Added` / `Removed` / `HeartbeatLost` updates as they arrive. The cache is read under a mutex during request dispatch.

   Until the snapshot phase completes, the registry-read endpoints (`GET /list-processes`, `GET /describe-processes`, `GET /describe-process/...`, `GET /check-process-registration/...`) return `503 Service Unavailable` and submission attempts return `404`.

3. **HTTP server** — construct an `http.Server{Addr: opts.Addr, Handler: mux}` where `mux` is a configured `*http.ServeMux` with the routes documented above. Call `srv.ListenAndServe()`.

4. **Block** on the HTTP server until `ctx` is cancelled or the server returns an error.

---

## Shutdown sequence

When `ctx` is cancelled:

1. Call `srv.Shutdown(graceCtx)` with a derived context for graceful drain. In-flight requests finish; SSE streams see their channels close as the broker honours `ctx`.
2. Wait for the embedded worker goroutine (if any) to finish. The worker performs its own graceful shutdown: stop fetching new jobs, drain in-flight executors, unregister, drain writer pool, return.
3. `Run` returns `ctx.Err()` (or `nil` if `ctx` cancelled cleanly).

If the embedded worker fails mid-life, the worker goroutine returns an error; `Run` propagates it back to the caller after stopping the HTTP server.

---

## SSE format reference

Each `InstanceEvent` from `broker.SubscribeToInstance(...)` becomes one SSE frame:

```
event: lifecycle
id:    01HZ...
data:  {"phase":"Running"}

event: input_request
id:    01HZ...
data:  {"nodeId":"await-approval","requestId":"01HZ...","payload":{"prompt":"Approve loan?"}}

event: node_completed
id:    01HZ...
data:  {"nodeId":"validate","nodeKind":"NativeFunctionTask","outputs":{"is_valid":true}}

event: error
id:    01HZ...
data:  {"code":"INSTANCE_NOT_FOUND","message":"..."}

event: result
id:    01HZ...
data:  {"status":"Completed","context":{...}}
```

The `id:` field uses a stream-unique identifier (ULID or broker-supplied sequence). Clients reconnecting with `Last-Event-ID` get a "best-effort" replay only — the broker's retention policy governs how far back replay can go (per-broker spec). The REST server does not buffer events itself.

The terminal frame on an instance-scoped stream is whichever of `result` (Completed) / `error` (Failed) / a `Cancelled` `lifecycle` event lands first. The handler closes the connection after writing it.

---

## Examples

### All-in-one binary (embedded worker)

```go
package main

import (
    "context"
    "log"
    "os"
    "os/signal"
    "syscall"

    bl "github.com/friendly-business-machines/blkit/core"
    redisbroker "github.com/friendly-business-machines/blkit/brokers/redis"
    "blkit/restserver"

    _ "example.com/processes/lending"
)

func main() {
    ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
    defer stop()

    broker, err := redisbroker.New(redisbroker.Config{
        Addr: os.Getenv("BLKIT_REDIS_ADDR"),
    })
    if err != nil {
        log.Fatal(err)
    }
    defer broker.Close()

    store := bl.NewPostgresStateStore(os.Getenv("BLKIT_DB_URL"), "lending")

    err = restserver.Run(ctx, broker, restserver.Options{
        Addr: ":8080",
        Schema: func(reg bl.ProcessRegistration) (string, map[string]any, error) {
            desc := ""
            if reg.Description != nil {
                desc = *reg.Description
            }
            return desc, jsonSchemaFromRegistration(reg), nil
        },

        EmbeddedWorker: &restserver.EmbeddedWorkerOpts{
            StateStore: store,
            WorkerID:   hostname() + "-rest",
        },
    })
    if err != nil {
        log.Fatalf("restserver: %v", err)
    }
}
```

### Remote-only (workers run elsewhere)

```go
func main() {
    ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
    defer stop()

    broker, err := redisbroker.New(redisbroker.Config{
        Addr: os.Getenv("BLKIT_REDIS_ADDR"),
    })
    if err != nil {
        log.Fatal(err)
    }
    defer broker.Close()

    err = restserver.Run(ctx, broker, restserver.Options{
        Addr:   ":8080",
        Schema: schemaFromRegistration,
        // No EmbeddedWorker. Tools are exposed only if a remote worker
        // pool has registered the corresponding processes with the broker.
    })
    if err != nil {
        log.Fatalf("restserver: %v", err)
    }
}
```

### Client interaction

A typical client flow combining submit + SSE:

```javascript
// 1. Submit
const submit = await fetch(`/submit/example.com/processes/lending/loan-application/1.0`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ input: { applicant: {...}, loan_amount: 250000 } }),
});
const { instanceId } = await submit.json();

// 2. Open SSE stream
const events = new EventSource(`/subscribe-to-instance/${instanceId}`);

events.addEventListener("input_request", (e) => {
    const req = JSON.parse(e.data);
    promptUserForApproval(req).then((response) => {
        fetch(`/respond-to-input-request/${instanceId}/${req.requestId}`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ payload: response }),
        });
    });
});

events.addEventListener("result", (e) => {
    const result = JSON.parse(e.data);
    console.log("done", result);
    events.close();
});
```

---

## Concurrency & Lifecycle

The `MessageBroker` interface is required to be safe for concurrent use, by contract, so multiple in-flight HTTP requests sharing the same broker is safe. `Options` is captured by value at `Run` time and not mutated. The cached `ProcessRegistration` map is read under a mutex during request dispatch and written under the same mutex by the registry-subscription goroutine.

SSE handlers each spawn a `broker.SubscribeToInstance(...)` subscription. Each is independent — multiple clients watching the same instance get the same event stream, fanned out by the broker implementation.

---

## Edge Cases

- The broker has no live workers when `Run` starts: `broker.SubscribeToProcessRegistry` delivers an empty snapshot (just the `RegistryUpdateSnapshotComplete` sentinel). `GET /list-processes` and `GET /describe-processes` return `[]`; `GET /check-process-registration/...` returns `{"registered": false}`. Submission attempts return `404` (process not found) until a worker registers and the corresponding `RegistryUpdateAdded` arrives.
- `opts.Addr` is empty: `Run` returns a `ValueError`.
- `opts.Schema` is nil: `Run` returns a `ValueError`. Client tooling needs the schema to construct submissions.
- The HTTP server exits with a non-`ErrServerClosed` error (e.g. port already in use): `Run` returns that error. The embedded worker (if any) is still drained before returning.
- A client opens an SSE stream then disconnects: the request `ctx` cancels, `broker.SubscribeToInstance`'s channel returns no more events, the handler exits, the broker cleans up the subscription.
- Slow SSE consumer (small client read buffer): backpressure is handled by the broker. If events are dropped, a `BACKPRESSURE_DROP` error event is emitted on the stream — see [../message-brokers/overview.spec.md](../message-brokers/overview.spec.md).
- An SSE client uses `Last-Event-ID` to reconnect after a network blip: replay is best-effort and depends on the broker's retention policy. Per-implementation specs document the actual replay window.
- A `POST /cancel/{instanceId}` for an already-finished instance: the handler returns `202` — the broker holds no per-instance status record, so this cannot be detected synchronously. The worker that receives the `JobCancel` posts `InstanceError{Code: "ALREADY_FINISHED"}`, which surfaces as an `error` event on the SSE stream at `/subscribe-to-instance/{instanceId}`.
- `GET /subscribe-to-instance/{instanceId}` for an unknown instance: the broker delivers an `error` event with `code: "INSTANCE_NOT_FOUND"` (per the message-broker spec's async-error model), then closes the channel. The SSE handler forwards the error frame to the client and closes the connection.
- Multiple subscribers to the same instance via SSE: each gets the full event stream by default (broadcast). See [../message-brokers/overview.spec.md](../message-brokers/overview.spec.md).
- The embedded worker fails `RegisterProcesses` at startup: `Run` returns the error without binding the listening port.
- `EmbeddedWorker.WorkerID` is empty: `Run` returns a `ValueError` before starting anything.

---

## Out of Scope (v1)

The following are deliberately not in v1. They can be added without breaking the API:

- **Authentication / authorization** — no `Authenticate` hook. Callers that need auth wrap the handler externally (e.g. with their own reverse-proxy middleware) or use a standard `net/http` middleware chain.
- **CORS** — no built-in CORS handling. Add via middleware externally if needed for browser clients.
- **Health-check endpoint** — no `GET /healthz`. Add a route externally if needed for k8s liveness probes.
- **OpenAPI / JSON Schema document** — no auto-generated `/openapi.json`. Clients construct requests against the per-process schemas surfaced through `GET /list-processes` and `GET /describe-process/{namespace}/{processId}/{version}`.
- **WebSocket transport** — see future `blkit/websocketserver` for a parallel WebSocket-based protocol.
- **gRPC transport** — see future `blkit/grpcserver`.
