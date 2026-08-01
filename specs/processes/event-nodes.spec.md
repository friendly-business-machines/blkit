---
name: Event Nodes
description: Lifecycle and event node types — StartEvent, EndEvent, CancelEvent, ErrorEvent, TerminateEvent, SuspendForDuration, SuspendUntilDatetime, PauseForDuration. Timer-based only; external input is handled by the RequestInputTask task node.
status: agreed
code:
  - core/event_nodes.go
---

# Event Nodes

Event nodes mark process boundaries (entrypoints and terminations) and timer-based waits within a flow. All event nodes implement `ProcessNode` and can appear in a process graph via `.To()`.

## Class Hierarchy

```
ProcessNode (interface)
├── StartEvent
├── EndEvent              — normal completion
├── CancelEvent           — clean abort, status = Cancelled
├── ErrorEvent            — failure with an error code, status = Failed
├── TerminateEvent        — immediate hard stop of all branches, status = Cancelled
├── SuspendForDurationEvent
├── SuspendUntilDatetimeEvent
└── PauseForDurationEvent
```

`StartEvent` is the only entrypoint type. The four terminating events (`EndEvent`, `CancelEvent`, `ErrorEvent`, `TerminateEvent`) all consume their token; `Process.create()` discovers them by type during graph walk and validates that at least one is reachable from each `StartEvent`.

The three Suspend/Pause events are non-terminating — execution resumes when their wait condition (a timer or datetime) is satisfied. There is no event-node mechanism for waiting on external input; for that, use the [`RequestInputTask`](task-nodes.spec.md#requestinputtask) task node, which pairs with the gateway's `RespondToInputRequest` verb.

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
start := bl.Start("start", "New Application", bl.NewInputContract(
    bl.RequiredField("applicant", applicantContract),
    bl.RequiredField("loan_amount", bl.BlNumber),
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
done := bl.End("done", "Done")

approved := bl.End("approved", "Loan Approved", EndOpts{
    Contract: bl.NewOutputContract(
        bl.RequiredField("offer_id", bl.BlString),
        bl.RequiredField("approved_amount", bl.BlNumber),
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

A synthetic `CancelEvent` step can also be injected by the runtime in response to an external `MessageBroker.Cancel(...)` request — see [../message-brokers/overview.spec.md](../message-brokers/overview.spec.md). The semantics and history record are identical to a graph-driven `CancelEvent`. External Cancel requires the process to opt in via `ProcessOpts.AllowExternalCancel: true`.

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

A synthetic `TerminateEvent` step can also be injected by the runtime in response to an external `MessageBroker.Terminate(...)` request — see [../message-brokers/overview.spec.md](../message-brokers/overview.spec.md). The semantics and history record are identical to a graph-driven `TerminateEvent`. External Terminate requires the process to opt in via `ProcessOpts.AllowExternalTerminate: true`.

---

## Suspend Events (Durable Wait)

Suspend events durably halt the process. When reached, `Evaluate()` records the suspension in `ExecutionHistory`, returns with status `ProcessStatusSuspended`, and the runtime persists state to the configured `StateStore`. The goroutine exits. A separate event (timer firing, message delivery) later triggers a fresh `Evaluate()` call that resumes from the suspension point.

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
backoff := SuspendForDuration("backoff", "Wait Before Retry", bl.DaysTimeDuration("PT15M"))
```

The runtime computes the wake time as `now + duration` and persists it. A timer subsystem (the long-running worker's scheduler) re-enqueues the process when the wake time is reached.

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
    bl.DateTimeVar("start.auction_start_time"))
```

If the resolved deadline is in the past, the suspension is recorded but the runtime wakes immediately on the next scheduler tick.

---

## Pause Events (Ephemeral Wait)

Pause events wait without persisting state. The pause node is dispatched as its own goroutine that sleeps for the configured duration; the scheduler loop continues to tick and `Evaluate()` does **not** return. The process status remains `ProcessStatusRunning` throughout. Other ready tasks continue to be dispatched and advance concurrently; only the token at the pause node is held.

Use pause events when:

- The wait is short (seconds to a few minutes).
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
rateLimit := PauseForDuration("rate-limit", "Rate Limit", bl.DaysTimeDuration("PT2S"))
```

---

## Waiting for External Input

Event nodes only handle timer-based waits. To wait for input from an external system (a human, another service, etc.), use the [`RequestInputTask`](task-nodes.spec.md#requestinputtask) task node:

- The task emits an `InstanceEventInputRequest` on the instance's topic via the gateway, carrying a `requestID`.
- External subscribers (REST, MCP, web UI) call `gw.RespondToInputRequest(ctx, instanceID, requestID, payload)` to deliver the response.
- The task's `ResponseContract` validates the payload; on success the response is written into the `ExecutionContext` and the process resumes.

There is no lower-level "deliver an arbitrary message to a process" verb in v1 — `RequestInputTask` + `RespondToInputRequest` is the only path, which keeps the input contract enforceable on both ends.

---

## Validation Timing Reference

| Event node | When validated/run | Effect |
|---|---|---|
| `StartEvent` | At submission, before any token is placed | `Inputs` contract validated; failure → `DataContractValidationError`, submission rejected synchronously |
| `EndEvent` | When token reaches it, before `PROCESS_COMPLETED` | `Outputs` contract (if set) validated; failure → `PROCESS_FAILED` |
| `CancelEvent` | When token reaches it | `Outputs` contract (if set) validated; success → `PROCESS_CANCELLED`; validation failure → `PROCESS_FAILED` |
| `ErrorEvent` | When token reaches it | Always → `PROCESS_FAILED` with `ErrorRef`. No contract |
| `TerminateEvent` | When token reaches it | All other branches cancelled, then `PROCESS_CANCELLED` |
| `SuspendForDuration` / `SuspendUntilDatetime` | When token reaches it | Wake time computed and persisted; status → `SUSPENDED`; `Evaluate()` returns |
| `PauseForDuration` | When token reaches it | Goroutine blocks on timer; status remains `RUNNING`; other branches advance |

---

## Edge Cases

- A process with no terminating event reachable from a `StartEvent` produces a `ProcessDefinitionError` at construction time. "Terminating event" means any of `EndEvent`, `CancelEvent`, `ErrorEvent`, `TerminateEvent`.
- Duplicate ids across **all** event-node types within a single process produce a `ProcessDefinitionError`. The id namespace is shared across event subtypes.
- `Cancel`, `Error`, `Terminate`, and additional `End` nodes all consume their token. A `TerminateEvent` reached on one branch cancels other branches' in-flight tasks before they can reach their own terminating events.
- An `ErrorEvent` whose `ErrorRef` is empty produces a `ProcessDefinitionError`. The error code is part of the process's observable contract — silent failures are not allowed via this node.
- A `SuspendUntilDatetime` whose evaluated `Deadline` is in the past wakes immediately on the next scheduler tick — this is not an error.
- A `TerminateEvent` reached while another branch is mid-task: the in-flight task's goroutine is cancelled (per `Evaluate()`'s existing branch-cancellation behaviour). Side effects already committed by the cancelled task are not rolled back — blkit has no compensation.
- A `CancelEvent` and `EndEvent` reached on different parallel branches of the same instance: whichever token arrives first wins. The other branches are cancelled. The status reflects the winning node's type (`Cancelled` for `Cancel`, `Completed` for `End`).
- An `ErrorEvent` whose token races with an `EndEvent` on a parallel branch: the `ErrorEvent` wins by precedence — failure is sticky. Status is `Failed`.
