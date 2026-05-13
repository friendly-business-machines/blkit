---
name: REST Server
description: An HTTP REST server that exposes processes registered on a BrokerGateway via REST endpoints, with Server-Sent Events for per-instance event streaming. Optionally embeds a worker in the same binary via EmbeddedWorker.
targets:
  - ../rest/server.go
---

# REST Server

The `blkit.restserver` package provides a long-running HTTP server that exposes processes registered on a [`BrokerGateway`](../messagebroker/overview.spec.md) as REST endpoints. Clients submit process runs over HTTP, respond to per-instance input requests, cancel/terminate, and observe progress via Server-Sent Events.

The REST server interacts only with a `BrokerGateway`. It does not hold a `StateStore` or any direct queue reference. Each `POST` to the submission endpoint runs `gw.Submit(...)`; each SSE subscription runs `gw.SubscribeToInstance(...)`. State queries (admin UIs, audit) connect to the `StateStore` directly, separately from this interface.

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
func Run(ctx context.Context, gw messagebroker.BrokerGateway, opts Options) error

type Options struct {
    // HTTP listen address. Required. e.g. ":8080" or "127.0.0.1:9090".
    Addr string

    // Per-process tool description and JSON Schema. Required: clients calling
    // GET /processes need to know each process's input shape.
    Schema func(reg messagebroker.ProcessRegistration) (description string, inputSchema map[string]any, err error)

    // Default StartID for submissions when the client doesn't specify one in
    // the URL or body.
    StartID string // default "start"

    // Optional callbacks.
    Input    func(ctx context.Context, raw map[string]any) (map[string]any, error)        // default: identity
    Response func(ctx context.Context, result *EvaluationResult) (any, error)             // default: snapshot of result.Context

    // Deprecated/no-op in v1: registry updates are pushed via
    // gw.SubscribeToProcessRegistry(...). Reserved for back-compat with
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

Routing uses `net/http.ServeMux` (Go 1.22+) only — no third-party dependencies. The path patterns documented below use the standard `{name}` placeholder syntax that `ServeMux` parses natively.

| Method | Path | Handler |
|---|---|---|
| `GET` | `/processes` | List all `ProcessRegistration`s the broker currently exposes |
| `GET` | `/processes/{namespace}/{processId}/{version}` | Single process registration (description + input schema + markdown) |
| `POST` | `/processes/{namespace}/{processId}/{version}/instances` | Submit a new process run |
| `POST` | `/instances/{instanceId}/input-responses` | Respond to a `RequestInputTask` waiting on this instance |
| `POST` | `/instances/{instanceId}/cancel` | Cancel a pending / running / suspended instance |
| `POST` | `/instances/{instanceId}/terminate` | Hard-stop a running/suspended instance |
| `GET` | `/instances/{instanceId}/events` | SSE stream of `InstanceEvent`s for the instance |
| `GET` | `/instances/{instanceId}/result` | Block until the instance finishes; return final `EvaluationResult` |

`{namespace}` typically contains slashes (it's a Go package path). The handler extracts the full segment between `/processes/` and `/{processId}` as the namespace. Implementations may URL-encode the namespace in client-side URL construction; the server URL-decodes on receipt.

---

## Endpoint details

### `GET /processes`

Returns an array of `ProcessRegistration` JSON objects from the server's local registry cache (maintained by `gw.SubscribeToProcessRegistry(ctx)` — see [Startup sequence](#startup-sequence)), each augmented with the description and input schema produced by `opts.Schema(reg)`.

**Response 200**:

```json
[
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
      { "id": "approved", "name": "Approved" },
      { "id": "rejected", "name": "Rejected" }
    ],
    "allowExternalCancel":    true,
    "allowExternalTerminate": false
  }
]
```

The cache is push-updated by `gw.SubscribeToProcessRegistry(ctx)` — adds, removes, and heartbeat losses arrive as `RegistryUpdate` messages.

### `GET /processes/{namespace}/{processId}/{version}`

Returns the single `ProcessRegistration` matching the URL path, including the full markdown spec (`reg.Markdown`) for clients that want to render it.

**Response 200**: a single object of the shape above, plus a `markdown` field carrying the markdown body.

**Response 404**: `{"error": "unknown process"}` when no registration matches.

### `POST /processes/{namespace}/{processId}/{version}/instances`

Submits a new process run. The request body is the `Input` payload. The path determines `(Namespace, ProcessID, Version)`; an optional `?startId=...` query parameter overrides `opts.StartID`.

The handler:

1. Constructs `StartRequest{Namespace, ProcessID, Version, StartID, Input: body}`.
2. Calls `opts.Input(ctx, body)` to produce the final `Input` map (default: pass through).
3. Calls `gw.Submit(ctx, req)`.

**Response 202**: `{"instanceId": "..."}` on successful publish.

**Response 400**: returned for `ErrUnknownStartID` / `DataContractValidationError` from the gateway, with the error message in the body.

**Response 404**: returned for `ErrUnknownProcess`.

**Response 500**: returned for broker-publish errors and any other unexpected errors.

The endpoint returns immediately — execution happens asynchronously on the worker. To watch the instance's progress, the client subsequently opens an SSE connection to `/instances/{instanceId}/events` or polls `/instances/{instanceId}/result`.

### `POST /instances/{instanceId}/input-responses`

Responds to a `RequestInputTask` that this instance is currently waiting on. The client first learns about the pending request via the SSE stream (`input_request` event, which carries `requestId`). Request body:

```json
{
  "requestId": "01HZ...",
  "payload": { "...": "..." }
}
```

The handler calls `gw.RespondToInputRequest(ctx, instanceID, requestID, payload)`.

**Response 202**: empty body. The handler does not wait for the worker to consume the job; success means the response was published. Outcomes (`INSTANCE_NOT_FOUND`, `NOT_WAITING`, contract-validation failures) flow back as `error` events on the SSE stream.

**Response 500**: broker-publish error.

### `POST /instances/{instanceId}/cancel`

Request body (optional):

```json
{ "reason": "user clicked cancel" }
```

The handler calls `gw.Cancel(ctx, CancelRequest{Namespace, ProcessID, Version, InstanceID, Reason})`. The `(Namespace, ProcessID, Version)` triple needs to be known to the handler — see [Instance Routing](#instance-routing) below for how that is resolved.

**Response 200**: queue-side cancellation succeeded (instance was Pending; no opt-in needed).

**Response 202**: cancel instruction published to a running/suspended instance; outcome flows back via the SSE stream.

**Response 400**: `ErrCancelNotAllowed` (instance is Running/Suspended and the process opted out of external cancellation).

**Response 404**: `ErrUnknownProcess` — the process registration aged out, or the instance's process was never advertised to the gateway.

**Response 409 Conflict**: `ErrAlreadyCompleted` / `ErrAlreadyCancelled` / `ErrAlreadyFailed` — instance is already finished. Response body carries the specific status.

**Response 500**: broker-publish error.

### `POST /instances/{instanceId}/terminate`

Identical to `/cancel` but calls `gw.Terminate(ctx, TerminateRequest{...})`. Always requires `AllowExternalTerminate` (terminate has no queue-side short-circuit). Returns `400` for `ErrTerminateNotAllowed` and `409` for the `ErrAlready*` family.

### `GET /instances/{instanceId}/events`

Server-Sent Events stream. The handler:

1. Sets `Content-Type: text/event-stream`, `Cache-Control: no-cache`, `Connection: keep-alive`.
2. Calls `gw.SubscribeToInstance(ctx, instanceID)` to obtain the event channel.
3. For each `InstanceEvent` received, writes one SSE frame:

```
event: <kind>
id:    <ulid or sequence number>
data:  <JSON-encoded event payload>

```

`<kind>` is the lowercase event-kind name: `status_change`, `input_request`, `node_completed`, `error`, `result`. The trailing blank line is required by the SSE protocol.

The connection closes when:

- The client disconnects (the request `ctx` cancels), **or**
- The instance reaches a finished status (Completed / Cancelled / Failed) and the corresponding final event is delivered (the channel from `gw.SubscribeToInstance` closes).

Heartbeats: the handler sends an SSE comment frame (`: heartbeat\n\n`) every 15s when no real events are pending, to keep idle proxies from killing the connection.

### `GET /instances/{instanceId}/result`

Synchronous wait for the instance to finish. The handler subscribes via `gw.SubscribeToInstance(ctx, instanceID)` and returns when the first event of kind `result` (Completed) or `error` (Failed) arrives, or when a `status_change` lands on `Cancelled`. Implementations may share this as a small helper (`waitForFinish(gw, instanceID)`); it is not a gateway verb.

**Response 200**: `opts.Response(ctx, result)`-shaped body when status is `Completed`.

**Response 422 Unprocessable Entity**: when status is `Failed` or `Cancelled`. Body includes the failure reason.

**Response 408 Request Timeout**: when the request `ctx` times out before the instance finishes.

**Long-poll behavior**: clients can supply a `?timeout=30s` query parameter to bound the wait. The handler builds a child context with that deadline. If the deadline fires before the instance finishes, the handler returns `408`. Default is no application-level timeout — the wait runs until the request `ctx` is cancelled.

---

## Instance Routing

`Cancel` and `Terminate` need the `(Namespace, ProcessID, Version)` of the target instance to call `gw.Cancel(...)` / `gw.Terminate(...)` (the gateway uses the triple to look up the process's `AllowExternalCancel` / `AllowExternalTerminate` opt-ins). The URL only carries `instanceId`. The REST server does not have direct `StateStore` access, so it cannot look up the instance's process triple.

Two options for resolving this, supported in v1:

1. **Client provides the triple in the request body.** The REST handler accepts:

   ```json
   {
     "namespace": "example.com/processes/lending",
     "processId": "loan-application",
     "version": "1.0",
     "reason": "..."
   }
   ```

   The client tracks `(instanceId → triple)` itself when it submits. This is the canonical v1 path.

2. **Client provides the triple in query parameters** as a fallback:

   ```
   POST /instances/{instanceId}/cancel?namespace=...&processId=...&version=...
   ```

   Useful for clients that prefer URL-only cancellation (e.g. simple `<form>` submits).

If the client provides neither, the handler returns `400 Bad Request` with a message explaining the requirement.

A future extension could have the broker maintain an `instanceId → triple` mapping (via the same TTL-backed registry the gateway uses for processes). v1 keeps that out of scope.

---

## Startup sequence

When `Run` is called:

1. **Embedded worker (if any)** — if `opts.EmbeddedWorker != nil`, spawn a goroutine that calls `worker.Run(ctx, gw, opts.EmbeddedWorker.StateStore, workerOpts)` with the worker fields translated from `EmbeddedWorkerOpts`. Identical pattern to the MCP server's embedded mode (see [../mcp/mcp-server.spec.md](../mcp/mcp-server.spec.md)).

   If the worker's `RegisterProcesses` call fails, `Run` returns the error without listening.

2. **Registry subscription** — call `gw.SubscribeToProcessRegistry(ctx)` and spawn a goroutine that maintains a local in-memory map keyed by `(Namespace, ProcessID, Version)`. The first batch of `RegistryUpdate`s carries the snapshot (each is `RegistryUpdateSnapshot`), terminated by a single `RegistryUpdateSnapshotComplete` sentinel; after that, the goroutine applies `Added` / `Removed` / `HeartbeatLost` updates as they arrive. The cache is read under a mutex during request dispatch.

   Until the snapshot phase completes, `GET /processes` returns `503 Service Unavailable` and submission attempts return `404`.

4. **HTTP server** — construct an `http.Server{Addr: opts.Addr, Handler: mux}` where `mux` is a configured `*http.ServeMux` with the routes documented above. Call `srv.ListenAndServe()`.

5. **Block** on the HTTP server until `ctx` is cancelled or the server returns an error.

---

## Shutdown sequence

When `ctx` is cancelled:

1. Call `srv.Shutdown(graceCtx)` with a derived context for graceful drain. In-flight requests finish; SSE streams see their channels close as the gateway honours `ctx`.
2. Wait for the embedded worker goroutine (if any) to finish. The worker performs its own graceful shutdown: stop fetching new jobs, drain in-flight executors, unregister, drain writer pool, return.
3. `Run` returns `ctx.Err()` (or `nil` if `ctx` cancelled cleanly).

If the embedded worker fails mid-life, the worker goroutine returns an error; `Run` propagates it back to the caller after stopping the HTTP server.

---

## SSE format reference

Each `InstanceEvent` from `gw.SubscribeToInstance(...)` becomes one SSE frame:

```
event: status_change
id:    01HZ...
data:  {"from":"Pending","to":"Running"}

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

The `id:` field uses a stream-unique identifier (ULID or broker-supplied sequence). Clients reconnecting with `Last-Event-ID` get a "best-effort" replay only — the gateway's retention policy governs how far back replay can go (per-broker spec). The REST server does not buffer events itself.

The terminal frame on an instance-scoped stream is whichever of `result` (Completed) / `error` (Failed) / a `Cancelled` `status_change` lands first. The handler closes the connection after writing it.

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

    "blkit"
    "blkit/messagebroker"
    "blkit/restserver"

    _ "example.com/processes/lending"
)

func main() {
    ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
    defer stop()

    gw, err := messagebroker.NewRedisBrokerGateway(messagebroker.RedisOpts{
        Addr: os.Getenv("BLKIT_REDIS_ADDR"),
    })
    if err != nil {
        log.Fatal(err)
    }
    defer gw.Close()

    store := blkit.NewPostgresStateStore(os.Getenv("BLKIT_DB_URL"), "lending")

    err = restserver.Run(ctx, gw, restserver.Options{
        Addr: ":8080",
        Schema: func(reg messagebroker.ProcessRegistration) (string, map[string]any, error) {
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

    gw, err := messagebroker.NewRedisBrokerGateway(messagebroker.RedisOpts{
        Addr: os.Getenv("BLKIT_REDIS_ADDR"),
    })
    if err != nil {
        log.Fatal(err)
    }
    defer gw.Close()

    err = restserver.Run(ctx, gw, restserver.Options{
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
const submit = await fetch(`/processes/example.com%2Fprocesses%2Flending/loan-application/1.0/instances`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ applicant: {...}, loan_amount: 250000 }),
});
const { instanceId } = await submit.json();

// 2. Open SSE stream
const events = new EventSource(`/instances/${instanceId}/events`);

events.addEventListener("input_request", (e) => {
    const req = JSON.parse(e.data);
    promptUserForApproval(req).then((response) => {
        fetch(`/instances/${instanceId}/input-responses`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ requestId: req.requestId, payload: response }),
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

The `BrokerGateway` interface is required to be safe for concurrent use, by contract, so multiple in-flight HTTP requests sharing the same gateway is safe. `Options` is captured by value at `Run` time and not mutated. The cached `ProcessRegistration` map is read under a mutex during request dispatch and written under the same mutex by the registry-subscription goroutine.

SSE handlers each spawn a `gw.SubscribeToInstance(...)` subscription. Each is independent — multiple clients watching the same instance get the same event stream, fanned out by the gateway implementation.

---

## Edge Cases

- The broker has no live workers when `Run` starts: `gw.SubscribeToProcessRegistry` delivers an empty snapshot (just the `RegistryUpdateSnapshotComplete` sentinel). `GET /processes` returns `[]`. Submission attempts return `404` (process not found) until a worker registers and the corresponding `RegistryUpdateAdded` arrives.
- `opts.Addr` is empty: `Run` returns a `ValueError`.
- `opts.Schema` is nil: `Run` returns a `ValueError`. Client tooling needs the schema to construct submissions.
- The HTTP server exits with a non-`ErrServerClosed` error (e.g. port already in use): `Run` returns that error. The embedded worker (if any) is still drained before returning.
- A client opens an SSE stream then disconnects: the request `ctx` cancels, `gw.SubscribeToInstance`'s channel returns no more events, the handler exits, the gateway cleans up the subscription.
- Slow SSE consumer (small client read buffer): backpressure is handled by the gateway. If events are dropped, a `BACKPRESSURE_DROP` error event is emitted on the stream — see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md).
- An SSE client uses `Last-Event-ID` to reconnect after a network blip: replay is best-effort and depends on the gateway's retention policy. Per-implementation specs document the actual replay window.
- A `POST /instances/{instanceId}/cancel` for an already-finished instance: the handler returns `409 Conflict` with the specific `ErrAlready*` carried in the body. No event flows to the SSE stream — the gateway resolved this synchronously from the broker's status record.
- `GET /instances/{instanceId}/result` for an unknown instance: the request opens a subscription that yields an `error` event with `code: "INSTANCE_NOT_FOUND"` (per the messagebroker spec's async-error model), which the handler maps to `404 Not Found`.
- Multiple subscribers to the same instance via SSE: each gets the full event stream by default (broadcast). See [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md).
- The embedded worker fails `RegisterProcesses` at startup: `Run` returns the error without binding the listening port.
- `EmbeddedWorker.WorkerID` is empty: `Run` returns a `ValueError` before starting anything.

---

## Out of Scope (v1)

The following are deliberately not in v1. They can be added without breaking the API:

- **Authentication / authorization** — no `Authenticate` hook. Callers that need auth wrap the handler externally (e.g. with their own reverse-proxy middleware) or use a standard `net/http` middleware chain.
- **CORS** — no built-in CORS handling. Add via middleware externally if needed for browser clients.
- **Health-check endpoint** — no `GET /healthz`. Add a route externally if needed for k8s liveness probes.
- **OpenAPI / JSON Schema document** — no auto-generated `/openapi.json`. Clients construct requests against the per-process schemas surfaced through `GET /processes`.
- **WebSocket transport** — see future `blkit/websocketserver` for a parallel WebSocket-based protocol.
- **gRPC transport** — see future `blkit/grpcserver`.
