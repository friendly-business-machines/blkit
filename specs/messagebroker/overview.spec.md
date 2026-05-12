---
name: BrokerGateway Overview
description: The unified broker abstraction for blkit. The BrokerGateway interface presents both a producer-side API (Submit, DeliverMessage, Cancel, Terminate, Subscribe, ListAvailableProcesses) and a worker-side API (RegisterProcesses, HeartbeatRegistrations, ConsumeCommands, AckCommand, PublishEvent). Three implementations — Redis/Valkey, NATS, in-memory — share this overview. Implementations use broker-native primitives directly.
targets:
  - ../messagebroker/gateway.go
---

# BrokerGateway Overview

`messagebroker.BrokerGateway` is blkit's unified abstraction over the message broker / queue. It presents two role-specific groups of methods on a single interface:

- **Producer-side** — used by MCP servers, web servers, CLI tools, and admin UIs to submit new process runs, deliver messages to suspended instances, cancel/terminate from outside, and subscribe to events.
- **Worker-side** — used by `worker.Run` to register the worker's process capability set, refresh its TTL, consume routed commands, ack/nack them, and publish events as instances progress.

The same implementation satisfies both roles. Whether a given binary uses producer-side methods, worker-side methods, or both is determined by which entry-point functions it calls (`mcpserver.Run`, `worker.Run`, etc.) — not by which gateway type it constructs.

The gateway interacts **only with the broker**, not with the state store. Workers own state-store access exclusively. State queries (admin UIs, audit) go through the `StateStore` directly, separately from this interface (see [../data/state-store.spec.md](../data/state-store.spec.md)).

```
   Producers (MCP, web, CLI)               Workers
            │                                 │
            ▼                                 ▼
     ┌──────────────────────────┐
     │      BrokerGateway       │ ── (broker-native primitives:
     │ (producer + worker side) │     Redis Streams/Pub-Sub/KV;
     │                          │     NATS JetStream/Core/KV;
     │                          │     in-memory Go channels + maps)
     └──────────────────────────┘                        │
                                                         │ workers also use
                                                         ▼
                                                  ┌────────────┐
                                                  │ StateStore │
                                                  └────────────┘
```

`BrokerGateway` implementations use their broker's native primitives directly.

---

## Implementations

The `BrokerGateway` interface has three implementations, each in its own per-broker spec:

- **`RedisBrokerGateway`** — Redis or Valkey backend, see [redis.spec.md](redis.spec.md).
- **`NATSBrokerGateway`** — NATS + JetStream, see [nats.spec.md](nats.spec.md).
- **`InMemoryBrokerGateway`** — single-process, no external dependencies; suitable for tests and small deployments. See [in-memory.spec.md](in-memory.spec.md).

All three implement the same `BrokerGateway` interface defined below. Per-broker specs describe how the abstract operations map to broker primitives and the per-broker configuration shape.

---

## Interface

```go
package messagebroker

type BrokerGateway interface {
    // ===== Producer-side =====

    // Submit a new process run. The gateway generates the ProcessInstanceID
    // client-side and returns it immediately after the submit-command has
    // been published to the broker. The worker that consumes the command
    // creates the persisted state and runs the process.
    //
    // Synchronous errors:
    //   ErrUnknownProcess           — (Namespace, ProcessID, Version) not in the registry
    //   ErrUnknownStartID           — StartID does not match any StartEvent on the process
    //   DataContractValidationError — Input fails the StartEvent's InputContract
    //   broker-publish errors       — connection refused, auth failure, etc.
    //
    // Errors that surface only after the worker picks up the command
    // (e.g. process panic at evaluation start) are delivered as Event{Kind: Error}.
    Submit(ctx context.Context, req StartRequest) (string, error)

    // Resume a SuspendUntilMessage or PauseUntilMessage wait by publishing a
    // deliver-message command. The worker holding the suspended/paused
    // instance picks it up and resumes evaluation past the wait node.
    //
    // Synchronous errors: only broker-publish errors. Whether the instance
    // exists or is waiting on the given messageRef is not checked at the
    // gateway — those errors flow back as Event{Kind: Error}.
    DeliverMessage(ctx context.Context, instanceID, messageRef string, payload map[string]any) error

    // Clean abort. Publishes a cancel-command. Subject to the process's
    // AllowExternalCancel opt-in.
    Cancel(ctx context.Context, req InterruptRequest) error

    // Hard stop. Publishes a terminate-command. Subject to the process's
    // AllowExternalTerminate opt-in.
    Terminate(ctx context.Context, req InterruptRequest) error

    // Subscribe to events emitted as instances progress. Returns a channel
    // that closes when the context is cancelled. For an instance-scoped
    // filter, the channel additionally closes after the final EventResult
    // for that instance is delivered.
    Subscribe(ctx context.Context, filter EventFilter) (<-chan Event, error)

    // Convenience: subscribe to a single instance and block until terminal
    // status, returning the final EvaluationResult carried by EventResult.
    Wait(ctx context.Context, instanceID string) (*EvaluationResult, error)

    // Read the broker-held process registry. Returns one entry per
    // (Namespace, ProcessID, Version) the broker currently believes is
    // executable by some live worker. Stale entries (expired TTL) are not
    // returned.
    ListAvailableProcesses(ctx context.Context) ([]ProcessRegistration, error)

    // ===== Worker-side =====

    // Register the worker's capability set. Each call replaces this WorkerID's
    // previous registration set. Idempotent. The TTL is set internally by the
    // implementation (typically tied to the heartbeat interval × 3).
    RegisterProcesses(ctx context.Context, workerID string, regs []ProcessRegistration) error

    // Refresh the TTL on this WorkerID's registrations. Called periodically
    // by the worker's heartbeat goroutine.
    HeartbeatRegistrations(ctx context.Context, workerID string) error

    // Explicitly remove this WorkerID's registrations. Called on graceful
    // shutdown so the broker stops advertising processes a worker can no
    // longer service.
    UnregisterProcesses(ctx context.Context, workerID string) error

    // Consume commands published for processes in `keys`. Returns a channel
    // that emits Commands the worker should dispatch to executors. Closes
    // when ctx is cancelled.
    //
    // Each Command carries an opaque ack token; the worker calls AckCommand
    // / NackCommand on the gateway after handling.
    ConsumeCommands(ctx context.Context, keys []ProcessKey) (<-chan Command, error)

    // Acknowledge successful handling of a Command. After this the broker
    // releases the in-flight slot.
    AckCommand(ctx context.Context, cmd Command) error

    // Negative-acknowledge a Command (handler failed; broker should
    // redeliver per its retry policy).
    NackCommand(ctx context.Context, cmd Command, err error) error

    // Publish an Event for any subscribers. Workers call this for status
    // changes, message-requests, node-completion, errors, and final results.
    PublishEvent(ctx context.Context, evt Event) error
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

    // Optional client-side correlation key recorded in every emitted Event
    // for this instance. Use to tie the instance back to a request-id /
    // user-id / etc. on the client side.
    CorrelationKey *string
}

type InterruptRequest struct {
    Namespace string  // (Namespace, ProcessID, Version) of the target instance —
    ProcessID string  // needed so the gateway can check the process's
    Version   string  // AllowExternalCancel / AllowExternalTerminate flag in-process

    InstanceID string
    Reason     *string
}

type EventFilter struct {
    InstanceID *string     // restrict to a single instance; nil = all instances visible to this gateway
    Kinds      []EventKind // empty = all kinds
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
```

### Command types (worker-side)

`Command` is a tagged union of broker-published commands the worker dispatches to executors.

```go
type Command struct {
    Kind  CommandKind
    Key   ProcessKey // routing key
    Token any        // opaque ack token; populated by the gateway impl

    // Kind-specific payload. Exactly one is set per Command.
    Submit         *SubmitCommand
    DeliverMessage *DeliverMessageCommand
    Cancel         *CancelCommand
    Terminate      *TerminateCommand
    Continuation   *ContinuationCommand // re-evaluation after a Suspend* event
}

type CommandKind int

const (
    CommandSubmit CommandKind = iota
    CommandDeliverMessage
    CommandCancel
    CommandTerminate
    CommandContinuation
)

type SubmitCommand struct {
    InstanceID     string  // gateway-generated UUID
    StartID        string
    Input          map[string]any
    CorrelationKey *string
}

type DeliverMessageCommand struct {
    InstanceID string
    MessageRef string
    Payload    map[string]any
}

type CancelCommand struct {
    InstanceID string
    Reason     *string
}

type TerminateCommand struct {
    InstanceID string
    Reason     *string
}

type ContinuationCommand struct {
    InstanceID string
}
```

### Event types

```go
type Event struct {
    InstanceID     string
    CorrelationKey *string    // mirrored from StartRequest if set
    Kind           EventKind
    OccurredAt     time.Time

    // Kind-specific payloads; only one is set per Event.
    StatusChange    *StatusChange     // status transition events
    MessageRequest  *MessageRequest   // process suspended on SuspendUntilMessage; surfaces the messageRef
    NodeCompleted   *NodeCompleted    // task / decision completion (for progress UIs)
    Error           *EventError       // command rejected by the worker, or PROCESS_FAILED
    Result          *EvaluationResult // emitted on terminal status
}

type EventKind int

const (
    EventStatusChange EventKind = iota
    EventMessageRequest
    EventNodeCompleted
    EventError
    EventResult
)

type StatusChange struct {
    From, To ProcessStatus // see ../processes/process.spec.md
}

type MessageRequest struct {
    NodeID     string
    MessageRef string

    // Optional request body. Set by RequestInputTask to describe what is being
    // asked of the responder (form schema, prompt text, current case state).
    // Nil when emitted by a bare SuspendUntilMessage / PauseUntilMessage event,
    // which carry no outbound payload.
    Payload map[string]any
}

type NodeCompleted struct {
    NodeID   string
    NodeKind string // "Task", "DecisionTask", "EndEvent", etc.
    Outputs  map[string]any
}

type EventError struct {
    Code    string // see "EventError codes" below
    Message string
}
```

---

## Producer-side operation flows

### `Submit`

1. Look up the process via the in-memory registry: `blkit.LookupProcess(Namespace, ProcessID, Version)`. Missing → `ErrUnknownProcess`.
2. Find the `StartEvent` matching `StartID`. Missing → `ErrUnknownStartID`.
3. Validate `Input` against the StartEvent's `InputContract` (see [../data/data-contract.spec.md](../data/data-contract.spec.md)). Failure → `DataContractValidationError`.
4. Generate a `ProcessInstanceID` (UUIDv7 or similar).
5. Publish a submit-command on the broker carrying `(ProcessInstanceID, StartRequest)`. Broker error → returned synchronously.
6. Return `ProcessInstanceID`.

The worker that consumes the submit-command creates persisted state via `StateStore.NewExecutionState(...)` using the gateway-supplied `ProcessInstanceID`, begins evaluation, and publishes events.

If the broker delivers the same submit-command twice (at-least-once semantics), the worker treats the second arrival as a no-op — `ProcessInstanceID` already has persisted state.

### `DeliverMessage`

1. Publish a deliver-message command on the broker carrying `(instanceID, messageRef, payload)`. Broker error → returned synchronously.
2. Return.

A worker handling the instance loads state, confirms it is suspended on the given `messageRef`, and resumes evaluation past the wait node. If state is missing or the messageRef doesn't match, the worker publishes `Event{Kind: Error}` with `Code: "INSTANCE_NOT_FOUND"` or `"NOT_WAITING"`.

For `PauseUntilMessage` (in-memory wait): only the specific worker holding the in-flight goroutine can deliver. The deliver-message command is broadcast to all workers; the one holding the wait responds.

### `Cancel` and `Terminate`

1. Look up the process via `blkit.LookupProcess(...)`. Missing → `ErrUnknownProcess`.
2. Check `AllowExternalCancel` / `AllowExternalTerminate` on the resolved process. Not opted in → `ErrCancelNotAllowed` / `ErrTerminateNotAllowed`.
3. Publish a cancel/terminate-command. Broker error → returned synchronously.
4. Return.

A worker handling the instance appends a synthetic `CancelEvent` / `TerminateEvent` to history, drives the instance to terminal status, and publishes the resulting `StatusChange` / `Result` events.

### `ListAvailableProcesses`

Returns the snapshot of the broker-held registry. Each entry corresponds to a `(Namespace, ProcessID, Version)` that some live worker has registered and not yet let expire. Used by the MCP server's startup tool-discovery and by web/admin UIs.

The implementation queries the broker's KV / metadata store and filters out entries whose `LastHeartbeat + TTL` is in the past.

---

## Worker-side operation flows

### `RegisterProcesses` / `HeartbeatRegistrations` / `UnregisterProcesses`

The worker calls `RegisterProcesses` once on startup with one `ProcessRegistration` per process in `blkit.AllProcesses()`. The gateway stores them in the broker's registry KV with a TTL.

`HeartbeatRegistrations` is called by a worker-owned heartbeat goroutine on a configurable interval (default 30s). It refreshes the TTL on every registration this `WorkerID` published. If the worker stops heartbeating (crash, network partition), entries expire and `ListAvailableProcesses` stops returning them.

`UnregisterProcesses` is called on graceful shutdown to remove the entries immediately rather than waiting for TTL expiry.

The worker also calls `RegisterProcesses` again after every successful change to its capability set (rare in practice — it would require dynamic process loading, which blkit does not currently support).

### `ConsumeCommands` / `AckCommand` / `NackCommand`

`ConsumeCommands(ctx, keys)` returns a channel of `Command`s targeted at processes in `keys`. The implementation handles the broker-specific selective-consumption mechanism (Redis: stream-with-consumer-group filtering; NATS: JetStream subject filter; in-memory: filtered channel).

For each `Command` received, the worker dispatches it to an executor goroutine, then calls `AckCommand` on success or `NackCommand` on failure. The ack/nack tells the broker whether to redeliver.

### `PublishEvent`

Workers call `PublishEvent` after each meaningful state transition: `StatusChange` on every status update, `NodeCompleted` after each task / decision finishes, `MessageRequest` when reaching a `SuspendUntilMessage`, `Error` on task or process failure, `Result` on terminal status.

The gateway routes the event to all subscribers whose `EventFilter` matches.

---

## `Subscribe` and event delivery

Push-only. Implementations open a long-lived subscription on the broker and forward events to the returned channel.

```go
ch, err := gw.Subscribe(ctx, EventFilter{InstanceID: ptr(instanceID)})
for evt := range ch {
    switch evt.Kind {
    case EventMessageRequest:
        // surface to UI; eventually call gw.DeliverMessage(...)
    case EventResult:
        // terminal — channel closes after this
    }
}
```

The channel closes when:

- The context is cancelled, **or**
- The filter is instance-scoped (`InstanceID != nil`) and the instance has reached a terminal status with the final `EventResult` delivered.

For broadcast subscriptions (`InstanceID == nil`), the channel only closes on context cancellation.

### Backpressure

If a subscriber's channel buffer fills, events are **dropped** rather than blocking the broker reader. When the buffer recovers, a synthetic `Event{Kind: Error, Code: "BACKPRESSURE_DROP"}` is delivered. Per-implementation specs may override this default.

### Fan-out

Multiple subscribers to the same instance: each gets the full event stream by default (broadcast). Per-implementation specs may add a queue-group mode where events are load-balanced across subscribers.

---

## `ProcessOpts` opt-ins

Both `Cancel` and `Terminate` require the process author to opt in. See [../processes/process.spec.md](../processes/process.spec.md):

```go
type ProcessOpts struct {
    // ... existing fields ...

    AllowExternalCancel    bool // default false; required for BrokerGateway.Cancel to succeed
    AllowExternalTerminate bool // default false; required for BrokerGateway.Terminate to succeed
}
```

---

## Error model

### Synchronous errors

| Error | Returned from | When |
|---|---|---|
| `ErrUnknownProcess` | `Submit`, `Cancel`, `Terminate` | `(Namespace, ProcessID, Version)` not in the in-process registry |
| `ErrUnknownStartID` | `Submit` | `StartID` does not match any `StartEvent` on the resolved process |
| `DataContractValidationError` | `Submit` | `Input` fails the StartEvent's `InputContract` |
| `ErrCancelNotAllowed` | `Cancel` | Process has `AllowExternalCancel: false` |
| `ErrTerminateNotAllowed` | `Terminate` | Process has `AllowExternalTerminate: false` |
| `ErrUnknownWorker` | `HeartbeatRegistrations`, `UnregisterProcesses` | `WorkerID` not currently registered |
| broker-publish errors | All publish ops | Connection refused, auth failure, etc. |

### Asynchronous errors (via `Event{Kind: Error}`)

| `EventError.Code` | Source |
|---|---|
| `INSTANCE_NOT_FOUND` | Worker handling a command for an unknown `instanceID` |
| `NOT_WAITING` | `DeliverMessage` for an instance not suspended on the given `messageRef` |
| `ALREADY_TERMINAL` | `Cancel` / `Terminate` for an instance that's already finished |
| `ALREADY_INTERRUPTING` | `Cancel` / `Terminate` issued while a prior interrupt is still being processed |
| `TASK_FAILED` | An in-process task error during evaluation |
| `PROCESS_FAILED` | The instance reached `ProcessStatusFailed` |
| `BACKPRESSURE_DROP` | Subscriber's buffer overflowed; events were dropped (synthesized by the gateway) |

---

## End-to-end example

A web server submits a loan-application process, surfaces a human-approval request to the user, and delivers the response back. The worker pool runs separately (in a different binary, or in-process via `mcpserver.EmbeddedWorker`).

```go
package main

import (
    "context"
    "log"

    "blkit"
    "blkit/messagebroker"

    _ "example.com/processes/lending" // blank import registers the process via NewProcess()
)

func main() {
    gw, err := messagebroker.NewRedisBrokerGateway(messagebroker.RedisOpts{
        Addr: "localhost:6379",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer gw.Close()

    ctx := context.Background()

    instanceID, err := gw.Submit(ctx, messagebroker.StartRequest{
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

    events, err := gw.Subscribe(ctx, messagebroker.EventFilter{
        InstanceID: &instanceID,
    })
    if err != nil {
        log.Fatalf("subscribe: %v", err)
    }

    for evt := range events {
        switch evt.Kind {
        case messagebroker.EventMessageRequest:
            response := promptUserForApproval(ctx, evt.MessageRequest)
            if err := gw.DeliverMessage(ctx, instanceID, evt.MessageRequest.MessageRef, response); err != nil {
                log.Printf("deliver: %v", err)
            }

        case messagebroker.EventResult:
            log.Printf("done: status=%v outputs=%v", evt.Result.Status, evt.Result.Context)

        case messagebroker.EventError:
            log.Printf("error: code=%s message=%s", evt.Error.Code, evt.Error.Message)
        }
    }
}
```

---

## Edge cases

- The gateway and the worker pool may be different binaries, but both must import the package that defines the process. The gateway uses the in-process registry for synchronous validation on `Submit`. The worker uses it for capability-set filtering on `ConsumeCommands`.
- A `Cancel` racing with a graph-driven terminal event: whichever the worker observes first wins. If the cancel arrives after, the worker publishes `Event{Kind: Error, Code: "ALREADY_TERMINAL"}`.
- `Terminate` after `Cancel` already requested but not yet processed: implementations may accept (Terminate overrides; "harder" wins) or reject with `ALREADY_INTERRUPTING`. The overview spec recommends accepting; per-impl specs document their choice.
- `Subscribe` on an instance that has already reached terminal status: whether the gateway can replay the cached final `EventResult` depends on the broker's retention policy. Per-impl specs document the behavior.
- `ListAvailableProcesses` returns entries whose TTL hasn't expired. A worker that crashes silently leaves stale entries until TTL expiry — typically `heartbeat_interval × 3`.
- Multiple workers register the same `(Namespace, ProcessID, Version)`: the registry holds one `ProcessRegistration` per worker. `ListAvailableProcesses` may collapse duplicates by `(Namespace, ProcessID, Version)` for the producer view; per-impl specs document the collapsing.
- `ConsumeCommands` with an empty `keys` slice: returns a channel that yields nothing. Idle wait until ctx cancels.
- A submit-command delivered twice: the second arrival is a no-op (gateway-generated `ProcessInstanceID` is the idempotency key).
- `Wait` for an instance that the worker pool cannot reach (broker partitioned, no live worker for the key): blocks until ctx times out.
