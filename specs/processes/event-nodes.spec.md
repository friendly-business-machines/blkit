---
name: Event Nodes
description: Lifecycle and event node types — StartEvent, EndEvent, CancelEvent, ErrorEvent, TerminateEvent, SuspendForDuration, SuspendUntilDatetime, SuspendUntilMessage, PauseForDuration, PauseUntilMessage; and the runtime Message Delivery API for resuming Suspend*/Pause*UntilMessage waits
targets:
  - ../processes/event_nodes.go
---

# Event Nodes

Event nodes mark process boundaries (entrypoints and terminations) and durable or ephemeral waits within a flow. All event nodes implement `ProcessNode` and can appear in a process graph via `.To()`.

## Class Hierarchy

```
ProcessNode (interface)
├── StartEvent
├── EndEvent              — normal completion
├── CancelEvent           — clean abort, status = Cancelled
├── ErrorEvent            — failure with an error code, status = Failed
├── TerminateEvent        — immediate hard stop of all branches, status = Completed
├── SuspendForDurationEvent
├── SuspendUntilDatetimeEvent
├── SuspendUntilMessageEvent
├── PauseForDurationEvent
└── PauseUntilMessageEvent
```

`StartEvent` is the only entrypoint type. The five terminating events (`EndEvent`, `CancelEvent`, `ErrorEvent`, `TerminateEvent`) all consume their token; `Process.create()` discovers them by type during graph walk and validates that at least one is reachable from each `StartEvent`.

The five Suspend/Pause events are non-terminating — execution resumes when their wait condition is satisfied.

---

## Lifecycle Events

### `StartEvent`

Marks a process entrypoint. A `StartEvent` carries a mandatory `*InputContract` that validates the variables supplied at submission. See [data-contract.spec.md](../data/data-contract.spec.md).

```go
type StartEvent struct {
    ProcessNode
    Id       string
    Name     string         // mandatory
    Contract *InputContract // mandatory; never nil for a constructed StartEvent
}

type StartOpts struct {
    // Reserved for future per-StartEvent options; currently empty.
}

func Start(id string, name string, contract *InputContract, opts ...StartOpts) *StartEvent
```

```go
start := Start("start", "New Application", NewInputContract(
    RequiredField("applicant", applicantContract),
    RequiredField("loan_amount", BlNumber),
))
```

A process can have multiple start nodes, each with a unique `id`. The `id` is used by `store.NewExecutionState(NewExecutionStateOpts{StartId: ...})` to select the entrypoint. Duplicate start node ids on the same process produce a `ProcessDefinitionError`.

---

### `EndEvent`

Marks the normal completion of a token. When all tokens have been consumed and at least one was consumed by an `EndEvent`, the process completes with `ProcessStatusCompleted`. Optionally carries an `*OutputContract` that validates the `ExecutionContext` when the token arrives, before completion is recorded.

```go
type EndEvent struct {
    ProcessNode
    Id       string
    Name     string          // mandatory
    Contract *OutputContract // optional; nil means no output validation
}

type EndOpts struct {
    Contract *OutputContract
}

func End(id string, name string, opts ...EndOpts) *EndEvent
```

```go
done := End("done", "Done")

approved := End("approved", "Loan Approved", EndOpts{
    Contract: NewOutputContract(
        RequiredField("offer_id", BlString),
        RequiredField("approved_amount", BlNumber),
    ),
})
```

Duplicate end node ids on the same process produce a `ProcessDefinitionError`.

---

### `CancelEvent`

Cleanly aborts the process as a non-failure outcome. When a token reaches a `CancelEvent`, all in-flight tasks on parallel branches are cancelled, the process status is set to `ProcessStatusCancelled`, and `Evaluate()` returns. A cancelled process is **not** retried by the runtime.

`CancelEvent` is distinct from `EndEvent` (normal completion) and `ErrorEvent` (failure). Use it for outcomes like "user withdrew the application" or "deadline missed" where the process ended deliberately without succeeding or failing.

```go
type CancelEvent struct {
    ProcessNode
    Id       string
    Name     string
    Reason   *string         // optional human-readable reason recorded in history
    Contract *OutputContract // optional; validates ExecutionContext at cancellation
}

type CancelOpts struct {
    Reason   *string
    Contract *OutputContract
}

func Cancel(id string, name string, opts ...CancelOpts) *CancelEvent
```

```go
withdrawn := Cancel("withdrawn", "Application Withdrawn")

expired := Cancel("expired", "Deadline Expired", CancelOpts{
    Reason: ptr("SLA window closed before applicant returned documents"),
})
```

When reached, the runtime records `PROCESS_CANCELLED` in the `ExecutionHistory` with the reason (if set) and any `Outputs` that pass the optional contract.

A synthetic `CancelEvent` step can also be injected by the runtime in response to an external `BrokerGateway.Cancel(...)` request — see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md). The semantics and history record are identical to a graph-driven `CancelEvent`. External Cancel requires the process to opt in via `ProcessOpts.AllowExternalCancel: true`.

---

### `ErrorEvent`

Fails the process with an error code. When a token reaches an `ErrorEvent`, all in-flight tasks on parallel branches are cancelled, the process status is set to `ProcessStatusFailed`, and `Evaluate()` returns. Subject to the process's `Retry` configuration, the runtime may retry the process from a fresh start.

`ErrorEvent` is for declared failure outcomes within the graph — e.g. "credit-score below cutoff, fail the application." For task-level errors (a `Fn` returned an error), use task `ErrorExitPort` instead, which lets the graph route around the failure rather than terminating.

```go
type ErrorEvent struct {
    ProcessNode
    Id       string
    Name     string
    ErrorRef string  // mandatory: the error code recorded in history and visible to retry policy
    Message  *string // optional: human-readable detail
}

type ErrorOpts struct {
    Message *string
}

func Error(id string, name string, errorRef string, opts ...ErrorOpts) *ErrorEvent
```

```go
declined := Error("declined", "Application Declined", "CREDIT_BELOW_CUTOFF")

invalidInput := Error("invalid", "Invalid Input", "VALIDATION_FAILED", ErrorOpts{
    Message: ptr("loan_amount exceeds policy maximum"),
})
```

When reached, the runtime records `PROCESS_FAILED` with the `ErrorRef` and `Message` attached. The behaviour is identical to a task that throws an uncaught error.

---

### `TerminateEvent`

Immediately stops every active branch in the process and marks the process complete (status = `ProcessStatusCompleted`). Unlike `EndEvent`, which only consumes its own token (the process completes when **all** tokens are consumed), `TerminateEvent` short-circuits parallel branches — pending tasks on other branches are cancelled and their tokens discarded.

Use it when one branch determines the outcome and other branches' work no longer matters (e.g. fraud-check returns "high risk" and you want to abandon the credit-pull and income-check that are still running).

```go
type TerminateEvent struct {
    ProcessNode
    Id   string
    Name string
}

func Terminate(id string, name string) *TerminateEvent
```

```go
fraudHalt := Terminate("fraud-halt", "Fraud Detected")
```

When reached, the runtime records `PROCESS_TERMINATED` in the history with the terminating node's id, then cancels every remaining in-flight task before returning.

A synthetic `TerminateEvent` step can also be injected by the runtime in response to an external `BrokerGateway.Terminate(...)` request — see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md). The semantics and history record are identical to a graph-driven `TerminateEvent`. External Terminate requires the process to opt in via `ProcessOpts.AllowExternalTerminate: true`.

---

## Suspend Events (Durable Wait)

Suspend events durably halt the process. When reached, `Evaluate()` records the suspension in `ExecutionHistory`, returns with status `ProcessStatusSuspended`, and the runtime persists state to the configured `StateStore`. The FAAS handler unloads; the goroutine exits. A separate event (timer firing, message delivery) later triggers a fresh `Evaluate()` call that resumes from the suspension point.

Use suspend events for waits that can exceed an invocation's lifetime — minutes to days. For sub-second to a few-minutes waits where keeping the runtime alive is acceptable, use Pause events instead.

### `SuspendForDuration`

Suspends the process for a duration relative to when the suspension is recorded.

```go
type SuspendForDurationEvent struct {
    ProcessNode
    Id       string
    Name     string
    Duration BlExpr // resolves to BlDaysTimeDuration
}

func SuspendForDuration(id string, name string, duration BlExpr) *SuspendForDurationEvent
```

```go
backoff := SuspendForDuration("backoff", "Wait Before Retry", Bl.DaysTimeDuration("PT15M"))
```

The runtime computes the wake time as `now + duration` and persists it. A timer subsystem (the long-running worker's scheduler, or an external scheduler invoking the FAAS handler at the appropriate time) re-enqueues the process when the wake time is reached.

### `SuspendUntilDatetime`

Suspends the process until an absolute datetime, evaluated against the `ExecutionContext` at the moment of suspension.

```go
type SuspendUntilDatetimeEvent struct {
    ProcessNode
    Id       string
    Name     string
    Deadline BlExpr // resolves to BlDateTime
}

func SuspendUntilDatetime(id string, name string, deadline BlExpr) *SuspendUntilDatetimeEvent
```

```go
auctionStart := SuspendUntilDatetime("await-auction", "Wait for Auction Start",
    Bl.DateTimeVar("start.auction_start_time"))
```

If the resolved deadline is in the past, the suspension is recorded but the runtime wakes immediately on the next scheduler tick.

### `SuspendUntilMessage`

Suspends the process until a message with the given `MessageRef` is delivered to the instance. See [Message Delivery](#message-delivery) below for the runtime API.

```go
type SuspendUntilMessageEvent struct {
    ProcessNode
    Id            string
    Name          string
    MessageRef    string  // matches the messageRef supplied to DeliverMessage
    PayloadVariable *string // optional: variable name to write the payload into the ExecutionContext
}

type SuspendUntilMessageOpts struct {
    PayloadVariable *string
}

func SuspendUntilMessage(id string, name string, messageRef string, opts ...SuspendUntilMessageOpts) *SuspendUntilMessageEvent
```

```go
awaitApproval := SuspendUntilMessage("await-approval", "Wait for Approval", "approval-response",
    SuspendUntilMessageOpts{PayloadVariable: ptr("approval")})
```

When the matching message is delivered, the payload is written to the `ExecutionContext` under `PayloadVariable` (when set) and the process resumes from the node's outgoing edge.

---

## Pause Events (Ephemeral Wait)

Pause events wait without persisting state. `Evaluate()` blocks the goroutine — it does **not** return — until the wait condition is satisfied. The process status remains `ProcessStatusRunning` throughout. Other parallel branches (if any) continue advancing concurrently; only the token at the pause node is held.

Use pause events when:
- The wait is short (seconds to a few minutes).
- The runtime is a long-running worker, not a FAAS invocation. Pausing inside a FAAS handler holds the invocation open and accrues cost; suspend instead.
- You don't need to free goroutine resources during the wait.

### `PauseForDuration`

```go
type PauseForDurationEvent struct {
    ProcessNode
    Id       string
    Name     string
    Duration BlExpr // resolves to BlDaysTimeDuration
}

func PauseForDuration(id string, name string, duration BlExpr) *PauseForDurationEvent
```

```go
rateLimit := PauseForDuration("rate-limit", "Rate Limit", Bl.DaysTimeDuration("PT2S"))
```

### `PauseUntilMessage`

Waits in-process for a message delivered via the runtime's [Message Delivery](#message-delivery) API. The wait is held on a per-instance channel; if the runtime restarts before delivery, the wait is lost (unlike `SuspendUntilMessage`, which persists).

```go
type PauseUntilMessageEvent struct {
    ProcessNode
    Id              string
    Name            string
    MessageRef      string
    PayloadVariable *string
}

type PauseUntilMessageOpts struct {
    PayloadVariable *string
}

func PauseUntilMessage(id string, name string, messageRef string, opts ...PauseUntilMessageOpts) *PauseUntilMessageEvent
```

---

## Message Delivery

Both `SuspendUntilMessage` and `PauseUntilMessage` require an out-of-band channel for external systems to deliver messages back to a waiting process instance. blkit exposes this via runtime APIs — the runtime layer (long-running worker or FAAS message-delivery handler), not `Process.Evaluate()` itself, owns delivery.

### Long-running worker

```go
// Deliver a message to a single suspended/paused process instance.
// Returns a not-found error if no instance is waiting on this (instanceID, messageRef).
func (w *Worker) DeliverMessage(processInstanceID string, messageRef string, payload map[string]any) error
```

Behavior:

1. Look up the persisted state for `processInstanceID` in the `StateStore`.
2. Confirm the instance is waiting on the given `messageRef` (via the most recent `SUSPENSION_RECORDED` step in history, or via an in-memory wait channel for `Pause*` cases).
3. For `SuspendUntilMessage`: enqueue a continuation `ProcessTask` with the payload attached. A worker (this one or another) picks it up, replays history, and resumes evaluation past the suspension.
4. For `PauseUntilMessage`: deliver the payload directly on the in-memory wait channel; the goroutine resumes inline.
5. The first matching wait wins. If multiple suspended branches in the same instance are waiting on the same `messageRef`, only one is resumed; the others continue waiting. This matches `select`-on-multiple-receivers semantics.

### FAAS

```go
// faas.MessageDeliveryHandler — adapts (instanceID, messageRef, payload) inputs from
// an inbound vendor event into a DeliverMessage call. Same shape as the FAAS handler
// factory pattern — see ../faas/overview.spec.md.
func MessageDeliveryHandler(opts MessageDeliveryOpts) func(...)

type MessageDeliveryOpts struct {
    Route      func(ctx context.Context, event json.RawMessage) (instanceID, messageRef string, payload map[string]any, err error)
    StateStore StateStore                  // required: load the persisted instance for routing context
    Gateway    messagebroker.BrokerGateway // required: deliver the message to the suspended instance
}
```

The handler routes the inbound event into `(instanceID, messageRef, payload)` via `Route`, then calls `gw.DeliverMessage(ctx, instanceID, messageRef, payload)` (see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md#delivermessage)). A worker subscribed to the broker picks up the resulting deliver-message command, loads state from the configured `StateStore`, and resumes evaluation past the wait node.

The FAAS variant only delivers to **suspended** instances (those with persisted state). Pause-style waits are not addressable from FAAS because no goroutine is alive to receive on the in-memory channel.

### Correlation

The `(processInstanceID, messageRef)` pair is the correlation key. blkit does not provide message-content-based correlation (e.g. routing a message by `customer_id` to whichever instance is awaiting that customer). If you need that, the application layer maintains the mapping (e.g. by including `processInstanceID` in the request when you emit the outbound message — typically via a `NativeFunctionTask` immediately before the `SuspendUntilMessage` node).

### Pattern: request input from a human or another system

The high-level path is [`RequestInputTask`](task-nodes.spec.md#requestinputtask), which bundles "emit a typed input request → wait for a correlated response → write the response back into the context" into a single task. It supports three wait modes (durable suspend, in-memory pause, hybrid pause-then-suspend), an optional `ResponseContract` for payload validation, and the standard `TimerExitPort` / `ErrorExitPort` machinery for SLA enforcement and error recovery.

Use the raw `NativeFunctionTask + SuspendUntilMessage` pairing below only when you need to side-effect on an external system that is **not** addressable through the `BrokerGateway` (e.g. posting to a third-party inbox that does not subscribe to `EventMessageRequest`). The runtime publishes `EventMessageRequest` automatically for `SuspendUntilMessage` (with `Payload == nil`) and for `RequestInputTask` (with `Payload` set to the resolved request payload).

```go
// Lower-level primitive — emit the outbound request via a NativeFunctionTask
// when the responder cannot be reached via BrokerGateway subscriptions.
emitRequest := NewNativeFunctionTask("emit-request", "Request Approval", approvals.PostToInbox)

awaitApproval := SuspendUntilMessage("await-approval", "Wait for Approval", "approval-response",
    SuspendUntilMessageOpts{PayloadVariable: ptr("approval")})

decide := NewNativeFunctionTask("decide", "Apply Decision", approvals.Apply)

// emitRequest reads the persisted ExecutionContext for what to ask, posts it
// to the human task inbox along with processInstanceID. When the human responds,
// the inbox calls BrokerGateway.DeliverMessage(instanceID, "approval-response", payload).
//
// Graph wiring: ... .To(emitRequest).To(awaitApproval).To(decide)...
```

The `processInstanceID` is available to `emitRequest` via `ctx.ProcessInstanceID()` so it can be embedded in the outbound payload as the correlation key.

---

## Validation Timing Reference

| Event node | When validated/run | Effect |
|---|---|---|
| `StartEvent` | At submission, before any token is placed | `Inputs` contract validated; failure → `DataContractValidationError`, submission rejected synchronously |
| `EndEvent` | When token reaches it, before `PROCESS_COMPLETED` | `Outputs` contract (if set) validated; failure → `PROCESS_FAILED` |
| `CancelEvent` | When token reaches it | `Outputs` contract (if set) validated; success → `PROCESS_CANCELLED`; validation failure → `PROCESS_FAILED` |
| `ErrorEvent` | When token reaches it | Always → `PROCESS_FAILED` with `ErrorRef`. No contract |
| `TerminateEvent` | When token reaches it | All other branches cancelled, then `PROCESS_COMPLETED` |
| `SuspendForDuration` / `SuspendUntilDatetime` | When token reaches it | Wake time computed and persisted; status → `SUSPENDED`; `Evaluate()` returns |
| `SuspendUntilMessage` | When token reaches it | `(instanceID, messageRef)` registered in persisted state; status → `SUSPENDED`; `Evaluate()` returns |
| `PauseForDuration` / `PauseUntilMessage` | When token reaches it | Goroutine blocks on timer/channel; status remains `RUNNING`; other branches advance |

---

## Edge Cases

- A process with no terminating event reachable from a `StartEvent` produces a `ProcessDefinitionError` at construction time. "Terminating event" means any of `EndEvent`, `CancelEvent`, `ErrorEvent`, `TerminateEvent`.
- Duplicate ids across **all** event-node types within a single process produce a `ProcessDefinitionError`. The id namespace is shared across event subtypes.
- `Cancel`, `Error`, `Terminate`, and additional `End` nodes all consume their token. A `TerminateEvent` reached on one branch cancels other branches' in-flight tasks before they can reach their own terminating events.
- An `ErrorEvent` whose `ErrorRef` is empty produces a `ProcessDefinitionError`. The error code is part of the process's observable contract — silent failures are not allowed via this node.
- A `SuspendUntilDatetime` whose evaluated `Deadline` is in the past wakes immediately on the next scheduler tick — this is not an error.
- A `PauseForDuration` inside a FAAS-deployed process is permitted but discouraged: it holds the invocation open. Use `SuspendForDuration` instead. The spec does not mechanically prevent it.
- A `SuspendUntilMessage` for a `messageRef` that no external system ever delivers leaves the process suspended indefinitely. Pair with `MaxCompletionTime` on the process if a deadline is required.
- A `PauseUntilMessage` is lost across worker restarts — there's no persisted record. If the worker crashes mid-pause, the in-memory channel is gone and the wait can never resolve. Use `SuspendUntilMessage` if durability matters.
- `DeliverMessage` for an `instanceID` that is not waiting on the given `messageRef` returns a not-found error; the runtime does not buffer messages for future delivery.
- A `TerminateEvent` reached while another branch is mid-task: the in-flight task's goroutine is cancelled (per `Evaluate()`'s existing branch-cancellation behaviour). Side effects already committed by the cancelled task are not rolled back — blkit has no compensation.
- A `CancelEvent` and `EndEvent` reached on different parallel branches of the same instance: whichever token arrives first wins. The other branches are cancelled. The status reflects the winning node's type (`Cancelled` for `Cancel`, `Completed` for `End`).
- An `ErrorEvent` whose token races with an `EndEvent` on a parallel branch: the `ErrorEvent` wins by precedence — failure is sticky. Status is `Failed`.
