---
name: MCP Server
description: A stdio MCP server that exposes processes registered on a BrokerGateway as MCP tools and resources. Each tool call is a Submit + Wait round-trip on the gateway. Optionally embeds a worker in the same binary via EmbeddedWorker.
targets:
  - ../mcp/server.go
---

# MCP Server

The `blkit.mcpserver` package provides a long-running MCP (Model Context Protocol) server that adapts the broker-held process registry into an MCP-compatible surface. Each process advertised by the broker is registered as an MCP **tool** (so MCP clients can invoke it) and an MCP **resource** (so MCP clients can read its markdown specification).

The MCP server interacts only with a [`BrokerGateway`](../messagebroker/overview.spec.md). It does not hold a `StateStore` or any direct queue reference. Each `tools/call` invocation runs `gw.Submit(...)` then `gw.Wait(...)` on the gateway and returns the final `EvaluationResult` as the tool result.

The transport is **stdio only**. Streamable HTTP and other MCP transports are out of scope.

```go
package mcpserver

// Run blocks until ctx is cancelled, returning nil on clean shutdown.
//
// If opts.EmbeddedWorker is non-nil, Run additionally spawns a worker.Run in
// the same process — the producer (MCP server) and the consumer (worker) live
// in one binary, the MCP server owns the worker's lifecycle, and ctx
// cancellation stops both together.
//
// If opts.EmbeddedWorker is nil, Run is broker-only and relies on remote
// workers to consume the broker's commands.
func Run(ctx context.Context, gw messagebroker.BrokerGateway, opts Options) error

type Options struct {
    ServerName    string // required: advertised in the MCP `initialize` response
    ServerVersion string // required: advertised in the MCP `initialize` response

    // Per-process tool description and JSON Schema. Required: the runtime
    // refuses to start with no Schema function, because MCP clients cannot
    // discover an untyped tool surface usefully.
    Schema func(reg messagebroker.ProcessRegistration) (description string, inputSchema map[string]any, err error)

    // Default StartID for tool calls when the MCP client doesn't specify one.
    StartID string // default "start"

    // Optional callbacks.
    Input    func(ctx context.Context, args map[string]any) (map[string]any, error)
    Response func(ctx context.Context, result *EvaluationResult) (any, error)

    // How often to re-query gw.ListAvailableProcesses(...) to pick up workers
    // that have joined or left. Default 30s. Set to 0 to disable refresh
    // (one-shot at startup).
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

`Run` is the long-running blocking entrypoint, analogous to `worker.Run` in [worker.spec.md](../worker/worker.spec.md). The first parameter is named `ctx` (the stdlib `context.Context`) — there is no `ExecutionContext` threaded through the public API.

`Run` blocks until `ctx` is cancelled, stdin returns EOF, or a fatal protocol error occurs. On clean shutdown it returns `nil`; on cancellation it returns `ctx.Err()`. In-flight tool-call handlers receive the same `ctx`, so cancellation propagates into their `gw.Wait(...)` calls.

---

## Startup sequence

When `Run` is called:

1. **Embedded worker (if any)** — if `opts.EmbeddedWorker != nil`, spawn a goroutine that calls `worker.Run(ctx, gw, opts.EmbeddedWorker.StateStore, workerOpts)` with the worker fields translated from `EmbeddedWorkerOpts`. The worker handles its own registration / heartbeat / consume / executor / writer-pool lifecycle. The MCP server doesn't reach into the worker directly. (See [worker.spec.md](../worker/worker.spec.md) for the worker's shape.)

   If the worker's `RegisterProcesses` call fails, `Run` returns the error without entering the MCP serve loop.

2. **Initial registry snapshot** — call `gw.ListAvailableProcesses(ctx)`. When the embedded worker is in the same process, its registrations have just been published to the broker, so this snapshot includes them. For remote-only deployments, only externally-running workers' processes appear.

3. **Tool registration** — for each `ProcessRegistration` in the snapshot:
   - Compute the tool name as `fmt.Sprintf("%s__%s__%s", reg.Namespace, reg.ProcessID, reg.Version)`. Double-underscore separators keep the name unambiguously parseable.
   - Call `opts.Schema(reg)` to obtain the description and JSON Schema.
   - Register an MCP tool whose handler runs the [Per-Tool Invocation Pipeline](#per-tool-invocation-pipeline) for this `(Namespace, ProcessID, Version)`.
   - Register an MCP resource at `blkit://process/{namespace}/{id}/{version}` whose body is `reg.Markdown`.

   If `opts.Schema(reg)` returns an error for any process, `Run` aborts startup with that error. No tools are registered, no resources are advertised, the embedded worker (if any) is stopped, and the stdio loop is not entered.

4. **`describe_process` built-in** — a single tool registered independent of registry contents:
   - **Name**: `describe_process`.
   - **Description**: `"Return the markdown specification for a registered blkit process."`
   - **Input schema**:
     ```json
     {
       "type": "object",
       "required": ["namespace", "processId", "version"],
       "properties": {
         "namespace":  { "type": "string" },
         "processId": { "type": "string" },
         "version":   { "type": "string" }
       }
     }
     ```
   - **Handler**: looks up the registration in the cached snapshot and returns `reg.Markdown` as `text/markdown` content. Returns an MCP error if the registration is unknown.

5. **Refresh goroutine** — if `opts.RegistryPollInterval > 0`, spawn a goroutine that re-queries `gw.ListAvailableProcesses(ctx)` every `RegistryPollInterval` and reconciles the tool list (registers new tools, deregisters tools whose `ProcessRegistration` has aged out). Default interval: 30s.

6. **Stdio loop** — enter the MCP stdio read loop on the caller's goroutine. Block until `ctx` is cancelled or stdin closes.

---

## Shutdown sequence

When `ctx` is cancelled:

1. The MCP serve loop exits.
2. `Run` waits for the embedded worker goroutine (if any) to finish — `worker.Run` performs its own graceful shutdown: stop accepting new commands, drain in-flight executors, unregister, drain writer pool, return.
3. `Run` returns `ctx.Err()`.

If the embedded worker fails mid-life (e.g. broker connection lost and unrecoverable), the worker goroutine returns an error; `Run` propagates it back to the caller after stopping the MCP serve loop.

---

## Per-Tool Invocation Pipeline

For each `tools/call` against a `{namespace}__{id}__{version}` tool:

1. **Process resolution** — `(Namespace, ProcessID, Version)` is bound to the tool's handler closure at registration time; no routing required.
2. **Input extraction** — `opts.Input(ctx, args)` converts raw MCP tool arguments into the `map[string]any` that `gw.Submit` accepts. The default `Input` returns `args` unchanged.
3. **Submit** — call `gw.Submit(ctx, StartRequest{Namespace, ProcessID, Version, StartID: opts.StartID, Input: input})`. Synchronous errors:
   - `ErrUnknownProcess` → MCP tool error (the process registration aged out between snapshot refreshes).
   - `ErrUnknownStartID` → MCP tool error.
   - `DataContractValidationError` → MCP tool error with the validation message.
   - broker-publish errors → MCP tool error.
4. **Wait** — call `gw.Wait(ctx, instanceID)` to block until the instance reaches terminal status. Returns an `*EvaluationResult`.
5. **Response shaping** — `opts.Response(ctx, result)` shapes the MCP tool result. The default returns a `map[string]any` snapshot of `result.Context` keyed by task id.
6. **Failure mapping** — `result.Status == Failed` → MCP tool error. `Cancelled` → MCP tool error. `Completed` → success.

For interactive flows where the process suspends on `SuspendUntilMessage`, the MCP server can use `gw.Subscribe(ctx, ...)` directly instead of `gw.Wait` and surface `EventMessageRequest` to the MCP client (e.g. via an MCP elicitation when supported). The v1 path uses `Wait` and treats suspension as opaque from the MCP client's perspective; this is documented as a future extension.

---

## Failure Mapping

| Outcome | MCP result |
|---|---|
| `result.Status == ProcessStatusCompleted` | success, `Response` value as content |
| `result.Status == ProcessStatusFailed` | tool error (`isError: true`) carrying the `NODE_FAILED` / `PROCESS_FAILED` details from `result.History` |
| `result.Status == ProcessStatusCancelled` | tool error (`isError: true`) carrying the cancel reason |
| `gw.Submit` returned `ErrUnknownProcess` / `ErrUnknownStartID` | tool error (`isError: true`) |
| `gw.Submit` returned `DataContractValidationError` | tool error (`isError: true`) carrying the validation message |
| `gw.Submit` returned a broker-publish error | tool error (`isError: true`) |
| `gw.Wait` returned `ctx.Err()` (client disconnected) | MCP cancellation, no result |
| `Input` returned an error | tool error (`isError: true`) |
| `Schema` returned an error during registration | `Run` fails to start; not surfaced as a per-call error |
| `describe_process` called with an unknown `(namespace, processId, version)` | tool error (`isError: true`) carrying `UnknownProcessError` |
| `resources/read` for an unknown URI | MCP resource-error response |

---

## Concurrency & Lifecycle

MCP stdio is single-channel; the underlying SDK serializes JSON-RPC reads. The SDK may dispatch multiple in-flight tool-call handlers concurrently, so the runtime is required to be safe under concurrent invocation. This holds because:

- The `BrokerGateway` interface is required to be safe for concurrent use, by contract.
- `Options` is captured by value at `Run` time and is not mutated.
- The cached `[]ProcessRegistration` snapshot is read under a mutex during refresh and during tool dispatch.

---

## Cold Start and Reuse

Construct the `BrokerGateway` and (if embedding) the `StateStore` once at process startup and pass them to `Run`. There is no warm-vs-cold distinction — the MCP server is a single long-running process.

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
    "blkit/mcpserver"

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

    err = mcpserver.Run(ctx, gw, mcpserver.Options{
        ServerName:    "lending-mcp",
        ServerVersion: "1.0.0",
        Schema: func(reg messagebroker.ProcessRegistration) (string, map[string]any, error) {
            desc := ""
            if reg.Description != nil {
                desc = *reg.Description
            }
            return desc, jsonSchemaFromRegistration(reg), nil
        },

        // Spawn the worker components inside this process.
        EmbeddedWorker: &mcpserver.EmbeddedWorkerOpts{
            StateStore: store,
            WorkerID:   hostname() + "-mcp",
        },
    })
    if err != nil {
        log.Fatalf("mcpserver: %v", err)
    }
}

// jsonSchemaFromRegistration is a project-local helper. The MCP runtime does
// not prescribe how schemas are derived — Schema is the integration point.
func jsonSchemaFromRegistration(reg messagebroker.ProcessRegistration) map[string]any {
    // Find the start event matching opts.StartID (defaults to "start") and
    // translate its InputContract into JSON Schema. Implementation omitted.
    return map[string]any{"type": "object", "additionalProperties": true}
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

    err = mcpserver.Run(ctx, gw, mcpserver.Options{
        ServerName:    "lending-mcp",
        ServerVersion: "1.0.0",
        Schema:        schemaFromRegistration,
        // No EmbeddedWorker. Tools are exposed only if a remote worker
        // pool has registered the corresponding processes with the broker.
    })
    if err != nil {
        log.Fatalf("mcpserver: %v", err)
    }
}
```

A client config (e.g. `claude_desktop_config.json`) points at the binary directly:

```json
{
  "mcpServers": {
    "lending": {
      "command": "/usr/local/bin/lending-mcp"
    }
  }
}
```

---

## Edge Cases

- The broker has no live workers when `Run` starts: `gw.ListAvailableProcesses` returns an empty slice and only `describe_process` is registered. Subsequent refresh polls pick up workers as they come online.
- An embedded worker fails `RegisterProcesses` at startup: `Run` returns the error without entering the MCP serve loop.
- `EmbeddedWorker.WorkerID` is empty: `Run` returns a `ValueError` before starting anything.
- A tool call is in flight when the worker's process registration ages out (stale on a refresh): the in-flight `gw.Wait` continues — the worker still has the instance — but new calls to that tool fail with `ErrUnknownProcess` until the worker re-registers.
- Multiple subscribers to the same instance: each gets the full event stream by default — see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md). The MCP server's `gw.Wait` is one such subscriber.
- `RegistryPollInterval == 0`: registry is queried once at startup. New processes are not picked up until restart.
- The `describe_process` tool bypasses the evaluation pipeline entirely — `Schema`, `StartID`, `Input`, `Response`, and `EmbeddedWorker` are not consulted.
- Concurrent tool calls share the cached `[]ProcessRegistration` snapshot. The snapshot is updated under a mutex on each refresh.
