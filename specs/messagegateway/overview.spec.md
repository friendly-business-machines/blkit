---
name: MessageGateway Overview
description: The unified broker abstraction for blkit. The MessageGateway interface presents a producer-side API (Submit, Cancel, Terminate, RespondToInputRequest, SubscribeToInstance, SubscribeToProcessRegistry) and a worker-side API (RegisterProcesses, Heartbeat, Unregister, FetchJobs, ReenqueueSuspended, PostError, MarkRunning, MarkCompleted, MarkFailed, MarkCancelled). Three implementations — Redis/Valkey, NATS, in-memory — share this overview. Implementations use broker-native primitives directly.
targets:
  - ../messagegateway/gateway.go
---

# MessageGateway Overview

`messagegateway.MessageGateway` is blkit's unified abstraction over the message broker / queue. It presents two role-specific groups of methods on a single interface:

- **Producer-side** — used by MCP servers, web servers, CLI tools, and admin UIs to submit new process runs, respond to a process's request for input, cancel or terminate, and subscribe to events.
- **Worker-side** — used by `worker.Run` to register the worker's process capability set, refresh its TTL, fetch jobs, re-enqueue suspended instances, post error messages, and mark instances as Running / Completed / Failed / Cancelled.

The same implementation satisfies both roles. Whether a given binary uses producer-side methods, worker-side methods, or both is determined by which entry-point functions it calls (`mcpserver.Run`, `worker.Run`, etc.) — not by which gateway type it constructs.

The gateway interacts **only with the broker**, not with the state store. Workers own state-store access exclusively. State queries (admin UIs, audit) go through the `StateStore` directly, separately from this interface (see [../data/state-store.spec.md](../data/state-store.spec.md)).

```
   Producers (MCP, web, CLI)               Workers
            │                                 │
            ▼                                 ▼
     ┌──────────────────────────┐
     │      MessageGateway       │ ── (broker-native primitives:
     │ (producer + worker side) │     Redis Streams/Pub-Sub/KV;
     │                          │     NATS JetStream/Core/KV;
     │                          │     Azure Service Bus + Table/Cosmos;
     │                          │     Google Pub/Sub + Firestore;
     │                          │     in-memory Go channels + maps)
     └──────────────────────────┘                        │
                                                         │ workers also use
                                                         ▼
                                                  ┌────────────┐
                                                  │ StateStore │
                                                  └────────────┘
```

`MessageGateway` implementations use their broker's native primitives directly.

---

## Implementations

The `MessageGateway` interface has five implementations, each in its own per-broker spec:

- **`RedisMessageGateway`** — Redis or Valkey backend, see [redis.spec.md](redis.spec.md).
- **`NATSMessageGateway`** — NATS + JetStream, see [nats.spec.md](nats.spec.md).
- **`AzureServiceBusMessageGateway`** — Azure Service Bus, see [azure-service-bus.spec.md](azure-service-bus.spec.md).
- **`GooglePubSubMessageGateway`** — Google Cloud Pub/Sub, see [google-pubsub.spec.md](google-pubsub.spec.md).
- **`InMemoryMessageGateway`** — single-process, no external dependencies; suitable for tests and small deployments. See [in-memory.spec.md](in-memory.spec.md).

All five implement the same `MessageGateway` interface defined below. Per-broker specs describe how the abstract operations map to broker primitives and the per-broker configuration shape.

The Azure Service Bus and Google Pub/Sub implementations target the same `MessageGateway` surface but their hosted-broker substrates lack a native key-value store. Both pair the message-bus primitives with a pluggable `RegistryStore` interface (typically Azure Table Storage / Cosmos DB on Azure, Firestore on GCP) for the registry snapshot and per-instance status records — see the respective specs for the trade-offs.

---

## Interface

Each verb names a single action. There are no tagged-union ack/publish methods — every status transition and every error post has its own verb.

```go
package messagegateway

type MessageGateway interface {
    // ===== Producer-side =====

    // Submit a new process run. The gateway generates the ProcessInstanceID
    // client-side and returns it immediately after the start job has been
    // published. The worker that picks the job up creates the persisted
    // state and runs the process.
    //
    // Synchronous errors:
    //   ErrUnknownProcess           — (Namespace, ProcessID, Version) not in the registry
    //   ErrUnknownStartID           — StartID does not match any StartEvent on the process
    //   DataContractValidationError — Input fails the StartEvent's InputContract
    //   broker-publish errors       — connection refused, auth failure, etc.
    //
    // Errors that surface only after a worker picks the job up
    // (e.g. process panic at evaluation start) are delivered as an
    // InstanceEvent of kind Error on SubscribeToInstance.
    Submit(ctx context.Context, req StartRequest) (instanceID string, err error)

    // Smart cancel. Behavior depends on where the instance currently sits:
    //   - Still pending in the queue (no worker has picked it up): remove
    //     from the queue. Opt-in is NOT required for queue-side removal.
    //     Returns nil.
    //   - A worker is already processing it: post a cancel instruction to
    //     the instance's topic for the worker to honor. Requires the
    //     process to have AllowExternalCancel; otherwise returns
    //     ErrCancelNotAllowed. Returns nil on successful publish.
    //   - The instance is already Completed: returns ErrAlreadyCompleted.
    //   - The instance is already Cancelled: returns ErrAlreadyCancelled.
    //   - The instance is already Failed:    returns ErrAlreadyFailed.
    //
    // Other synchronous errors: ErrUnknownProcess, broker-publish errors.
    Cancel(ctx context.Context, req CancelRequest) error

    // Post a terminate instruction to the instance's topic. Hard stop:
    // the worker drives the instance to ProcessStatusCancelled via the
    // graph's TerminateEvent if the process defines one, otherwise marks
    // it terminated without graceful unwind.
    //
    // Requires AllowExternalTerminate on the process.
    //
    // Synchronous errors: ErrUnknownProcess, ErrTerminateNotAllowed,
    // ErrAlreadyCompleted, ErrAlreadyCancelled, ErrAlreadyFailed,
    // broker-publish errors.
    Terminate(ctx context.Context, req TerminateRequest) error

    // Respond to a RequestInputTask. The process explicitly executed a
    // RequestInputTask node and is now waiting for input matching the
    // task's ResponseContract; this verb delivers that input keyed by
    // the requestID emitted on the instance's topic when the task fired.
    //
    // Synchronous errors: only broker-publish errors. Whether the
    // instance exists or is waiting on the given requestID is not
    // checked at the gateway — those errors flow back as an
    // InstanceEvent of kind Error.
    RespondToInputRequest(ctx context.Context, instanceID, requestID string, payload map[string]any) error

    // Subscribe to events for a single process instance. Returns a
    // channel that closes when the instance enters a finished status
    // (Completed, Cancelled, or Failed) or when ctx is cancelled.
    //
    // Synchronous errors: broker-subscribe errors only. INSTANCE_NOT_FOUND
    // flows back as the first InstanceEvent if no such instance has any
    // history visible to the broker.
    SubscribeToInstance(ctx context.Context, instanceID string) (<-chan InstanceEvent, error)

    // Subscribe to the process registry. The first message on the channel
    // is a full snapshot of currently-live registrations; subsequent
    // messages are incremental changes (added / removed / heartbeat-loss).
    // The channel closes only on ctx cancellation.
    //
    // Use this to populate and maintain a local cache so a REST or MCP
    // server can pre-check whether an incoming request targets a process
    // that some live worker is registered to execute.
    SubscribeToProcessRegistry(ctx context.Context) (<-chan RegistryUpdate, error)

    // ===== Worker-side: registration lifecycle =====

    // Register this worker's capability set. Each call replaces any prior
    // registration set for the same workerID. Idempotent. The TTL is set
    // internally by the implementation (typically tied to the heartbeat
    // interval × 3).
    RegisterProcesses(ctx context.Context, workerID string, regs []ProcessRegistration) error

    // Refresh the TTL on this worker's registrations. Called periodically
    // by the worker's heartbeat goroutine.
    //
    // Synchronous errors: ErrUnknownWorker, broker-publish errors.
    Heartbeat(ctx context.Context, workerID string) error

    // Remove this worker's registrations. Called on graceful shutdown so
    // the broker stops advertising processes the worker can no longer
    // service.
    //
    // Synchronous errors: ErrUnknownWorker, broker-publish errors.
    Unregister(ctx context.Context, workerID string) error

    // ===== Worker-side: job queue =====

    // Fetch jobs from the queue for the given process keys. Returns a
    // channel that yields jobs the worker should dispatch to executors.
    // Each Job carries its kind (Start / RespondToInput / Cancel /
    // Terminate / Resume) and the payload the worker needs.
    //
    // The broker keeps a job in-flight from the moment it is delivered
    // until the worker either:
    //   - calls MarkCompleted / MarkCancelled / MarkFailed for the
    //     instance (terminal outcome); or
    //   - calls ReenqueueSuspended for the instance (the process
    //     suspended and should be picked back up later).
    //
    // If the worker dies before any of these, the broker times out the
    // in-flight slot and redelivers the job to another worker.
    //
    // Closes when ctx is cancelled.
    FetchJobs(ctx context.Context, keys []ProcessKey) (<-chan Job, error)

    // Re-enqueue a process whose evaluation suspended. Called when the
    // worker reaches a Suspend* / Pause* event or executes a
    // RequestInputTask. The instance leaves in-flight and becomes
    // eligible to be re-fetched when its wait condition is satisfied
    // (duration elapsed, datetime reached, RespondToInputRequest
    // delivered).
    //
    // Synchronous errors: broker-publish errors.
    ReenqueueSuspended(ctx context.Context, instanceID string) error

    // ===== Worker-side: status + topic =====

    // Post an error message to the instance's topic. Visible to
    // subscribers as an InstanceEvent of kind Error. Does NOT by itself
    // change instance status — use MarkFailed for a terminal failure.
    //
    // Synchronous errors: broker-publish errors.
    PostError(ctx context.Context, instanceID string, err InstanceError) error

    // Status transitions. Each verb publishes a StatusChange event on
    // the instance's topic and updates the broker-held status record so
    // Cancel/Terminate can detect already-finished instances.
    //
    // Synchronous errors for all four: broker-publish errors.

    MarkRunning(ctx context.Context, instanceID string) error
    MarkCompleted(ctx context.Context, instanceID string, result EvaluationResult) error
    MarkFailed(ctx context.Context, instanceID string, err InstanceError) error
    MarkCancelled(ctx context.Context, instanceID string) error
}
```

### Request types

```go
type StartRequest struct {
    Namespace string         // package import path of the registered process
    ProcessID string         // the registered process's Id
    Version   string         // the registered process's Version
    StartID   string         // which StartEvent to use as the entrypoint
    Input     map[string]any // input variables; validated against the StartEvent's InputContract before publish

    // Optional client-side correlation key recorded in every InstanceEvent
    // emitted for this instance. Use to tie the instance back to a
    // request-id / user-id / etc. on the client side.
    CorrelationKey *string
}

type CancelRequest struct {
    Namespace string  // (Namespace, ProcessID, Version) of the target instance —
    ProcessID string  // needed so the gateway can check the process's
    Version   string  // AllowExternalCancel flag in-process

    InstanceID string
    Reason     *string
}

type TerminateRequest struct {
    Namespace string  // (Namespace, ProcessID, Version) of the target instance —
    ProcessID string  // needed so the gateway can check the process's
    Version   string  // AllowExternalTerminate flag in-process

    InstanceID string
    Reason     *string
}
```

### Registry types

```go
type ProcessRegistration struct {
    Namespace   string
    ProcessID   string
    Version     string
    Name        *string
    Description *string

    // Boundary surface — what producers need to construct a Submit
    StartEvents []StartEventInfo
    EndEvents   []EndEventInfo

    // Operation hints — what the MCP / web UI can offer for this process
    AllowExternalCancel    bool
    AllowExternalTerminate bool

    // Pre-rendered markdown for tools like the MCP describe_process built-in
    Markdown string

    // For observability
    WorkerID      string    // set by the worker on RegisterProcesses
    RegisteredAt  time.Time // set by the broker on first RegisterProcesses
    LastHeartbeat time.Time // set by the broker on each Heartbeat / Register
}

type StartEventInfo struct {
    Id            string
    Name          string
    InputContract *InputContract // see ../data/data-contract.spec.md
}

type EndEventInfo struct {
    Id       string
    Name     string
    Contract *OutputContract // optional
}

type ProcessKey struct {
    Namespace string
    ProcessID string
    Version   string
}

type RegistryUpdate struct {
    Kind         RegistryUpdateKind
    Registration *ProcessRegistration // present for all kinds
}

type RegistryUpdateKind int

const (
    // First batch delivered on SubscribeToProcessRegistry — one
    // RegistryUpdateSnapshot per currently-live registration, then a
    // RegistryUpdateSnapshotComplete sentinel with nil Registration.
    RegistryUpdateSnapshot RegistryUpdateKind = iota
    RegistryUpdateSnapshotComplete
    RegistryUpdateAdded         // a worker freshly registered this process
    RegistryUpdateRemoved       // a worker called Unregister
    RegistryUpdateHeartbeatLost // TTL expired without a Heartbeat
)
```

### Job types (worker-side)

`Job` is a tagged union of broker-published work items the worker dispatches to executors.

```go
type Job struct {
    Kind JobKind
    Key  ProcessKey // routing key

    // Kind-specific payload. Exactly one is set per Job.
    Start            *StartJob
    RespondToInput   *RespondToInputJob
    Cancel           *CancelJob
    Terminate        *TerminateJob
    Resume           *ResumeJob // re-evaluation after a Suspend* / Pause* / RequestInputTask
}

type JobKind int

const (
    JobStart JobKind = iota
    JobRespondToInput
    JobCancel
    JobTerminate
    JobResume
)

type StartJob struct {
    InstanceID     string  // gateway-generated UUID
    StartID        string
    Input          map[string]any
    CorrelationKey *string
}

type RespondToInputJob struct {
    InstanceID string
    RequestID  string
    Payload    map[string]any
}

type CancelJob struct {
    InstanceID string
    Reason     *string
}

type TerminateJob struct {
    InstanceID string
    Reason     *string
}

type ResumeJob struct {
    InstanceID string
}
```

### Event types

`InstanceEvent` is what `SubscribeToInstance` delivers. It is a tagged union covering status changes, input requests, node completions, errors, and final results.

```go
type InstanceEvent struct {
    InstanceID     string
    CorrelationKey *string             // mirrored from StartRequest if set
    Kind           InstanceEventKind
    OccurredAt     time.Time

    // Kind-specific payloads; only one is set per InstanceEvent.
    StatusChange    *StatusChange
    InputRequest    *InputRequest     // RequestInputTask fired; carries the requestID
    NodeCompleted   *NodeCompleted
    Error           *InstanceError
    Result          *EvaluationResult // emitted as the channel closes
}

type InstanceEventKind int

const (
    InstanceEventStatusChange InstanceEventKind = iota
    InstanceEventInputRequest
    InstanceEventNodeCompleted
    InstanceEventError
    InstanceEventResult
)

type StatusChange struct {
    From, To ProcessStatus // see ../processes/process.spec.md
}

type InputRequest struct {
    NodeID     string
    RequestID  string         // pass to RespondToInputRequest

    // Request body. Set by RequestInputTask to describe what is being
    // asked of the responder (form schema, prompt text, current case state).
    Payload    map[string]any
}

type NodeCompleted struct {
    NodeID   string
    NodeKind string // "Task", "DecisionTask", "EndEvent", etc.
    Outputs  map[string]any
}

type InstanceError struct {
    Code    string // see "InstanceError codes" below
    Message string
}
```

---

## Producer-side operation flows

### `Submit`

1. Look up the process via the in-memory registry: `bl.LookupProcess(Namespace, ProcessID, Version)`. Missing → `ErrUnknownProcess`.
2. Find the `StartEvent` matching `StartID`. Missing → `ErrUnknownStartID`.
3. Validate `Input` against the StartEvent's `InputContract` (see [../data/data-contract.spec.md](../data/data-contract.spec.md)). Failure → `DataContractValidationError`.
4. Generate a `ProcessInstanceID` (UUIDv7 or similar).
5. Publish a `JobStart` on the broker carrying `(ProcessInstanceID, StartRequest)`. Broker error → returned synchronously.
6. Return `ProcessInstanceID`.

The worker that fetches the `JobStart` creates persisted state via `StateStore.NewExecutionState(...)` using the gateway-supplied `ProcessInstanceID`, calls `MarkRunning`, begins evaluation, and publishes further events via the worker-side verbs.

If the broker delivers the same `JobStart` twice (at-least-once semantics), the worker treats the second arrival as a no-op — the `ProcessInstanceID` already has persisted state.

### `RespondToInputRequest`

1. Publish a `JobRespondToInput` on the broker carrying `(instanceID, requestID, payload)`. Broker error → returned synchronously.
2. Return.

The worker that holds (or fetches) the suspended instance loads state, confirms it is waiting on the given `requestID`, validates the payload against the `RequestInputTask`'s `ResponseContract`, and resumes evaluation. If state is missing or the requestID doesn't match, the worker calls `PostError` with `Code: "INSTANCE_NOT_FOUND"` or `"NOT_WAITING"`.

### `Cancel`

1. Look up the process via `bl.LookupProcess(...)`. Missing → `ErrUnknownProcess`.
2. Inspect the broker-held status record for the instance:
   - Already `Completed` → `ErrAlreadyCompleted`. `Cancelled` → `ErrAlreadyCancelled`. `Failed` → `ErrAlreadyFailed`.
   - `Pending` (not yet picked up by a worker): atomically remove the `JobStart` from the queue. Return nil. (No opt-in check — pre-execution cancellation is always allowed.)
   - `Running` or `Suspended`: check `AllowExternalCancel`. Not opted in → `ErrCancelNotAllowed`. Otherwise publish a `JobCancel` and return nil.

A worker handling a `JobCancel` appends a synthetic `CancelEvent` to history, drives the instance to `ProcessStatusCancelled`, and calls `MarkCancelled`.

### `Terminate`

1. Look up the process via `bl.LookupProcess(...)`. Missing → `ErrUnknownProcess`.
2. Inspect the broker-held status record:
   - Already `Completed` / `Cancelled` / `Failed` → corresponding `ErrAlready*`.
3. Check `AllowExternalTerminate`. Not opted in → `ErrTerminateNotAllowed`.
4. Publish a `JobTerminate`. Broker error → returned synchronously.
5. Return.

A worker handling a `JobTerminate` drives the instance to terminal status (via `TerminateEvent` if the process defines one) and calls `MarkCancelled` (terminate is a stronger form of cancel from the status perspective; processes that need to distinguish do so via the synthetic event in history, not via status).

### `SubscribeToProcessRegistry`

The first messages on the returned channel are a snapshot — one `RegistryUpdateSnapshot` per currently-live registration — followed by a single `RegistryUpdateSnapshotComplete` sentinel. After the sentinel, the channel delivers incremental updates as workers register, unregister, or have their TTL expire.

Consumers maintain a local map keyed by `(Namespace, ProcessID, Version)`. A REST or MCP server uses this map to pre-check whether an incoming request targets a process that some live worker is currently registered to execute.

The implementation queries the broker's KV / metadata store for the snapshot and watches the broker's change feed (Redis Pub-Sub channel, NATS KV watcher, in-memory channel) for updates.

---

## Worker-side operation flows

### `RegisterProcesses` / `Heartbeat` / `Unregister`

The worker calls `RegisterProcesses` once on startup with one `ProcessRegistration` per process in `bl.AllProcesses()`. The gateway stores them in the broker's registry KV with a TTL.

`Heartbeat` is called by a worker-owned heartbeat goroutine on a configurable interval (default 30s). It refreshes the TTL on every registration this `workerID` published. If the worker stops heartbeating (crash, network partition), entries expire and a `RegistryUpdateHeartbeatLost` is delivered to registry subscribers.

`Unregister` is called on graceful shutdown to remove the entries immediately rather than waiting for TTL expiry.

The worker also calls `RegisterProcesses` again after every successful change to its capability set (rare in practice — it would require dynamic process loading, which blkit does not currently support).

### `FetchJobs`

`FetchJobs(ctx, keys)` returns a channel of `Job`s targeted at processes in `keys`. The implementation handles the broker-specific selective-consumption mechanism (Redis: stream-with-consumer-group filtering; NATS: JetStream subject filter; in-memory: filtered channel).

For each `Job` received, the worker dispatches it to an executor goroutine. The broker holds the job in-flight until the worker calls one of the outcome verbs:

- `MarkCompleted(instanceID, result)` — the process reached an `EndEvent`. Implicitly acks the job.
- `MarkCancelled(instanceID)` — the process reached a `CancelEvent` (whether triggered internally or by an external `JobCancel` / `JobTerminate`). Implicitly acks the job.
- `MarkFailed(instanceID, err)` — terminal failure. Implicitly acks the job.
- `ReenqueueSuspended(instanceID)` — the process suspended (Suspend*/Pause* event or RequestInputTask). The job is removed from in-flight; a new `JobResume` will be delivered when the wait condition is satisfied.

If the worker dies before any of these, the broker times out the in-flight slot (per-impl configurable; default 5× heartbeat interval) and redelivers to another worker. There is no explicit Ack/Nack — outcome verbs and the timeout cover the same cases.

### `MarkRunning`

The first verb a worker calls after fetching a `JobStart` (or the first `JobResume` after re-enqueue). Publishes `StatusChange{From: Pending|Suspended, To: Running}` and updates the broker-held status record. `MarkRunning` does NOT ack the job — only the terminal Mark* verbs and `ReenqueueSuspended` do.

### `PostError`

Workers call `PostError` to surface a non-terminal error to subscribers: a transient task failure that the retry policy will handle, an invalid response to a `RespondToInputRequest` (`NOT_WAITING`), etc. Status does not change. If the error is terminal, the worker calls `MarkFailed` instead — which also publishes an Error event.

### `MarkRunning` / `MarkCompleted` / `MarkFailed` / `MarkCancelled` — status semantics

| Verb | Resulting status | Channel-close on SubscribeToInstance? |
|---|---|---|
| `MarkRunning` | `ProcessStatusRunning` | No |
| `MarkCompleted` | `ProcessStatusCompleted` | Yes, after the `InstanceEventResult` is delivered |
| `MarkCancelled` | `ProcessStatusCancelled` | Yes, after the `StatusChange` (no Result event) |
| `MarkFailed` | `ProcessStatusFailed` | Yes, after the `InstanceEventError` |

The broker maintains a status record per instance so that `Cancel` and `Terminate` can detect already-finished instances and return the appropriate `ErrAlready*`.

---

## `SubscribeToInstance` and event delivery

Push-only. Implementations open a long-lived subscription on the broker and forward events to the returned channel.

```go
ch, err := gw.SubscribeToInstance(ctx, instanceID)
for evt := range ch {
    switch evt.Kind {
    case messagegateway.InstanceEventInputRequest:
        // surface to UI; eventually call gw.RespondToInputRequest(...)
    case messagegateway.InstanceEventResult:
        // channel closes after this
    case messagegateway.InstanceEventError:
        log.Printf("error: code=%s message=%s", evt.Error.Code, evt.Error.Message)
    }
}
```

The channel closes when:

- The context is cancelled, **or**
- The instance has reached `ProcessStatusCompleted` / `Cancelled` / `Failed` and the corresponding final event has been delivered.

### Backpressure

If a subscriber's channel buffer fills, events are **dropped** rather than blocking the broker reader. When the buffer recovers, a synthetic `InstanceEvent{Kind: Error, Error.Code: "BACKPRESSURE_DROP"}` is delivered. Per-implementation specs may override this default.

### Fan-out

Multiple subscribers to the same instance: each gets the full event stream by default (broadcast). Per-implementation specs may add a queue-group mode where events are load-balanced across subscribers.

---

## `ProcessOpts` opt-ins

`Cancel` (when the instance is `Running` or `Suspended`) and `Terminate` require the process author to opt in. See [../processes/process.spec.md](../processes/process.spec.md):

```go
type ProcessOpts struct {
    // ... existing fields ...

    AllowExternalCancel    bool // default false; required for MessageGateway.Cancel on a running/suspended instance
    AllowExternalTerminate bool // default false; required for MessageGateway.Terminate
}
```

Queue-side cancellation (`Cancel` of a `Pending` instance that no worker has picked up) does NOT require opt-in — the process author can't reasonably forbid cancelling a run that never started.

---

## Error model

### Synchronous errors

| Error | Returned from | When |
|---|---|---|
| `ErrUnknownProcess` | `Submit`, `Cancel`, `Terminate` | `(Namespace, ProcessID, Version)` not in the in-process registry |
| `ErrUnknownStartID` | `Submit` | `StartID` does not match any `StartEvent` on the resolved process |
| `DataContractValidationError` | `Submit` | `Input` fails the StartEvent's `InputContract` |
| `ErrCancelNotAllowed` | `Cancel` (when instance is Running/Suspended) | Process has `AllowExternalCancel: false` |
| `ErrTerminateNotAllowed` | `Terminate` | Process has `AllowExternalTerminate: false` |
| `ErrAlreadyCompleted` | `Cancel`, `Terminate` | Instance is already `ProcessStatusCompleted` |
| `ErrAlreadyCancelled` | `Cancel`, `Terminate` | Instance is already `ProcessStatusCancelled` |
| `ErrAlreadyFailed` | `Cancel`, `Terminate` | Instance is already `ProcessStatusFailed` |
| `ErrUnknownWorker` | `Heartbeat`, `Unregister` | `workerID` not currently registered |
| broker-publish errors | All publish ops | Connection refused, auth failure, etc. |

### Asynchronous errors (via `InstanceEvent{Kind: Error}` on `SubscribeToInstance`)

Posted by workers via `PostError` (non-terminal) or `MarkFailed` (terminal).

| `InstanceError.Code` | Source |
|---|---|
| `INSTANCE_NOT_FOUND` | Worker handling a job for an unknown `instanceID` |
| `NOT_WAITING` | `RespondToInputRequest` for an instance not waiting on the given `requestID` |
| `ALREADY_INTERRUPTING` | `Cancel` / `Terminate` job arrives while a prior interrupt is still being processed |
| `TASK_FAILED` | A non-terminal task error during evaluation (retry policy applies) |
| `PROCESS_FAILED` | The instance reached `ProcessStatusFailed` — paired with `MarkFailed` |
| `BACKPRESSURE_DROP` | Subscriber's buffer overflowed; events were dropped (synthesized by the gateway) |

---

## End-to-end example

A web server submits a loan-application process, surfaces a `RequestInputTask` for human approval, and delivers the response back. The worker pool runs separately (in a different binary, or in-process via `restserver.EmbeddedWorker` / `mcpserver.EmbeddedWorker`).

```go
package main

import (
    "context"
    "log"

    bl "github.com/friendly-business-machines/blkit/core"
    "github.com/friendly-business-machines/blkit/messagegateway"

    _ "example.com/processes/lending" // blank import registers the process via bl.NewProcess()
)

func main() {
    gw, err := messagegateway.NewRedisMessageGateway(messagegateway.RedisOpts{
        Addr: "localhost:6379",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer gw.Close()

    ctx := context.Background()

    instanceID, err := gw.Submit(ctx, messagegateway.StartRequest{
        Namespace: "example.com/processes/lending",
        ProcessID: "loan-application",
        Version:   "1.0",
        StartID:   "start",
        Input: map[string]any{
            "applicant":   applicantPayload,
            "loan_amount": 250000,
        },
    })
    if err != nil {
        log.Fatalf("submit: %v", err)
    }

    events, err := gw.SubscribeToInstance(ctx, instanceID)
    if err != nil {
        log.Fatalf("subscribe: %v", err)
    }

    for evt := range events {
        switch evt.Kind {
        case messagegateway.InstanceEventInputRequest:
            response := promptUserForApproval(ctx, evt.InputRequest)
            if err := gw.RespondToInputRequest(ctx, instanceID, evt.InputRequest.RequestID, response); err != nil {
                log.Printf("respond: %v", err)
            }

        case messagegateway.InstanceEventResult:
            log.Printf("done: status=%v outputs=%v", evt.Result.Status, evt.Result.Context)

        case messagegateway.InstanceEventError:
            log.Printf("error: code=%s message=%s", evt.Error.Code, evt.Error.Message)
        }
    }
}
```

---

## Edge cases

- The gateway and the worker pool may be different binaries, but both must import the package that defines the process. The gateway uses the in-process registry for synchronous validation on `Submit`. The worker uses it for capability-set filtering on `FetchJobs`.
- A `Cancel` racing with a graph-driven terminal event: whichever the worker observes first wins. If the cancel-job arrives after the instance is already finished, the worker posts `InstanceError{Code: "ALREADY_INTERRUPTING"}` (or the gateway returns the appropriate `ErrAlready*` synchronously if the broker-held status record was updated first).
- `Terminate` after `Cancel` already requested but not yet processed: implementations may accept (terminate overrides; "harder" wins) or reject with `ALREADY_INTERRUPTING`. The overview spec recommends accepting; per-impl specs document their choice.
- `SubscribeToInstance` on an instance that has already finished: whether the gateway can replay the cached final `InstanceEventResult` (or terminal `StatusChange`) depends on the broker's retention policy. Per-impl specs document the behavior.
- A worker that crashes silently leaves stale registrations until TTL expiry — typically `heartbeat_interval × 3`. `SubscribeToProcessRegistry` delivers `RegistryUpdateHeartbeatLost` when this happens.
- Multiple workers register the same `(Namespace, ProcessID, Version)`: the registry holds one `ProcessRegistration` per worker. `SubscribeToProcessRegistry` consumers typically collapse to a per-`ProcessKey` view; per-impl specs document the on-wire shape.
- `FetchJobs` with an empty `keys` slice: returns a channel that yields nothing. Idle wait until ctx cancels.
- A `JobStart` delivered twice: the second arrival is a no-op (gateway-generated `instanceID` is the idempotency key).
- A worker calls `ReenqueueSuspended` but the wait condition never gets satisfied (no `RespondToInputRequest` ever arrives for a `RequestInputTask`): the instance stays suspended forever. Per-process timeouts on `RequestInputTask` are the recommended guard.
