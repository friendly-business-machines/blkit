---
name: Task Nodes
description: Task types for business processes — NativeFunctionTask, DecisionTask, SubProcessTask, TriggerProcessTask, RequestInputTask
targets:
  - ../processes/task.go
  - ../processes/subprocess_task.go
  - ../processes/trigger_process_task.go
  - ../processes/request_input_task.go
---

# Task Nodes

A `Task` is a unit of work in a business process. All tasks implement `ProcessNode` and can be placed in a process graph via `.To()`. Every task receives the current `ExecutionContext` when executed and returns an updated `Variables` map (which is merged back into the context).

## Type Hierarchy

```
ProcessNode (interface)
├── StartEvent, EndEvent, CancelEvent, ErrorEvent, TerminateEvent,
│   SuspendForDurationEvent, SuspendUntilDatetimeEvent, SuspendUntilMessageEvent,
│   PauseForDurationEvent, PauseUntilMessageEvent
├── ParallelGateway, ExclusiveGateway, InclusiveGateway, JoinGateway
├── DecisionTask                 (decision logic + task framing in one type)
└── Task
    ├── NativeFunctionTask
    ├── SubProcessTask
    ├── TriggerProcessTask
    └── RequestInputTask
```

Event types are defined in [event-nodes.spec.md](event-nodes.spec.md); gateway types in [gateway-nodes.spec.md](gateway-nodes.spec.md); `DecisionTask` in [../decision-tasks/decision-task.spec.md](../decision-tasks/decision-task.spec.md).

`DecisionTask` is a `ProcessNode` in its own right, separate from `Task`. It does not embed `Task` and does not use the `Task.Add*` exit-port methods — its exit ports are configured via `DecisionTaskOpts.ExitPorts`. See [../decision-tasks/decision-task.spec.md](../decision-tasks/decision-task.spec.md) for the full definition.

## ProcessNode Interface

```go
type ProcessNode interface {
    ID() string
    Name() *string
    To(target ProcessNode) ProcessNode
    AddExecutionListener(event string, fn func(*ExecutionContext))
}
```

## Task (abstract)

```go
type Task struct {
    Id          string
    Name        *string
    Description *string

    // Variable mappings
    InputMappings  *VariableMapping
    OutputMappings *VariableMapping

    // Process-level loopback guard
    MaxExecutionsPerProcessInstance int // default: 50

    // Loop configuration
    Loop *LoopConfig // default: nil (no looping)

    // Multi-instance configuration
    MultiInstance *MultiInstanceConfig // default: nil (single execution)

    // Exit ports (timer, error, ...) attached to this task — keyed by exit-port id, unique within the task
    ExitPorts map[string]ExitPort // default: nil
}

// ExitPort is implemented by every exit-port type (TimerExitPort, ErrorExitPort, ...).
// Exit ports are registered on a task via Add* methods and looked up via task.ExitPort(id) for graph wiring.
type ExitPort interface {
    ProcessNode
    AttachedTask() Task
}

func (t *Task) AddExecutionListener(event string, fn func(*ExecutionContext)) { ... }


type LoopConfig struct {
    Condition     BlExpr // evaluated after each iteration; loop continues while true
    MaxIterations int    // upper bound to prevent infinite loops (default: 100)
}

func NewLoopConfig(condition BlExpr, maxIterations int) *LoopConfig { ... }


type MultiInstanceConfig struct {
    Collection      BlExpr // expression resolving to a BlList
    ElementVariable string // variable name bound to the current item in each iteration
    IsSequential    bool   // true = sequential, false = parallel (default: true)
}

func NewMultiInstanceConfig(collection BlExpr, elementVariable string, isSequential bool) *MultiInstanceConfig { ... }


type VariableMapping struct {
    Entries []VariableMappingEntry
}

func NewVariableMapping(entries ...[2]string) *VariableMapping { ... }


type VariableMappingEntry struct {
    Source string // variable name or FEEL expression
    Target string // target variable name in context
}
```

When a task executes, the runtime:
1. Fires `start` execution listeners.
2. Applies `input_mappings` to compute the task's input variables.
3. Invokes the task's handler with the (possibly mapped) context.
4. Merges the returned variables back, applying `output_mappings` if defined.
5. Fires `end` execution listeners.

If `loop` is configured, steps 2–4 repeat in a do-while pattern: the task executes once, then the loop condition is evaluated. If true, the task executes again with the updated context. This continues until the condition is false or `max_iterations` is reached.

If `multi_instance` is configured, steps 2–4 execute once per item in the collection. Each iteration receives a copy of the context with `element_variable` bound to the current item. When `is_sequential` is false, all iterations are dispatched concurrently. The task is not complete until all iterations complete. Results from all iterations are collected and merged back into the context.

`loop` and `multi_instance` are mutually exclusive — setting both produces a `ProcessDefinitionError`.

`max_executions_per_process_instance` limits how many times the runtime may dispatch this task during a single process execution. This guards against infinite loopbacks in the process graph. The counter tracks how many times the run loop dispatches the task as a graph node — iterations from `loop` or `multi_instance` do not count towards this limit. When the limit is reached, `Evaluate()` produces a `TaskExecutionLimitError`.

## DecisionTask

`DecisionTask` is a `ProcessNode` that evaluates a blkit decision (a directed graph of `DecisionNode`s) inline as part of a process. Unlike `NativeFunctionTask` and `SubProcessTask`, it does not embed the `Task` struct — it carries its own task-level fields (`Id`, `Name`, `InputMappings`, `OutputMappings`, `Loop`, `MultiInstance`, `MaxExecutionsPerProcessInstance`, `ExitPorts`) and is reused across processes via a `Clone(opts)` method rather than via the `Task.Add*` chaining pattern.

Exit ports on a `DecisionTask` are configured via `DecisionTaskOpts.ExitPorts` using the standalone constructors documented in this spec (`NewInterruptingTimerExitPort`, `NewErrorExitPort`, `NewInterruptingConditionalExitPort`, etc.). The `Task.Add*` methods do not apply.

See [../decision-tasks/decision-task.spec.md](../decision-tasks/decision-task.spec.md) for the full definition, the `Clone` semantics, and the reuse pattern.

```go
// Brief sketch — full definition in decision-task.spec.md
template := NewDecisionTask(DecisionTaskOpts{
    InputContract:  ic,
    OutputContract: oc,
})
template.AddNode(...)

riskCheck := template.Clone(DecisionTaskOpts{
    Id:   "risk-check",
    Name: "Risk Assessment",
    InputMappings:  NewVariableMapping(...),
    OutputMappings: NewVariableMapping(...),
})

underwriting := NewProcess("underwriting", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        Start("start", "Start", NewInputContract()).To(riskCheck).To(End("done", "Done")),
    },
})
```

---

## NativeFunctionTask

Invokes a Go function directly. The function is passed as a value at construction time and called in-process when the task executes.

```go
type NativeFunctionTask struct {
    Task
    Fn func(*ExecutionContext) (BlValue, error)
}

func NewNativeFunctionTask(id string, name string, fn func(*ExecutionContext) (BlValue, error), opts ...NativeFunctionOpts) *NativeFunctionTask { ... }

type NativeFunctionOpts struct {
    InputMappings      *VariableMapping
    OutputMappings     *VariableMapping
    MaxRetries         int
    InitialDelay       *time.Duration
    ExponentialBackoff bool
    Timeout            *time.Duration
    TimeoutBehavior    string // "error" (default), "abort", "wait"
}
```

- `Fn` receives the current `ExecutionContext` (after any `input_mappings` have been applied) and returns a `BlValue`.
- The return type is typically a `BlContext` for structured output, but any FEEL type is valid (e.g. `BlNumber`, `BlString`, `BlList`).

### Retry and Timeout

- **`max_retries`** — the maximum number of retry attempts after the initial failure. Default `0` (no retries). A value of `3` means up to 4 total executions (1 initial + 3 retries).
- **`initial_delay`** — the delay before the first retry. If `nil`, retries begin immediately.
- **`exponential_backoff`** — when `true`, the delay doubles after each retry (e.g. 1s, 2s, 4s, 8s).
- **`timeout`** — the total time budget for all attempts including delays. The timer starts when the first execution begins. If `nil`, there is no time limit.
- **`timeout_behavior`** — controls what happens when the timeout is reached while an attempt is in progress:
  - `"abort"` — kill the in-progress attempt immediately and fail the task.
  - `"wait"` — let the in-progress attempt finish. If it succeeds, the task succeeds. If it fails, no further retries are attempted.
  - `"error"` — let the in-progress attempt finish, then produce an error regardless of the outcome. This is the default.

### Writing a Native Function

A native function is a regular function in the host language. It receives mapped input variables as keyword arguments and returns a `BlValue`. The full `ExecutionContext` is also available as a keyword argument if the function declares a `ctx` parameter.

#### Using the execution context

The function can declare a `ctx: ExecutionContext` parameter to read variables from earlier tasks or inspect process state.

```go
// loan_app/scoring.go

func CalculateScore(ctx *ExecutionContext) (BlValue, error) {
    // Read outputs from earlier tasks and convert to native types
    isValid := ctx.Get("validate.is_valid").ToNativeBool()
    creditScore := ctx.Get("credit-report.score").ToNativeFloat()
    income := ctx.Get("check-income.annual_income").ToNativeFloat()

    if !isValid {
        return Bl.Context(map[string]BlExpr{
            "score":  Bl.Number(0),
            "reason": Bl.String("Failed validation"),
        }), nil
    }

    score := creditScore*0.6 + min(income/1000, 200)*0.4

    return Bl.Context(map[string]BlExpr{
        "score":  Bl.Number(score),
        "reason": Bl.String("Calculated from credit and income"),
    }), nil
}

// Wire it up as a NativeFunctionTask (no input mappings — reads from ctx directly)
calcScore := NewNativeFunctionTask("calc-score", "Calculate Score", scoring.CalculateScore)
```

#### Using mapped variables

When `input_mappings` are defined, they reshape the context before `Fn` is called — variables are accessible under their mapped names. This is useful for functions that are re-used across different processes, as it decouples the function from the specific variable paths in the calling process.

```go
// loan_app/validation.go

func ValidateApplication(ctx *ExecutionContext) (BlValue, error) {
    // Access variables by their mapped names
    applicant := ctx.Get("applicant").(*BlContext)
    loanAmount := ctx.Get("loan_amount").ToNativeFloat()

    name := applicant.Get("name").ToNativeString()
    age := applicant.Get("age").ToNativeInt()

    isValid := age >= 18 && loanAmount > 0
    notes := "Failed validation"
    if isValid {
        notes = "All checks passed"
    }

    return Bl.Context(map[string]BlExpr{
        "is_valid":         Bl.Boolean(isValid),
        "validation_notes": Bl.String(notes),
    }), nil
}

// Wire it up with input mappings in a process
validate := NewNativeFunctionTask("validate", "Validate Application",
    validation.ValidateApplication,
    NativeFunctionOpts{
        InputMappings: NewVariableMapping(
            [2]string{"start.applicant", "applicant"},
            [2]string{"start.loan_amount", "loan_amount"},
        ),
    },
)

loanProcess := NewProcess("loan-process", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(validate).To(End("done", "Done")),
    },
})

store := blkit.NewInMemoryStateStore()
ctx, hist, _ := store.NewExecutionState(loanProcess, NewExecutionStateOpts{
    StartId: "start",
    Input: map[string]any{
        "applicant":   Bl.Context(map[string]BlExpr{"name": Bl.String("Alice"), "age": Bl.Number(30)}),
        "loan_amount": Bl.Number(200000),
    },
})
result, _ := loanProcess.Evaluate(EvaluateOpts{Context: ctx, History: hist})
_ = result
```

---

## SubProcessTask

Executes another `Process` as a step within the parent process. The sub-process runs to completion before the parent continues.

```go
type SubProcessTask struct {
    Task
    ProcessID         string               // id of the Process to execute
    StartID           string               // the start node id to use as the entrypoint
    StartValueMapping *VariableMapping     // maps values from the parent execution context to the sub-process start node
}

func NewSubProcessTask(id string, name string, processID string, startID string, opts ...SubProcessTaskOpts) *SubProcessTask { ... }

type SubProcessTaskOpts struct {
    StartValueMapping *VariableMapping
}
```

- The referenced process is resolved by `process_id` at runtime from the blkit registry (populated by `NewProcess()`). If the process cannot be resolved, a `ProcessResolutionError` is thrown before execution.
- `start_id` specifies which start node in the referenced process to use as the entrypoint. A `SubProcessTask` with no `start_id` set is invalid; `ProcessDefinitionError` is produced.
- `start_value_mapping` maps variables from the parent `ExecutionContext` to the sub-process's start node input. If `nil`, the sub-process receives no input variables.
- The sub-process operates on a scoped view of the parent's shared `ExecutionContext` (`parentCtx.Scope(subProcessTaskNodeID)`). Its transactions are appended to the same log with NodeIDs prefixed by the `SubProcessTask`'s id, so parent tasks can access any sub-process node's output via chained dot notation (e.g. `ctx.Get("verify-step.check-docs.docs_valid")`). See [execution-context.spec.md](../data/execution-context.spec.md#sub-process-scoping).
- `loop` and `multi_instance` are inherited from `Task` and apply to the sub-process execution as a whole.

### Example

```go
// Reference a separately-defined verification process by id
docVerify := NewSubProcessTask("verify-step", "Verify Documents",
    "doc-verify", "start")

onboarding := NewProcess("onboarding", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(docVerify).To(End("done", "Done")),
    },
})
```

### Example — with start value mapping

```go
// Reference a payment process, mapping values from the parent context to its start node
processPayment := NewSubProcessTask("process-payment", "Process Payment",
    "payment", "start",
    SubProcessTaskOpts{
        StartValueMapping: NewVariableMapping(
            [2]string{"start.order_total", "amount"},
            [2]string{"start.order_currency", "currency"},
        ),
    },
)

orderFulfillment := NewProcess("order-fulfillment", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(processPayment).To(End("done", "Done")),
    },
})
```

---

## TriggerProcessTask

Submits another `Process` as an independent instance and continues immediately. Unlike `SubProcessTask`, the triggered process runs **out-of-band** — the parent does not wait for it, the two instances do not share an `ExecutionContext`, and no parent/ultimate-parent backref is recorded on the new instance.

Use it for genuine fan-out where one process kicks off another that should outlive (or complete independently of) the caller: dispatching notification flows, starting downstream reporting pipelines, triggering a one-shot integration workflow.

```go
type TriggerProcessTask struct {
    Task
    Namespace string  // empty = same package as the calling process
    ProcessID string  // id of the registered Process to submit
    Version   string  // version of the registered Process to submit
    StartID   string  // start node id to use as the entrypoint of the triggered process

    // Optional: maps values from the parent ExecutionContext to the triggered
    // process's start input. Same shape as SubProcessTask.StartValueMapping.
    StartValueMapping *VariableMapping

    // Optional: variable name to write the new instance's ProcessInstanceID
    // into the parent ExecutionContext. Useful for downstream tasks that want
    // to record / subscribe to / cancel the triggered run.
    InstanceIDVariable *string

    // Optional: correlation key forwarded to BrokerGateway events for the
    // triggered instance. Mirrored into every Event emitted for that instance.
    CorrelationKey *string
}

func NewTriggerProcessTask(id string, name string, processID string, startID string, opts ...TriggerProcessTaskOpts) *TriggerProcessTask { ... }

type TriggerProcessTaskOpts struct {
    Namespace          string
    Version            string
    StartValueMapping  *VariableMapping
    InstanceIDVariable *string
    CorrelationKey     *string
}
```

### Resolution and submission

- The target process is resolved at task execution time via `blkit.LookupProcess(Namespace, ProcessID, Version)`. If `Namespace` is empty, the runtime uses the calling process's namespace. An unresolvable `(Namespace, ProcessID, Version)` produces a `ProcessResolutionError`, which is catchable via `ErrorExitPort`.
- The submitted input is built from the parent `ExecutionContext` via `StartValueMapping` (no mapping = empty input map).
- The input is validated against the target's `StartEvent.InputContract` at submit time. Validation failure produces a `DataContractValidationError`, also catchable via `ErrorExitPort`.
- The runtime hands off to whichever submission path is configured for the current execution mode:
  - `BrokerGateway.Submit` when a broker is configured (worker / FAAS mode) — see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md#interface).
  - Direct in-process spawn when no broker is configured (direct `Process.Evaluate` callers).
- The submit returns the new `processInstanceID`. If `InstanceIDVariable` is set, the id is written under that name into the parent `ExecutionContext` before the task completes.

### Async vs. inline

`TriggerProcessTask` and `SubProcessTask` look similar but solve different problems:

| | `SubProcessTask` | `TriggerProcessTask` |
|---|---|---|
| Execution | Inline — parent blocks until child completes | Out-of-band — parent continues immediately |
| Shared `ExecutionContext` | Yes (scoped view: `parentCtx.Scope(id)`) | No — independent context built from `StartValueMapping` |
| Parent/ultimate-parent link on child `ProcessTask` | Set | `nil` |
| History | Child's transactions appended to parent's history | Separate `ExecutionHistory` per the new instance |
| Output back to parent | Child's outputs accessible via dotted lookup | None — parent only learns the `processInstanceID` (via `InstanceIDVariable`) |
| Retries / `MaxRetries` | n/a (errors propagate inline) | n/a — the task completes on submit success; retry is the caller's concern (wrap in `ErrorExitPort`) |

### Loop and Multi-Instance

`Loop` and `MultiInstance` are inherited from `Task` and apply to the submit step itself — each iteration submits a separate instance. This is the canonical "trigger one instance per item in a list" pattern.

### Example — fire-and-forget notification

```go
ptr := func(s string) *string { return &s }

sendNotification := NewTriggerProcessTask("send-notification", "Send Notification Async",
    "notification-process", "start",
    TriggerProcessTaskOpts{
        StartValueMapping: NewVariableMapping(
            [2]string{"start.user_id", "user_id"},
            [2]string{"approve.outcome", "outcome"},
        ),
        InstanceIDVariable: ptr("notification_instance_id"),
    },
)
sendNotification.AddErrorExitPort("submit-failed")

handleSubmitFailure := NewNativeFunctionTask("submit-failed", "Log Submit Failure", logging.LogSubmitFailure)

approval := NewProcess("approval", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(approve).To(sendNotification).To(End("done", "Done")),
        sendNotification.ExitPort("submit-failed").To(handleSubmitFailure).To(End("logged", "Logged")),
    },
})
```

### Example — multi-instance fan-out

Trigger one downstream process per item in a list, in parallel.

```go
triggerReports := NewTriggerProcessTask("trigger-reports", "Trigger Report Run",
    "report-process", "start",
    TriggerProcessTaskOpts{
        StartValueMapping: NewVariableMapping(
            [2]string{"report_target", "target"},
        ),
    },
)
triggerReports.MultiInstance = NewMultiInstanceConfig(
    Bl.ListVar("start.report_targets"),
    "report_target",
    false, // dispatch in parallel
)
```

---

## RequestInputTask

Publishes a typed input-request event onto the `BrokerGateway` and waits for a correlated response delivered via `BrokerGateway.DeliverMessage`. The response payload is written back into the `ExecutionContext` under `PayloadVariable`. The wait can be durable (suspend), in-memory (pause), or a hybrid that pauses briefly and then suspends if no response arrives.

`RequestInputTask` consolidates the common "emit request → suspend / pause for reply" pattern that previously required a `NativeFunctionTask` followed by a `SuspendUntilMessage` (see [event-nodes.spec.md](event-nodes.spec.md#pattern-request-input-from-a-human-or-another-system)). It supports both human-in-the-loop and system-to-system input requests — blkit does not distinguish between the two; the responder simply calls `BrokerGateway.DeliverMessage` against the correct `(processInstanceID, messageRef)`.

```go
type RequestInputTask struct {
    Task
    MessageRef string  // correlation key matched by BrokerGateway.DeliverMessage

    // Expression evaluated against the ExecutionContext at task start to build
    // the payload that accompanies the EventMessageRequest. Typical use:
    // describe to the recipient what is being requested (form schema, prompt
    // text, current case state, etc.).
    RequestPayload BlExpr // resolves to a BlContext

    // Optional contract validated against the response payload before it is
    // written to the ExecutionContext. Validation failure raises a task error
    // with code "INPUT_CONTRACT_VIOLATION", catchable via ErrorExitPort.
    ResponseContract *InputContract

    // Optional variable name to write the (validated) response payload into
    // the parent ExecutionContext.
    PayloadVariable *string

    // How the runtime holds the token while waiting for the response. See
    // "Wait modes" below.
    WaitMode      RequestInputWaitMode
    PauseDuration BlExpr // BlDaysTimeDuration; required iff WaitMode == RequestInputPauseThenSuspend
}

type RequestInputWaitMode int

const (
    RequestInputSuspend          RequestInputWaitMode = iota // durable from the start
    RequestInputPause                                        // in-memory only
    RequestInputPauseThenSuspend                             // pause for PauseDuration, then suspend
)

func NewRequestInputTask(id string, name string, messageRef string, requestPayload BlExpr, opts ...RequestInputTaskOpts) *RequestInputTask { ... }

type RequestInputTaskOpts struct {
    ResponseContract *InputContract
    PayloadVariable  *string
    WaitMode         RequestInputWaitMode // default RequestInputSuspend
    PauseDuration    BlExpr
}
```

### Lifecycle

1. Task starts → runtime evaluates `RequestPayload` against the current `ExecutionContext`.
2. Runtime publishes `Event{Kind: EventMessageRequest, MessageRequest: {NodeID, MessageRef, Payload}}` via `BrokerGateway.PublishEvent` (see [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md#event-types)). `ctx.ProcessInstanceID()` is mirrored onto the event by the runtime so subscribers can route the response back via `BrokerGateway.DeliverMessage(processInstanceID, messageRef, payload)`.
3. The token enters the configured wait mode (see below).
4. On `DeliverMessage` arrival the runtime validates the payload against `ResponseContract` if set, writes it under `PayloadVariable` if set, then resumes past the task.

### Wait modes

- **`RequestInputSuspend`** (default): the runtime suspends the process immediately on entering the task — `ExecutionHistory` records a suspension step, `StateStore.Save` persists state, `Evaluate()` returns with status `ProcessStatusSuspended`. Semantics are identical to `SuspendUntilMessageEvent`. FAAS-compatible.
- **`RequestInputPause`**: the runtime parks the goroutine on an in-memory channel — status remains `ProcessStatusRunning`, no `StateStore` write occurs, other parallel branches continue advancing. Semantics are identical to `PauseUntilMessageEvent`. Not FAAS-addressable (no goroutine survives invocation), and the wait is lost across worker restarts.
- **`RequestInputPauseThenSuspend`**: the runtime starts in pause mode and, if no response arrives within `PauseDuration`, **converts** the wait to a suspension. The token's position does not change; only the wait substrate is swapped — `ExecutionHistory` records a suspension step, `StateStore.Save` persists state, and `Evaluate()` returns. A subsequent `DeliverMessage` then drives a continuation in the usual way. Use this for "fast path is in-memory, slow path is durable" workloads (e.g. a human approval that usually returns in seconds but occasionally takes hours). See [../worker/worker.spec.md](../worker/worker.spec.md) for the worker-side pause-to-suspend conversion.

### Exit ports

All exit-port kinds inherited from `Task` are supported — `TimerExitPort`, `ErrorExitPort`, `ConditionalExitPort`. **Attaching an interrupting `TimerExitPort` is recommended best practice** so that a stalled responder does not leave the process suspended indefinitely. An `ErrorExitPort` is the standard way to recover from `INPUT_CONTRACT_VIOLATION` or `ProcessResolutionError`-style failures during the request emission.

### Correlation

Same `(processInstanceID, MessageRef)` correlation as `SuspendUntilMessageEvent` / `PauseUntilMessageEvent`. blkit does not provide content-based correlation; the responder must address `DeliverMessage` to the exact `processInstanceID` and `messageRef`. The `processInstanceID` is included automatically in the published `MessageRequest` event so subscribers (UIs, integration adapters) can build the response correctly.

### Loop and Multi-Instance

Both are supported. `MultiInstance` is the canonical "ask N reviewers in parallel" pattern — each iteration emits its own `MessageRequest` event with a unique iteration-scoped `messageRef` (see edge cases below for the disambiguation rule).

### Example — request approval from a human with a 24-hour SLA

```go
ptr := func(s string) *string { return &s }

requestApproval := NewRequestInputTask(
    "request-approval",
    "Request Manager Approval",
    "approval-response",
    Bl.Context(map[string]BlExpr{
        "prompt":      Bl.String("Approve loan?"),
        "applicant":   Bl.ContextVar("start.applicant"),
        "loan_amount": Bl.NumberVar("start.loan_amount"),
    }),
    RequestInputTaskOpts{
        ResponseContract: NewInputContract(
            RequiredField("approved", BlBoolean),
            OptionalField("notes", BlString),
        ),
        PayloadVariable: ptr("approval"),
        WaitMode:        RequestInputPauseThenSuspend,
        PauseDuration:   Bl.DaysTimeDuration("PT30S"),
    },
)
requestApproval.AddInterruptingWaitForDuration("sla", Bl.DaysTimeDuration("P1D"))

escalate := NewNativeFunctionTask("escalate", "Escalate", approvals.Escalate)
decide   := NewNativeFunctionTask("decide", "Apply Decision", approvals.Apply)

loanApproval := NewProcess("loan-approval", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(requestApproval).To(decide).To(End("done", "Done")),
        requestApproval.ExitPort("sla").To(escalate).To(End("escalated", "Escalated")),
    },
})
```

### Example — system-to-system input request (no human, pure async integration)

The responder is another service that posts results back via `BrokerGateway.DeliverMessage` once it has computed the requested value.

```go
ptr := func(s string) *string { return &s }

requestScore := NewRequestInputTask(
    "request-score",
    "Request Risk Score",
    "risk-score-response",
    Bl.Context(map[string]BlExpr{
        "applicant_id": Bl.StringVar("start.applicant_id"),
        "model":        Bl.String("v3"),
    }),
    RequestInputTaskOpts{
        ResponseContract: NewInputContract(
            RequiredField("score", BlNumber),
            RequiredField("model_version", BlString),
        ),
        PayloadVariable: ptr("risk"),
        WaitMode:        RequestInputSuspend,
    },
)
```

---

## Loop and Multi-Instance

Every task — `NativeFunctionTask`, `SubProcessTask`, and `DecisionTask` — can be configured with `loop` or `multi_instance` to repeat its execution. For `Task`-embedded types (`NativeFunctionTask`, `SubProcessTask`), the fields are inherited from `Task` and can be assigned directly. For `DecisionTask`, they are passed via `DecisionTaskOpts` at creation or clone time (see [../decision-tasks/decision-task.spec.md](../decision-tasks/decision-task.spec.md)).

### Loop Example

```go
// Re-run a credit lookup while the score is unavailable, up to 3 attempts
fetchScore := NewNativeFunctionTask("fetch-score", "Fetch Credit Score", credit.FetchScore)
fetchScore.Loop = NewLoopConfig(
    Bl.StringVar("fetch-score.status").Equals(Bl.String("pending")),
    3,
)
```

The task executes once, then the loop condition is evaluated against the updated context. If true, the task executes again. This repeats until the condition is false or `max_iterations` is reached.

### Multi-Instance Example

```go
// Validate each applicant in a list
validate := NewNativeFunctionTask("validate", "Validate Applicant", applicants.Validate)
validate.MultiInstance = NewMultiInstanceConfig(
    Bl.ListVar("start.applicants"),
    "applicant",
    false, // evaluate all in parallel
)
```

The task executes once per item in `applicants`. Each iteration receives the full context with `applicant` bound to the current item. All iterations run in parallel. The task is complete when all iterations finish.

---

## Timer Exit Port

Any task can be configured with one or more `TimerExitPort` entries — task-level shorthand for BPMN time boundary events. Each exit port has an `id` (unique within its task) and is attached to the task at construction time. The timer for each exit port starts when the task starts executing and either redirects flow (interrupting) or spawns a parallel branch (non-interrupting) when it fires.

```go
type TimerExitPort struct {
    Id string // unique within the owning task

    // Exactly one of these must be set
    WaitForDuration   BlExpr // BlDaysTimeDuration; relative to task start
    WaitUntilDateTime BlExpr // BlDateTime; absolute deadline

    IsInterrupting bool // true = cancel task on fire; false = task continues, parallel branch spawned

    AttachedTo Task        // back-reference to the owning task; set by the constructor
    OnTimeout  ProcessNode // flow target; set by .To() during graph construction
}
```

`TimerExitPort` implements the `ExitPort` interface (and therefore `ProcessNode`).

### Configuring exit ports on a task

Exit ports are added to a task via methods on the embedded `Task`. Each method sets up the timing config and registers the exit port under its `id` in `Task.ExitPorts`. All four methods are inherited by `NativeFunctionTask` and `SubProcessTask`.

```go
func (t *Task) AddInterruptingWaitForDuration(id string, duration BlExpr) *TimerExitPort
func (t *Task) AddNonInterruptingWaitForDuration(id string, duration BlExpr) *TimerExitPort
func (t *Task) AddInterruptingWaitUntilDateTime(id string, deadline BlExpr) *TimerExitPort
func (t *Task) AddNonInterruptingWaitUntilDateTime(id string, deadline BlExpr) *TimerExitPort
```

### Standalone constructors

For configuring exit ports on a `DecisionTask` (which does not use the `Task.Add*` methods — see [../decision-tasks/decision-task.spec.md](../decision-tasks/decision-task.spec.md)), use the standalone constructors. Each returns a `*TimerExitPort` with its timing configured and no flow target; the flow target is wired via `task.ExitPort(id).To(...)` in the graph block.

```go
func NewInterruptingTimerExitPort(id string, duration BlExpr) *TimerExitPort
func NewNonInterruptingTimerExitPort(id string, duration BlExpr) *TimerExitPort
func NewInterruptingTimerExitPortUntilDateTime(id string, deadline BlExpr) *TimerExitPort
func NewNonInterruptingTimerExitPortUntilDateTime(id string, deadline BlExpr) *TimerExitPort
```

### Wiring in the graph

`Task.ExitPort(id)` returns the registered exit port as a graph node. The exit port's `.To(target)` sets the exit port's flow target (`OnTimeout`) and returns `target`, enabling chaining inside the `Graph` list.

```go
func (t *Task) ExitPort(id string) ExitPort
```

Looking up an unregistered `id` produces a `ProcessDefinitionError` at graph construction time.

### Behaviour

- **Interrupting** (`IsInterrupting: true`): when the timer fires, the running task is cancelled, its normal outgoing edge is **not** taken, and execution continues from `OnTimeout`.
- **Non-interrupting** (`IsInterrupting: false`): when the timer fires, the task continues running. A new concurrent token is placed at `OnTimeout`, creating a parallel branch that proceeds independently. When the task later completes, its normal outgoing edge is taken as usual.

In both variants, every pending timer attached to the task is cancelled when the task completes.

### Graph Discovery

`Process.create()` walks edges from each task's normal outgoing flows **and** from every registered exit port's flow target. Exit ports with no flow target set (i.e. never wired via `.To()` in the `Graph` list) produce a `ProcessDefinitionError`.

### Interaction with Loop and Multi-Instance

The timers cover the task's execution as a whole, not individual iterations. A timer firing during a `Loop` or `MultiInstance` task interrupts (or runs alongside) the entire task, not a single iteration.

### Example — SLA deadline (interrupting wait-for-duration)

The most common case: enforce a service-level deadline. If the task has not completed within 1 hour, cancel it and route to an escalation path.

```go
processOrder := NewNativeFunctionTask("process", "Process Order", orders.Process)
processOrder.AddInterruptingWaitForDuration("sla-deadline", Bl.DaysTimeDuration("PT1H"))

escalate := NewNativeFunctionTask("escalate", "Escalate to Manager", orders.Escalate)

orderFlow := NewProcess("order-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(processOrder).To(End("processed", "Processed")),
        processOrder.ExitPort("sla-deadline").To(escalate).To(End("escalated", "Escalated")),
    },
})
```

### Example — Periodic reminder (non-interrupting wait-for-duration)

A non-interrupting timer to nudge a long-running task while it continues. The task itself is unaffected; the reminder runs as a parallel branch.

```go
reviewTask := NewNativeFunctionTask("review", "Review Application", review.Run)
reviewTask.AddNonInterruptingWaitForDuration("reminder", Bl.DaysTimeDuration("PT4H"))

remind := NewNativeFunctionTask("remind", "Send Reminder", notifications.Remind)

reviewFlow := NewProcess("review-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(reviewTask).To(End("done", "Done")),
        reviewTask.ExitPort("reminder").To(remind).To(End("reminded", "Reminded")),
    },
})
```

### Example — Absolute deadline from process input (wait-until-datetime)

The deadline is carried in the process input rather than fixed at definition time. `Bl.DateTimeVar` resolves the deadline against the `ExecutionContext` when the timer is armed.

```go
runAuction := NewSubProcessTask("auction", "Run Auction", "auction-process", "start")
runAuction.AddInterruptingWaitUntilDateTime("auction-end", Bl.DateTimeVar("start.auction_end_time"))

closeAuction := NewNativeFunctionTask("close", "Close Auction", auction.Close)

auctionFlow := NewProcess("auction-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(runAuction).To(End("decided", "Decided")),
        runAuction.ExitPort("auction-end").To(closeAuction).To(End("expired", "Expired")),
    },
})
```

### Example — Multiple exit ports on one task

A loan review with both an interrupting escalation deadline (24h hard cap) and a non-interrupting reminder fired at a deadline carried in process input. Both are attached to the same task.

```go
reviewTask := NewNativeFunctionTask("review", "Review Application", review.Run)
reviewTask.AddInterruptingWaitForDuration("escalation-timer", Bl.DaysTimeDuration("P1D"))
reviewTask.AddNonInterruptingWaitUntilDateTime("reminder-timer", Bl.DateTimeVar("start.reminder_at"))

escalate := NewNativeFunctionTask("escalate", "Escalate to Manager", review.Escalate)
remind   := NewNativeFunctionTask("remind", "Send Reminder", notifications.Remind)
notify   := NewNativeFunctionTask("notify", "Notify Applicant", notifications.Decision)

loanReview := NewProcess("loan-review", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(reviewTask).To(notify).To(End("decided", "Decided")),
        reviewTask.ExitPort("escalation-timer").To(escalate).To(End("escalated", "Escalated")),
        reviewTask.ExitPort("reminder-timer").To(remind).To(End("reminded", "Reminded")),
    },
})
```

---

## Error Exit Port

Any task can be configured with one or more `ErrorExitPort` entries — task-level shorthand for BPMN error boundary events. When the task throws an error, a matching error exit port cancels the task and redirects flow to its target. Error exit ports are always **interrupting** — an errored task cannot continue.

```go
type ErrorExitPort struct {
    Id string // unique within the owning task

    ErrorRef *string // nil = catch any error; otherwise matches against the thrown error code

    // Optional capture variables — when set, the caught error's code/message
    // are written to the ExecutionContext under these names before flow continues.
    ErrorCodeVariable    *string
    ErrorMessageVariable *string

    AttachedTo Task        // back-reference to the owning task; set by the constructor
    OnError    ProcessNode // flow target; set by .To() during graph construction
}

type ErrorExitPortOpts struct {
    ErrorRef             *string
    ErrorCodeVariable    *string
    ErrorMessageVariable *string
}
```

`ErrorExitPort` implements the `ExitPort` interface (and therefore `ProcessNode`).

### Configuring error exit ports on a task

```go
func (t *Task) AddErrorExitPort(id string, opts ...ErrorExitPortOpts) *ErrorExitPort
```

### Standalone constructor

For `DecisionTask`, use the standalone constructor. It returns a `*ErrorExitPort` with the error-ref/code-variable/message-variable configured and no flow target.

```go
func NewErrorExitPort(id string, opts ...ErrorExitPortOpts) *ErrorExitPort
```

The same `Task.ExitPort(id)` lookup used for timer exit ports is used for error exit ports — they share the per-task id namespace, so registering an error exit port with the same id as an existing timer exit port (or vice versa) produces a `ProcessDefinitionError`.

### Behaviour

- An error thrown by the task is matched against each registered `ErrorExitPort` in declaration order.
  - An exit port with a non-nil `ErrorRef` matches when its value equals the thrown error code.
  - An exit port with `ErrorRef == nil` matches any error.
  - **Specific matches take precedence over catch-all matches.** If multiple specific exit ports are registered with the same `ErrorRef`, the first one declared wins.
- When an exit port matches: the task is cancelled, the error code and message are stored in the `ExecutionContext` under `ErrorCodeVariable`/`ErrorMessageVariable` (when set), the task's normal outgoing edge is **not** taken, and execution continues from `OnError`.
- When no exit port matches: the error propagates as a normal task failure (the process fails unless caught by a higher-level boundary).

### Graph Discovery

Identical to timer exit ports: every registered error exit port's `OnError` target is walked by `Process.create()`, and a registered exit port that is never wired via `.To()` in the `Graph` list produces a `ProcessDefinitionError`.

### Interaction with Loop, Multi-Instance, and Timer Exit Ports

- An error exit port applies to the task's execution as a whole. An error during any iteration of `Loop` or `MultiInstance` triggers the matching error exit port and cancels the entire task (including remaining iterations).
- If both an error exit port and a timer exit port become eligible to fire at the same instant, the error exit port wins (the task is in a failure state).
- Error exit ports do not catch `TimerEvaluationError` from the task's own timer exit ports — those are graph-definition errors and fail the process.

### Example — Catch-all error recovery

Route any failure of `processOrder` into a manual-review path.

```go
processOrder := NewNativeFunctionTask("process", "Process Order", orders.Process)
processOrder.AddErrorExitPort("any-error")

manualReview := NewNativeFunctionTask("manual", "Manual Review", orders.ManualReview)

orderFlow := NewProcess("order-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(processOrder).To(End("processed", "Processed")),
        processOrder.ExitPort("any-error").To(manualReview).To(End("reviewed", "Reviewed")),
    },
})
```

### Example — Specific error code with capture variables

Catch only `VALIDATION_FAILED`. Capture the error code and message into context variables so the correction task can read them.

```go
ptr := func(s string) *string { return &s }

validate := NewNativeFunctionTask("validate", "Validate Order", orders.Validate)
validate.AddErrorExitPort("validation-failed", ErrorExitPortOpts{
    ErrorRef:             ptr("VALIDATION_FAILED"),
    ErrorCodeVariable:    ptr("validation_error_code"),
    ErrorMessageVariable: ptr("validation_error_message"),
})

correct := NewNativeFunctionTask("correct", "Correct Order", orders.Correct)

validationFlow := NewProcess("validation-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(validate).To(End("ok", "OK")),
        validate.ExitPort("validation-failed").To(correct).To(End("corrected", "Corrected")),
    },
})
```

### Example — Multiple error exit ports with different recovery paths

Different error codes route to different recovery branches; uncaught errors fail the process.

```go
ptr := func(s string) *string { return &s }

payment := NewNativeFunctionTask("payment", "Process Payment", payment.Process)
payment.AddErrorExitPort("insufficient-funds", ErrorExitPortOpts{ErrorRef: ptr("INSUFFICIENT_FUNDS")})
payment.AddErrorExitPort("network-error", ErrorExitPortOpts{ErrorRef: ptr("NETWORK_ERROR")})

decline := NewNativeFunctionTask("decline", "Decline Order", orders.Decline)
retry   := NewNativeFunctionTask("retry", "Retry Payment", payment.Retry)

paymentFlow := NewProcess("payment-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(payment).To(End("captured", "Captured")),
        payment.ExitPort("insufficient-funds").To(decline).To(End("declined", "Declined")),
        payment.ExitPort("network-error").To(retry).To(End("retried", "Retried")),
    },
})
```

### Example — Combining error and timer exit ports on one task

A task that has both a deadline and an error recovery path. The two exit ports share the per-task id space, so each must have a distinct id.

```go
ptr := func(s string) *string { return &s }

charge := NewNativeFunctionTask("charge", "Charge Card", payment.Charge)
charge.AddInterruptingWaitForDuration("charge-deadline", Bl.DaysTimeDuration("PT30S"))
charge.AddErrorExitPort("declined", ErrorExitPortOpts{ErrorRef: ptr("CARD_DECLINED")})

timeoutHandler := NewNativeFunctionTask("timeout", "Handle Timeout", payment.HandleTimeout)
declineHandler := NewNativeFunctionTask("decline", "Handle Decline", payment.HandleDecline)

checkout := NewProcess("checkout", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(charge).To(End("captured", "Captured")),
        charge.ExitPort("charge-deadline").To(timeoutHandler).To(End("timed-out", "Timed Out")),
        charge.ExitPort("declined").To(declineHandler).To(End("declined", "Declined")),
    },
})
```

---

## Conditional Exit Port

Any task can be configured with one or more `ConditionalExitPort` entries — task-level shorthand for BPMN conditional boundary events. Each exit port has an `id` (unique within its task) and a `BlExpr` condition evaluated against the `ExecutionContext`. The condition is re-evaluated each time the context is updated by another concurrent branch's tasks while this task is running. When the condition becomes true, the exit port fires — either redirecting flow (interrupting) or spawning a parallel branch (non-interrupting).

```go
type ConditionalExitPort struct {
    Id string // unique within the owning task

    Condition BlExpr // resolves to BlBoolean; evaluated against the ExecutionContext

    IsInterrupting bool // true = cancel task on fire; false = task continues, parallel branch spawned

    AttachedTo Task        // back-reference to the owning task; set by the constructor
    OnTrue     ProcessNode // flow target; set by .To() during graph construction
}
```

`ConditionalExitPort` implements the `ExitPort` interface (and therefore `ProcessNode`).

### Configuring conditional exit ports on a task

```go
func (t *Task) AddInterruptingConditional(id string, condition BlExpr) *ConditionalExitPort
func (t *Task) AddNonInterruptingConditional(id string, condition BlExpr) *ConditionalExitPort
```

The same `Task.ExitPort(id)` lookup used for timer and error exit ports is used for conditional exit ports — they share the per-task id namespace, so registering a conditional exit port with the same id as an existing timer or error exit port (or vice versa) produces a `ProcessDefinitionError`.

### Standalone constructors

For `DecisionTask`, use the standalone constructors:

```go
func NewInterruptingConditionalExitPort(id string, condition BlExpr) *ConditionalExitPort
func NewNonInterruptingConditionalExitPort(id string, condition BlExpr) *ConditionalExitPort
```

### Behaviour

- The condition is evaluated when the task starts and again whenever a concurrent branch's task completes and writes to the `ExecutionContext`. The condition is **not** re-evaluated during the running task's own internal state changes — only against externally-visible context updates.
- The first context update that makes the condition true fires the exit port. Once fired, the exit port does not re-fire even if the condition oscillates.
- An exit port whose condition is already true at task start fires on the first scheduler tick.
- **Interrupting**: the running task is cancelled, its normal outgoing edge is **not** taken, and execution continues from `OnTrue`. All pending timer exit ports on the task are cancelled.
- **Non-interrupting**: the task continues running. A new concurrent token is placed at `OnTrue`, creating a parallel branch. When the task later completes, its normal outgoing edge is taken as usual.

In both variants, the exit port is unregistered when the task completes — its `OnTrue` target is not reached if the condition never becomes true during execution.

### Graph Discovery

Identical to timer and error exit ports: every registered conditional exit port's `OnTrue` target is walked by `Process.create()`, and a registered exit port that is never wired via `.To()` in the `Graph` list produces a `ProcessDefinitionError`.

### Interaction with Loop, Multi-Instance, and other exit ports

- A conditional exit port applies to the task's execution as a whole. A condition becoming true during any iteration of `Loop` or `MultiInstance` triggers the exit port and (if interrupting) cancels the entire task.
- If a conditional and a timer exit port become eligible to fire simultaneously, the timer wins (the deterministic clock-driven fire takes precedence over the condition that the same context-update tick may also have made true).
- An error exit port firing on the task takes precedence over a conditional exit port that may also be eligible — failure is sticky.

### Example — Cancel a long task when an external flag flips

```go
processOrder := NewNativeFunctionTask("process", "Process Order", orders.Process)
processOrder.AddInterruptingConditional("kill-switch",
    Bl.BooleanVar("flags.cancel_in_flight"))

cleanup := NewNativeFunctionTask("cleanup", "Roll Back Partial Work", orders.Cleanup)

orderFlow := NewProcess("order-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(processOrder).To(End("processed", "Processed")),
        processOrder.ExitPort("kill-switch").To(cleanup).To(End("cancelled", "Cancelled")),
    },
})
```

A concurrent branch (or external `DeliverMessage` call) writes `flags.cancel_in_flight = true`; the conditional exit port fires, `processOrder` is cancelled, and flow continues to `cleanup`.

### Example — Fire a side-task when a threshold is crossed (non-interrupting)

```go
monitor := NewNativeFunctionTask("monitor", "Monitor Inventory", inventory.Monitor)
monitor.AddNonInterruptingConditional("low-stock",
    Bl.NumberVar("inventory.stock_level").LessThan(Bl.Number(10)))

reorder := NewNativeFunctionTask("reorder", "Trigger Reorder", inventory.Reorder)

inventoryFlow := NewProcess("inventory-flow", "1.0", ProcessOpts{
    Graph: []Edge{
        Start("start", "Start", NewInputContract()).To(monitor).To(End("done", "Done")),
        monitor.ExitPort("low-stock").To(reorder).To(End("reordered", "Reordered")),
    },
})
```

The `monitor` task continues running; when stock drops below 10, the reorder branch starts in parallel.

---

## Edge Cases

- A task with no `id` set is invalid; construction raises a `ValidationError`.
- `input_mappings` and `output_mappings` with zero entries are valid (no-op).
- A `NativeFunctionTask` whose `Fn` is nil is invalid and produces a `ValidationError`.
- Setting both `loop` and `multi_instance` on the same task produces a `ProcessDefinitionError`.
- A loop that reaches `max_iterations` stops iterating and completes normally — it is not an error.
- A loop whose condition is false on the first evaluation still executes the task once (do-while semantics).
- Multi-instance with an empty collection executes zero iterations and immediately completes; this is not an error.
- Multi-instance with `is_sequential=false` dispatches all iterations concurrently. The task is not complete until all iterations complete.
- Multi-instance results are collected as a `BlList` of `Variables` maps, one per iteration, in collection order.
- When `max_executions_per_process_instance` is reached due to process-level loopbacks, `Evaluate()` produces a `TaskExecutionLimitError` and the process fails. Loop iterations (`LoopConfig`) and multi-instance iterations (`MultiInstanceConfig`) do not count towards this limit.
- A `SubProcessTask` with no `process_id` set produces a `ProcessDefinitionError`.
- A `SubProcessTask` whose `process_id` cannot be resolved produces a `ProcessResolutionError` before execution.
- Adding an exit port (timer or error) with an `id` that already exists on the task produces a `ProcessDefinitionError`. Exit-port ids are unique per task across all exit-port types; two tasks may both have an exit port called `"escalation"`.
- Calling `ExitPort(id)` for an `id` that has not been registered on the task produces a `ProcessDefinitionError` at graph construction time.
- A registered exit port that is never wired via `.To()` in the `Graph` list (i.e. has a nil flow target after `Process.create()`) produces a `ProcessDefinitionError`.
- `WaitForDuration` must evaluate to `BlDaysTimeDuration`; `WaitUntilDateTime` must evaluate to `BlDateTime`. A type mismatch produces a `ProcessDefinitionError` at validation time (where statically determinable) or a `TimerEvaluationError` at runtime.
- A `WaitUntilDateTime` whose evaluated time is in the past at task start fires immediately on the next scheduler tick.
- All pending timers for a task are cancelled when the task completes — their `OnTimeout` targets are not reached via the timer path.
- A non-interrupting timer that fires while the task is still running does not affect the task's completion or its normal outgoing flow; the parallel branch from `OnTimeout` proceeds independently.
- An interrupting timer firing on a task that has other pending non-interrupting timers cancels all of them along with the task.
- When an `ErrorExitPort` matches a thrown error, the task's outgoing flow is **not** taken; flow goes to `OnError` only. All pending timer exit ports on the task are cancelled.
- An `ErrorExitPort` with `ErrorRef == nil` matches any error code. If both a specific `ErrorRef` exit port and a catch-all are registered, the specific one wins when its code matches; the catch-all only fires when no specific exit port matches.
- If no `ErrorExitPort` matches the thrown error, the error propagates as a normal task failure.
- `ErrorCodeVariable` and `ErrorMessageVariable` are written to the `ExecutionContext` before flow continues to `OnError`. Re-using an existing variable name overwrites the prior value.
- An `ErrorExitPort` does not catch errors raised by the task's own configuration (e.g. `TimerEvaluationError` from a malformed timer expression) — those are process definition errors.
- A `ConditionalExitPort` whose `Condition` does not resolve to `BlBoolean` produces a `ConditionalEvaluationError` at runtime; the task fails. Static type-checking catches this at definition time where determinable.
- A `ConditionalExitPort` only re-evaluates its condition on context updates from concurrent branches, not on writes the running task itself produces. This matters for `MultiInstance` and `Loop`: only between-iteration writes from sibling branches trigger re-evaluation, not the task's own incremental state.
- A `ConditionalExitPort` that is true at task start fires on the first scheduler tick (does not require a context update to fire).
- Once a `ConditionalExitPort` fires, it does not re-fire even if the condition oscillates back to true. To trigger again, the process must reach the task again on a separate dispatch.
- A `TriggerProcessTask` whose `(Namespace, ProcessID, Version)` cannot be resolved via `blkit.LookupProcess` produces a `ProcessResolutionError` at task execution time. The error is catchable via `ErrorExitPort`.
- A `TriggerProcessTask` whose mapped input fails the target's `StartEvent.InputContract` produces a `DataContractValidationError`, catchable via `ErrorExitPort`. The new instance is not created in this case.
- A `TriggerProcessTask` completes when the submit is accepted by the runtime; it does **not** wait for the triggered instance to complete. The triggered instance's eventual success or failure has no effect on the parent.
- A `TriggerProcessTask` with `Loop` or `MultiInstance` submits a separate instance per iteration. The iterations' `processInstanceID`s are written into the multi-instance result list under `InstanceIDVariable` (when set), one entry per iteration.
- A `TriggerProcessTask` does not link the new instance as a child: `ParentProcessInstanceID` and `UltimateParentProcessInstanceID` on the submitted `ProcessTask` remain `nil`. Use `SubProcessTask` if a parent/child relationship is required.
- A `RequestInputTask` constructed with `WaitMode == RequestInputPauseThenSuspend` and a nil `PauseDuration` produces a `ProcessDefinitionError` at construction.
- A `RequestInputTask` constructed with any `WaitMode` other than `RequestInputPauseThenSuspend` and a non-nil `PauseDuration` produces a `ProcessDefinitionError` at construction — `PauseDuration` is meaningless in the other modes.
- A `RequestInputTask` whose response payload fails the configured `ResponseContract` raises a task error with `ErrorRef == "INPUT_CONTRACT_VIOLATION"`. The delivered message is **not** re-queued — the responder is expected to send a fresh `DeliverMessage` after correcting the payload, addressed to the same `(processInstanceID, messageRef)`.
- A `RequestInputTask` that reaches a terminal event (timer fires, parent cancelled, `TerminateEvent` reached on a sibling branch, etc.) before the response arrives has its pending message-wait unregistered. A late-arriving `DeliverMessage` for that `(processInstanceID, messageRef)` returns `NOT_WAITING` per [../messagebroker/overview.spec.md](../messagebroker/overview.spec.md#error-model).
- A `RequestInputTask` with `WaitMode == RequestInputPauseThenSuspend` whose `PauseDuration` is zero or negative is treated as `RequestInputSuspend` (no pause window).
- A `RequestInputTask` with `MultiInstance` emits one `EventMessageRequest` per iteration. Each iteration's wait registers under `(processInstanceID, messageRef + ":" + iterationIndex)` to disambiguate responders; the iteration index is also included in the published event payload so responders can address the correct iteration.
- A `RequestInputTask` running under `RequestInputPause` inside a FAAS invocation holds the invocation open until the response arrives or the task's `TimerExitPort` fires. Suspend modes are strongly preferred for FAAS — the spec does not mechanically prevent `RequestInputPause` in FAAS, but billing and cold-start behaviour discourage it.

---

## Out of Scope

The following task types are **intentionally not implemented** in blkit. They must not be added without an explicit design decision reversing this choice.

> **Revised 2026-05-12.** Two previously-omitted task types (`User task` and `Send task` in BPMN terms) are now covered by `RequestInputTask` and `TriggerProcessTask` respectively. The rows below reflect what remains out of scope; see the new sections above for the in-scope replacements.

| Task type | Reason omitted |
|---|---|
| Service task | External service calls (HTTP, gRPC, vendor SDKs, etc.) are expressed as `NativeFunctionTask` functions. Callers implement service adapters in their own libraries; blkit provides no connector or job-worker infrastructure. For *blkit-to-blkit* dispatch use `TriggerProcessTask`; for external-system sends use `NativeFunctionTask`. |
| Receive task | Passive waiting for inbound messages (no outbound request emitted) is expressed as `SuspendUntilMessage` or `PauseUntilMessage` event nodes (see [event-nodes.spec.md](event-nodes.spec.md)) rather than a task type. If you also need to *request* the input that you're waiting for, use `RequestInputTask` instead. blkit's correlation remains `(processInstanceID, messageRef)` only — content-based correlation and broad message brokering stay caller responsibilities. |
| Manual task | BPMN "manual task" describes work performed offline with no system involvement (paper forms, physical assembly, etc.). blkit has no event to record beyond a `NativeFunctionTask` that the operator's tooling marks complete via a synthetic call; there is nothing for the runtime to model. |
