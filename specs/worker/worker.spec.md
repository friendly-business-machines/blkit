---
name: Worker
description: A blocking goroutine loop that consumes commands from a BrokerGateway and runs each process to completion locally against a StateStore. Also handles broker-held registry lifecycle (register on startup, heartbeat, unregister on shutdown).
targets:
  - ../worker/worker.go
---

# Worker

A worker is a blocking call that registers its capability set with the broker, consumes routed [`Command`](../messagebroker/overview.spec.md)s from a [`BrokerGateway`](../messagebroker/overview.spec.md) — selectively, taking only those whose process is registered in the worker's binary — and hands each one to an executor goroutine. Each executor drives its process through its full execution lifecycle: evaluate the process graph and execute ready tasks — streaming `Transaction`s and `ExecutionStep`s to a [`StateStore`](../data/state-store.spec.md) via `WriteBatch` as work happens, and publishing `Event`s to the broker — until the process completes or suspends. The executor then persists the boundary metadata via `Save` and `Ack`s the consumed command. Up to `MaxConcurrent` executors run in parallel; the consume loop pulls new work as soon as a slot is free. This consume-and-execute cycle repeats for the life of the worker, stopping only when the `workerCtx` passed to `worker.Run` is cancelled.

Once a worker picks up a `Command`, that worker is responsible for the entire process: every task in the process graph runs in goroutines inside this worker, not as separate broker items handed to other workers. This is the long-running counterpart to the per-event handlers in [faas/overview.spec.md](../faas/overview.spec.md), where each invocation performs one evaluation and re-publishes any continuation work.

The worker is otherwise stateless — per-instance execution state is reconstructed from the `StateStore` on every pickup.

```go
func Run(workerCtx context.Context, gw messagebroker.BrokerGateway, store StateStore, opts Options) error {}

type Options struct {
    // Worker identity. Required. Must be unique per worker instance —
    // typically a UUID generated at process startup, or a pod name in
    // Kubernetes (e.g. hostname()). Used as the gw.RegisterProcesses key
    // so the broker can route registrations and heartbeats to this worker.
    WorkerID string

    // Heartbeat / registration
    HeartbeatInterval *time.Duration // min interval between gw.HeartbeatRegistrations calls; nil = 30s

    // Concurrency
    MaxConcurrent   *int           // max in-flight processes; nil = runtime.NumCPU()
    TaskConcurrency *int           // max concurrent task goroutines per process; nil = runtime.NumCPU()
    PollInterval    *time.Duration // implementation-specific pacing for ConsumeCommands; nil = 1 second

    // Writer pool (see Writer Pool section below)
    MaxWriters          *int           // max concurrent writer goroutines; nil = min(GOMAXPROCS, 8)
    MaxBatchSize        *int           // max WriteOps batched into one WriteBatch call; nil = 64
    MaxBatchWait        *time.Duration // max wait before flushing a partial batch; nil = 5ms
    IdleTimeout         *time.Duration // when idle writer goroutines exit; nil = 30s
    WriterChannelBuffer *int           // buffered channel capacity; nil = 1024
    WritePolicy         WritePolicy    // behaviour on write failure; default PolicyBuffer
}
```

The first parameter is named `workerCtx` rather than `ctx` to distinguish it from blkit's own `ExecutionContext` — `workerCtx` is a stdlib `context.Context` used purely as a cancellation signal for the worker's lifetime; it has no relationship to the per-process `ExecutionContext` data threaded through `Evaluate`.

`worker.Run` blocks until `workerCtx` is cancelled. It returns `nil` on a clean shutdown, or `context.DeadlineExceeded` / `context.Canceled` if `workerCtx` terminates while executors are still in flight.

---

## Components

A worker process has four concurrent actors. Together they keep work moving from the broker, through evaluation, to durable storage in the [`StateStore`](../data/state-store.spec.md).

- **Consume loop** — one goroutine, owned by the caller of `worker.Run`. Selectively reads `Command`s from `gw.ConsumeCommands(...)` (filtered to processes registered in this binary) and hands each to a fresh executor goroutine. Acquires a permit from a process semaphore before each read, so the worker only pulls work it can immediately execute. Stops when `workerCtx` is cancelled. See [Concurrency Model](#concurrency-model) for semaphore details.

- **Executor goroutines** — one goroutine per in-flight `Command`, spawned by the consume loop. Each executor drives its process through its full lifecycle: load state from the StateStore, call `process.Evaluate(...)`, publish `Event`s during/after, persist boundary metadata via `Save`, and ack/nack the command via `gw.AckCommand` / `gw.NackCommand`. Up to `MaxConcurrent` executors run in parallel.

- **Writer pool** — an elastic pool of goroutines spawned and owned by `worker.Run`. Drains queued `WriteOp`s that `Evaluate` and the executors produce — `OpRecordStep` for `ExecutionStep`s, `OpRecordTransaction` / `OpUpdateStatus` for `Transaction`s — batches them, and calls `StateStore.WriteBatch`. The producers (e.g. `history.Record(step)`, `context.Set(...)`) enqueue onto a buffered channel and move on; they do not block on durability. This is the path by which all per-event payload reaches the StateStore. The boundary `Save` call, by contrast, is made directly by the executor and bypasses the pool. See [Writer Pool](#writer-pool) for the full design.

- **Heartbeat goroutine** — one goroutine, owned by `worker.Run`. Calls `gw.HeartbeatRegistrations(workerCtx, opts.WorkerID)` every `HeartbeatInterval` to refresh this worker's TTL on the broker-held registry. Failures are retried with backoff. If heartbeats fail repeatedly such that the broker's TTL is at risk of expiring, the worker logs and continues — losing registry presence is recoverable on the next successful heartbeat.

The consume loop, writer pool, and heartbeat goroutine are singletons per `worker.Run` invocation; executor goroutines come and go with each `Command`. All four share the worker's `workerCtx` for shutdown signalling.

---

## The Process Registry

The blkit module exposes a **process registry** — a package-level variable in the blkit module, keyed by `(Namespace, ProcessID, Version)`. It is populated as a side effect of `NewProcess(...)` calls during package initialization. A worker binary's registry contents are therefore determined by **which packages are linked into the binary** — typically arranged by blank imports in `main`. The registry lives in the worker binary's memory; there is no shared in-memory registry across binaries. The namespace is derived from the calling package's import path, so processes from different packages cannot collide. See [process.spec.md](../processes/process.spec.md) for the full registration rules.

A worker uses its in-memory registry as its **capability set**: the set of processes this worker is permitted and able to execute. Two consequences follow:

- **Selective consumption.** When the worker calls `gw.ConsumeCommands(workerCtx, keys)`, it passes the set of currently registered keys, and the gateway returns only matching `Command`s. Work whose process is not registered in this binary is left on the broker for other workers to pick up.
- **Deployment shape determines routing.** Which processes a worker can run is determined entirely by which packages are linked into its binary. To dedicate a fleet of workers to a subset of processes, build a binary that imports only those packages.

The worker also publishes its capability set to the **broker-held registry** so producers (MCP servers, web servers, admin UIs) can discover what's available. See [Registration on startup](#registration-on-startup) below.

---

## Registration on startup

When `worker.Run` is called, before the consume loop starts:

1. Walk `blkit.AllProcesses()` to get the list of processes registered in this binary.
2. For each, build a `ProcessRegistration` (see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md)) with `Namespace` / `ProcessID` / `Version` / `Name` / `Description`, the `StartEvents` (with their `InputContract`s), `EndEvents` (with optional `OutputContract`s), `AllowExternalCancel` / `AllowExternalTerminate`, and `Markdown` rendered via `process.ToMarkdown()`.
3. Set `WorkerID` on each registration to `opts.WorkerID`.
4. Call `gw.RegisterProcesses(workerCtx, opts.WorkerID, regs)`.

If `RegisterProcesses` fails, `worker.Run` returns the error without starting the consume loop or the heartbeat goroutine.

After successful registration, the worker spawns the heartbeat goroutine (described in [Components](#components) above).

---

## Consume

The consume loop runs on the caller's goroutine — the one that called `worker.Run`. It is the single place new work enters the worker.

A process semaphore — `make(chan struct{}, MaxConcurrent)` — caps how many processes are executing simultaneously. The dispatch step **acquires a permit before launching the executor**, so the worker only spawns work it can immediately execute.

```go
// Conceptual outline (not the literal target API)
keys := registry.Keys() // ProcessKey for each process in blkit.AllProcesses()
cmds, err := gw.ConsumeCommands(workerCtx, keys)
if err != nil { return err }

sem := make(chan struct{}, *opts.MaxConcurrent)
for cmd := range cmds {
    sem <- struct{}{} // acquire process permit
    go func(c Command) {
        defer func() { <-sem }()
        if err := executeCommand(workerCtx, c, gw, store, opts); err != nil {
            gw.NackCommand(workerCtx, c, err)
            return
        }
        gw.AckCommand(workerCtx, c)
    }(cmd)
}
```

The selective consumption (filtering by `keys`) is done by the gateway implementation — the worker passes its capability set, and the gateway uses its broker's filtering primitive (Redis: stream-with-consumer-group filtering; NATS: JetStream subject filter; in-memory: filtered channel).

Multiple `worker.Run` calls — whether in the same Go process or across many machines — are supported. Each is an independent consumer of the broker's command stream; serialization between them is whatever the underlying broker provides.

---

## Execute

For each consumed `Command`, the consume loop spawns an executor goroutine. The executor uses a **run-to-completion** model: it drives the process through its entire lifecycle locally — evaluating the graph, executing tasks, advancing tokens — until the process completes or suspends. Within the executor, parallel branches in the process graph spawn task goroutines bounded by a per-process task semaphore — `make(chan struct{}, TaskConcurrency)`.

The executor dispatches by `Command.Kind`:

- **`CommandSubmit`** (initial run): call `store.NewExecutionState(process, NewExecutionStateOpts{StartId, Input})` using the gateway-supplied `InstanceID`, then call `process.Evaluate(ctx, history)`.
- **`CommandDeliverMessage`**: call `store.LoadExecutionState(instanceID)`, write the message payload into the execution context, then re-evaluate.
- **`CommandCancel`**: call `store.LoadExecutionState(instanceID)`, append a synthetic `CancelEvent` step, drive the instance to `Cancelled`.
- **`CommandTerminate`**: same shape as Cancel but with a synthetic `TerminateEvent` and final status `Completed`.
- **`CommandContinuation`**: call `store.LoadExecutionState(instanceID)`, then re-evaluate (resumes after a Suspend* event).

In each case:

1. **Resolve the process** in the worker's in-memory registry by `(Namespace, ProcessID, Version)`. By construction (selective consumption), a match always exists.
2. **Load (or create) state** as described above.
3. **Evaluate** — call `process.Evaluate(context, history)`. The graph-traversal semantics are entirely `Evaluate`'s responsibility (see [process.spec.md](../processes/process.spec.md)).

   Two write paths are used against the StateStore. **During** `Evaluate`, the worker's [writer pool](#writer-pool) streams per-event payload — `Transaction`s and `ExecutionStep`s — to the StateStore via `WriteBatch` as work happens. **After** `Evaluate` returns, the executor calls `store.Flush(processInstanceID)` to wait for any in-flight batches to be confirmed durable, then `store.Save(processInstanceID, history)` to persist the boundary metadata.

4. **Publish events** — during and after `Evaluate`, the executor calls `gw.PublishEvent(workerCtx, evt)` for each meaningful state transition: `StatusChange` on every status update, `NodeCompleted` after each task / decision, `MessageRequest` when reaching a `SuspendUntilMessage`, `Error` on task failure, and the final `Result` on terminal status.

5. **Ack / Nack the Command** — on success, call `gw.AckCommand(workerCtx, cmd)`. On failure (panic, broker-publish error), call `gw.NackCommand(workerCtx, cmd, err)`. The broker's redelivery policy then governs retry behavior.

6. **Continuation on suspension** — if `Evaluate` returns with `Status: SUSPENDED`, the executor publishes a `Continuation` command via the gateway (the implementation determines whether this is a separate `gw.Publish*` call or part of the event stream — see the per-broker specs). The continuation will be picked up by some worker (this one or another) when the awaited event arrives.

`Evaluate` is idempotent with respect to its input state: repeated calls with the same or a more recent history advance only from positions the history shows are genuinely ready, and produce no new work on already-completed or already-suspended state.

### Subprocesses

When `Evaluate` reaches a subprocess task it runs the subprocess inline by recursing into `Evaluate` on the subprocess definition (in a new goroutine if part of a parallel fanout). Context scoping and history isolation for subprocesses are described in [execution-context.spec.md](../data/execution-context.spec.md#sub-process-scoping).

### RequestInputTask: pause-to-suspend conversion

A [`RequestInputTask`](../processes/task-nodes.spec.md#requestinputtask) with `WaitMode == RequestInputPauseThenSuspend` starts as an in-memory pause and converts to a durable suspension if `PauseDuration` elapses without a `DeliverMessage` arriving. The executor implements the conversion as follows:

1. On entering the task, the executor arms a per-instance pause timer for `PauseDuration` and parks the goroutine on the same in-memory wait channel used by `PauseUntilMessage`.
2. If a `DeliverMessage` arrives before the timer fires: the channel delivers the payload, the pause timer is cancelled, and evaluation resumes inline.
3. If the timer fires before a `DeliverMessage` arrives: the executor cancels the in-memory wait, appends a `SUSPENSION_RECORDED` step to `ExecutionHistory` for this node (using the same shape as a `SuspendUntilMessageEvent` suspension), flushes the writer pool, calls `store.Save(processInstanceID, history)`, and lets `Evaluate` return with status `ProcessStatusSuspended`. The token's position does not change — only the wait substrate is swapped from in-memory to persisted.
4. The eventual `DeliverMessage` is then handled by the standard `CommandDeliverMessage` path: a worker (this one or another) loads state, writes the payload into the context, and resumes evaluation past the task.

A `RequestInputTask` running under `RequestInputSuspend` or `RequestInputPause` follows the existing `SuspendUntilMessage` / `PauseUntilMessage` paths respectively and does not exercise the conversion logic.

### Error handling

If a task raises an error during execution:

- `Evaluate` records a `NODE_FAILED` step and, if there is no error boundary event, sets process status to `FAILED` and returns. Other in-progress tasks in the same evaluation are cancelled.
- The executor publishes `Event{Kind: Error, Code: "PROCESS_FAILED"}` via `gw.PublishEvent`, persists the returned history via `store.Save(...)`, and acks the `Command`.

A panic inside an executor goroutine is recovered: the `Command` is `Nack`ed and the consume loop continues. A panic in the consume loop itself terminates `worker.Run`, which returns the panic as an error.

---

## Process Task

`ProcessTask` is the worker's internal record for *one evaluation of one instance*. It is created when the worker claims a `Command` from the gateway, advances through the `TaskStatus` lifecycle as the executor runs, and is discarded once the command is acked. `ProcessTask` is **not** transmitted over the broker and is **not** part of the `BrokerGateway` interface — producers describe work via `StartRequest` / `Command`, and the worker maintains `ProcessTask` independently for telemetry, history correlation, and concurrency accounting.

```go
type TaskStatus int

const (
    TaskStatusPending   TaskStatus = iota // claimed from the gateway, waiting for a per-process concurrency slot
    TaskStatusRunning                     // executor is currently evaluating
    TaskStatusCompleted                   // evaluation finished successfully
    TaskStatusFailed                      // evaluation failed
)

type ProcessTask struct {
    ProcessInstanceID               string         // unique id for this process instance
    ParentProcessInstanceID         *string        // immediate parent; nil for top-level processes
    UltimateParentProcessInstanceID *string        // top-level root; nil for top-level processes
    ExecutionID                     string         // unique id for this specific evaluation pass
    Status                          TaskStatus
    ProcessID                       string         // id of the process being evaluated
    ProcessVersion                  string         // version of the process being evaluated
    StartID                         *string        // start event id; set on initial submission, nil on re-evaluation
    Input                           map[string]any // initial input; set on initial submission, nil on re-evaluation
    PublishedTS                     time.Time      // when the start-command that produced this ProcessTask was published to the BrokerGateway
    ExecutionStartTS                *time.Time     // when ExecutionID started; nil while PENDING
    ExecutionFinishTS               *time.Time     // when ExecutionID finished; nil while PENDING/RUNNING
}
```

`ExecutionID` is the correlation key that ties a `ProcessTask` to its rows in `ExecutionHistory` — see [../data/execution-history.spec.md](../data/execution-history.spec.md). A single `ProcessInstanceID` can have many `ProcessTask`s over its lifetime (initial run + every continuation after suspension); each has a distinct `ExecutionID`.

---

## Graceful shutdown

When `workerCtx` is cancelled:

1. The consume loop stops accepting new commands (the channel from `gw.ConsumeCommands` closes when the gateway honours the ctx).
2. Wait for in-flight executors to finish (or for `workerCtx` to deadline-exceed).
3. Call `gw.UnregisterProcesses(workerCtx, opts.WorkerID)` so the broker's registry stops advertising this worker's processes immediately rather than waiting for the TTL to expire.
4. Stop the heartbeat goroutine.
5. Drain the writer pool — see [Lifecycle](#lifecycle) under Writer Pool.
6. Return.

If `workerCtx` carries a timeout that fires before unregister completes, the registration falls off the broker via TTL expiration — slower but no functional impact.

---

## Writer Pool

`ExecutionContext` and `ExecutionHistory` mutations (`ctx.Record`, `ctx.Commit`, `ctx.Abort`, `history.Record`) need to land in the `StateStore` durably without blocking the goroutines that produce them. `worker.Run` spawns an **elastic pool of writer goroutines** that owns this responsibility, sitting between the producers and the `StateStore`.

### Design

- **Producers** — the executor goroutine (during `Evaluate`) and any task goroutines it spawns. Each mutation:
  1. Updates the in-memory log (mutex-protected — assigns `CommitNumber`, sets `Timestamp`, appends).
  2. Constructs a `WriteOp` (see [state-store.spec.md](../data/state-store.spec.md)) and sends it on the worker's writer channel.
  3. Returns immediately.
- **Consumers** — writer goroutines spawned lazily on demand, capped at `MaxWriters`. When the channel has work and no idle worker is free, a new worker spawns up to the cap. Idle workers exit after `IdleTimeout`, so the pool naturally shrinks when load drops.
- **Batching** — each worker drains the channel into a batch capped by `MaxBatchSize` and `MaxBatchWait`, then issues a single `StateStore.WriteBatch` call.
- **No sharding** — every event is self-describing via `(Timestamp, CommitNumber)`, so any writer can write any event and arrival order at the backend does not matter.

### Configuration knobs

Set on `worker.Options`. All have sensible defaults; none are required.

| Knob | Default | Purpose |
|---|---|---|
| `MaxWriters` | `min(GOMAXPROCS, 8)` | Upper bound on concurrent writer goroutines. |
| `MaxBatchSize` | `64` | Maximum `WriteOp`s batched into one `WriteBatch` call. |
| `MaxBatchWait` | `5 ms` | How long a worker waits for additional ops before flushing a partial batch. |
| `IdleTimeout` | `30 s` | How long an idle worker waits before exiting. |
| `WriterChannelBuffer` | `1024` | Capacity of the buffered channel between producers and writers. |
| `WritePolicy` | `PolicyBuffer` | Behaviour on write failure (see below). |

### Write policy

```go
type WritePolicy int

const (
    PolicyBuffer WritePolicy = iota // buffer & retry; halt on bounded-buffer overflow
    PolicyDrop                      // drop on backend failure (lossy; observability use)
    PolicyHalt                      // first failure halts the affected process instance
)
```

- `PolicyBuffer` (default) — transient failures are retried with bounded backoff; sustained failures surface via the worker's error channel and eventually halt the affected process instance when the buffer overflows.
- `PolicyDrop` — failed writes are dropped silently. Suitable for observability sinks where data loss is acceptable.
- `PolicyHalt` — the first failure for a process instance transitions that instance to `FAILED` synchronously.

### Flush semantics

`StateStore.Flush(processInstanceID)` is a per-instance durability barrier. The writer pool implements it by enqueueing flush sentinels — one per active worker goroutine — and waiting for all of them to be acknowledged. When `Flush` returns, every `WriteOp` enqueued before the call (for the given instance) has been confirmed durable.

The executor calls `Flush` at:

- The end of an `Evaluate()` cycle, before persisting boundary metadata via `Save` and ack'ing the `Command`. This guarantees that when the command is acked, every per-event payload is already durable.
- After a root-scope `ctx.Commit` / `ctx.Abort`, so the outside world's view of the process is consistent with what's durable.

### Lifecycle

The writer pool is started lazily on the first `WriteOp` send and is bound to the lifetime of `worker.Run`. On `workerCtx` cancellation:

1. The consume loop stops, so no new `WriteOp`s will be produced once in-flight executors finish.
2. Each writer goroutine drains any remaining ops from the channel and flushes a final batch.
3. `worker.Run` waits for all writer goroutines to exit before returning.

If `workerCtx` carries a deadline that fires while writes are still in flight, the pool best-effort flushes what it can and returns; remaining buffered ops are lost.

---

## Example: Worker Binary

A complete `main.go` for a pure-worker binary. The processes the worker will run are pulled in via blank imports — that is the only mechanism that determines this binary's capability set.

`worker.Run` blocks for the life of the binary. There is no `Start` / `Shutdown` API — lifecycle is governed entirely by the `context.Context` you pass in.

```go
// cmd/worker/main.go
package main

import (
    "context"
    "log"
    "os"
    "os/signal"
    "syscall"

    "github.com/friendly-business-machines/blkit"
    "github.com/friendly-business-machines/blkit/messagebroker"
    "github.com/friendly-business-machines/blkit/worker"

    // Blank imports load process-defining packages so their NewProcess(...)
    // calls run during package init and write into blkit's in-memory registry.
    // The worker uses the registry as its capability set.
    _ "example.com/area/lendingflows/v1"
    _ "example.com/area/onboarding/v2"
)

func intPtr(n int) *int { return &n }

func main() {
    workerCtx, stopWorker := context.WithCancel(context.Background())
    defer stopWorker()

    go func() {
        sig := make(chan os.Signal, 1)
        signal.Notify(sig, syscall.SIGTERM, syscall.SIGINT)
        <-sig
        log.Println("shutdown signal received; draining in-flight processes")
        stopWorker()
    }()

    gw, err := messagebroker.NewRedisBrokerGateway(messagebroker.RedisOpts{
        Addr: os.Getenv("BLKIT_REDIS_ADDR"),
    })
    if err != nil {
        log.Fatal(err)
    }
    defer gw.Close()

    // StateStore is still owned by the worker — the gateway never touches it.
    stateStore := blkit.NewPostgresStateStore(
        os.Getenv("BLKIT_DB_URL"),
        "loan_app",
    )

    err = worker.Run(workerCtx, gw, stateStore, worker.Options{
        WorkerID:        os.Getenv("HOSTNAME"), // unique per worker instance
        MaxConcurrent:   intPtr(16),
        TaskConcurrency: intPtr(8),
    })
    if err != nil {
        log.Fatalf("worker exited: %v", err)
    }
}
```

For deployments that want the MCP server and the worker in the same binary, use `mcpserver.Run` with the `EmbeddedWorker` option instead of calling `worker.Run` directly — see [../mcp/mcp-server.spec.md](../mcp/mcp-server.spec.md).

From a single Go module you can produce as many different worker binaries as you want, each capable of running a different subset of processes. Add a `main` package per binary under `cmd/`, and have each one blank-import only the process packages that binary should be able to run. The registry contents of each binary determine which `Command`s its workers will pick up off the shared broker.

Each of these binaries can then be containerised and deployed independently, creating distinct pools of workers that each focus on executing their own subset of processes — all consuming from the same broker, with the gateway's selective-consumption routing the right work to the right pool.

---

## Example: Containerization & Deployment

A worker binary is a plain Go executable, so a minimal multi-stage `Dockerfile` is sufficient. Build statically, copy the binary into a distroless or `scratch` base, and run it as PID 1 — `worker.Run` already honours `SIGTERM` for graceful shutdown.

```dockerfile
# Dockerfile

FROM golang:1.23 AS build
WORKDIR /src
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build \
    -ldflags="-s -w" \
    -o /out/worker \
    ./cmd/worker

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /out/worker /worker
USER nonroot:nonroot
ENTRYPOINT ["/worker"]
```

Build and push:

```bash
docker build -t registry.example.com/blkit/worker:1.0.0 .
docker push    registry.example.com/blkit/worker:1.0.0
```

Deploy on Kubernetes as a `Deployment` — workers are stateless and horizontally scalable, so any replica count is valid. Set `terminationGracePeriodSeconds` longer than the longest expected in-flight process so the consume loop has time to drain after `SIGTERM`.

```yaml
# k8s/worker.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: blkit-worker
spec:
  replicas: 4
  selector:
    matchLabels: { app: blkit-worker }
  template:
    metadata:
      labels: { app: blkit-worker }
    spec:
      terminationGracePeriodSeconds: 600
      containers:
        - name: worker
          image: registry.example.com/blkit/worker:1.0.0
          env:
            - name: BLKIT_REDIS_ADDR
              valueFrom:
                secretKeyRef: { name: blkit-redis, key: addr }
            - name: BLKIT_DB_URL
              valueFrom:
                secretKeyRef: { name: blkit-db, key: url }
            - name: HOSTNAME
              valueFrom:
                fieldRef: { fieldPath: metadata.name }
          resources:
            requests: { cpu: "500m", memory: "256Mi" }
            limits:   { cpu: "2",    memory: "1Gi"  }
```

Operational notes:

- **Replicas.** Each pod runs one `worker.Run` loop. Multiple pods are independent consumers; serialization is whatever the underlying broker provides.
- **Worker IDs.** Use `metadata.name` (the pod name) as `WorkerID` — Kubernetes guarantees pod names are unique per namespace.
- **Graceful shutdown.** Kubernetes sends `SIGTERM` then waits up to `terminationGracePeriodSeconds` before `SIGKILL`. Size this window to exceed your longest expected in-flight process.
- **Routing by deployment.** To run different process subsets on different node pools, build a separate worker binary per subset and deploy each as its own `Deployment`.
- **Health checks.** A `worker.Run` loop has no HTTP surface; rely on the process being alive (`livenessProbe` of type `exec` against the binary, or omit the probe entirely).

---

## Edge Cases

- An empty registry is valid: `gw.RegisterProcesses` is called with an empty registration list, `gw.ConsumeCommands` is called with an empty key set, and the consume loop idles until `workerCtx` is cancelled.
- `WorkerID` empty produces a `ValueError`.
- `MaxConcurrent <= 0`, `TaskConcurrency <= 0`, `PollInterval <= 0`, or `HeartbeatInterval <= 0` produce a `ValueError`.
- `MaxWriters <= 0`, `MaxBatchSize <= 0`, `WriterChannelBuffer <= 0`, `MaxBatchWait < 0`, or `IdleTimeout < 0` produce a `ValueError`.
- An unknown `WritePolicy` value produces a `ValueError`.
- If the writer channel fills (producers outpace writers because the backend is slow or unreachable), producer behaviour follows `WritePolicy`.
- A process that suspends publishes a continuation command (mechanism is gateway-impl-specific) and acks the current `Command`. The continuation may be picked up by any worker subscribed to the same broker.
- When `Evaluate` returns no ready tasks but the process has not completed (e.g. a join still waiting on parallel branches), the executor waits for those in-flight tasks rather than re-publishing.
- A panic inside an executor goroutine is recovered; the `Command` is `Nack`ed and the consume loop continues. A panic in the consume loop terminates `worker.Run`.
- Multiple `worker.Run` calls — within the same Go process or across multiple processes/machines — are supported. Each is an independent consumer of the broker. Serialization is whatever the underlying broker provides.
- `worker.Run` does not own or modify the `Process` graph — it is read-only during execution.
- The worker registers processes with the broker on startup but does **not** call `NewProcess(...)` itself; in-process registration happens at package init time (via blank imports of process-defining packages from `main`).
- See [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md) and [state-store.spec.md](../data/state-store.spec.md) for component-specific edge cases.
