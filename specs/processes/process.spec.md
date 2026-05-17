---
name: Process
description: The central class for defining and executing a business process — created via NewProcess() with a graph of node chains, evaluated via Evaluate() using token-flow semantics
targets:
  - ../processes/process.go
---

# Process

A `Process` defines a business process — a directed graph of `ProcessNode`s (tasks, gateways, events) connected by sequence flows. The design is inspired by BPMN but blkit does not implement the BPMN specification. Processes are created via `NewProcess()`, which accepts process metadata and a `graph` list of node chain expressions. The blkit registry caches each Process instance (populated by `NewProcess()`); long-running workers look it up by `(Namespace, Id, Version)` and call `Evaluate()` on it. Callers can also invoke `Evaluate()` directly to run a process without a worker.

```go
type Process struct {
    // Read-only attributes (set by NewProcess())
    Id                 string
    Version            string
    Namespace          string                     // auto-derived from the Go package import path of the NewProcess() caller; see "Registry"
    Name               *string
    Description        *string
    Graph              ProcessGraph               // processed, structured graph (built by NewProcess())
    MaxRunTime         *time.Duration
    MaxCompletionTime  *time.Duration
    Retry              *RetryConfig
}

func NewProcess(
    id string,
    version string,
    opts ProcessOpts,
) *Process

// ProcessOpts holds optional parameters for NewProcess()
type ProcessOpts struct {
    Name               *string
    Description        *string
    Graph              []ProcessNode              // raw chain expressions; processed into ProcessGraph
    MaxRunTime         *time.Duration             // measured from when execution begins, not when first queued
    MaxCompletionTime  *time.Duration             // measured from when the process instance is first queued
    Retry              *RetryConfig

    // Producer-side interruption opt-ins. Both default to false.
    //
    // When AllowExternalCancel is true, MessageGateway.Cancel (see
    // ../messagegateway/overview.spec.md) accepts external cancel requests for
    // instances of this process. The worker observes the request and injects
    // a synthetic CancelEvent into history (status -> Cancelled).
    //
    // When AllowExternalTerminate is true, MessageGateway.Terminate similarly
    // accepts external terminate requests, injecting a synthetic TerminateEvent
    // (status -> Completed, all branches cancelled).
    //
    // When false, the corresponding gateway call returns ErrCancelNotAllowed
    // / ErrTerminateNotAllowed and the process runs to graph-driven completion
    // only.
    AllowExternalCancel    bool
    AllowExternalTerminate bool
}

// Walks the graph using token-flow semantics, dispatching ready tasks as
// goroutines and awaiting their completion, until the process completes,
// suspends waiting for an external event, or fails.
// The process instance is reusable — Evaluate() does not mutate the process object.
func (p *Process) Evaluate(opts EvaluateOpts) (*EvaluationResult, error)

// EvaluateOpts holds parameters for Evaluate(). Both Context and History are
// required and must be obtained from a StateStore — typically via
// store.NewExecutionState(...) for a fresh run or store.LoadExecutionState(...)
// for a resume. The fresh-start vs. resume distinction lives in the factory,
// not here; Evaluate only walks the graph.
type EvaluateOpts struct {
    Context *ExecutionContext
    History *ExecutionHistory
}

// Render the process as a markdown string
func (p *Process) ToMarkdown() string


type EvaluationResult struct {
    Context  ExecutionContext  // updated context after evaluation
    History  ExecutionHistory  // full post-evaluation execution history
    Status   ProcessStatus     // COMPLETED, SUSPENDED, CANCELLED, or FAILED
    Steps    []ExecutionStep   // steps recorded during this evaluation
}


type ProcessStatus int

const (
    ProcessStatusPending   ProcessStatus = iota // submitted but not yet started
    ProcessStatusRunning                        // at least one task is in progress or pending
    ProcessStatusSuspended                      // paused waiting for an external event (Suspend* event node — see event-nodes.spec.md); resumable via re-evaluation
    ProcessStatusCompleted                      // reached an EndEvent or TerminateEvent
    ProcessStatusCancelled                      // reached a CancelEvent — clean abort, distinct from Completed and Failed; not retried
    ProcessStatusFailed                         // an unrecoverable error occurred (task failure or ErrorEvent)
)


type RetryConfig struct {
    MaxRetries         *int           // max retry attempts after initial failure (nil = no count limit)
    RetryFor           *time.Duration // keep retrying for this duration (nil = no time limit)
    RetryDelay         time.Duration  // min delay before retry (first retry if exponential_backoff is true) (default: 30s)
    ExponentialBackoff bool           // double the delay after each retry (default: false)
}

func NewRetryConfig(opts RetryOpts) *RetryConfig

type RetryOpts struct {
    MaxRetries         *int
    RetryFor           *time.Duration
    RetryDelay         time.Duration
    ExponentialBackoff bool
}
```

---

## Graph Construction

The `graph` parameter to `NewProcess()` receives a list of node chain expressions. Each chain is built from event nodes, gateway nodes, and task nodes connected via `.To()`:

- **Event nodes** — see [event-nodes.spec.md](event-nodes.spec.md) for `Start`, `End`, `Cancel`, `Error`, `Terminate`, and the `Suspend*` / `Pause*` variants.
- **Gateway nodes** — see [gateway-nodes.spec.md](gateway-nodes.spec.md) for `And`, `Xor`, `Or`, `Join`, and `GatewayConditions`.
- **Task nodes** — see [task-nodes.spec.md](task-nodes.spec.md).

`NewProcess()` walks the chains, builds the graph, validates structure (connectivity, at least one reachable terminating event from each `StartEvent`, cycle exits, duplicate ids), and stores the result as a `ProcessGraph` on `Process.Graph`.

### ProcessGraph and SequenceFlow

`ProcessGraph` is the processed, structured representation of a process graph. It is built by `NewProcess()` from the raw `Graph` list and stored as `Process.Graph`.

```go
type ProcessGraph struct {
    Nodes      map[string]ProcessNode // id -> node (all nodes including gateways)
    StartNodes []*StartEvent          // discovered start events
    EndNodes   []*EndEvent            // discovered normal-completion end events
    Edges      []*SequenceFlow        // all directed edges
}

type SequenceFlow struct {
    Source    ProcessNode
    Target    ProcessNode
    Condition BlExpr // nil on unconditional edges
}
```

`NewProcess()` takes the raw `[]ProcessNode`, walks all edges from the nodes in the list to discover every reachable node and edge, categorizes nodes by type, and validates the graph structure (connectivity, terminating-event reachability, cycle exits, duplicate ids). The result is stored on `Process.Graph`.

`StartNodes` and `EndNodes` are typed conveniences for the most common boundary lookups. The full set of terminating event nodes (including `CancelEvent`, `ErrorEvent`, `TerminateEvent`) is reachable via `Nodes`.

### `.To()` Chaining

Every `ProcessNode` exposes a `.To()` method. Calling `node.To(target)` records a sequence flow (directed edge) from `node` to `target` and returns `target`, enabling chaining inside the `Graph` list.

```go
func (n ProcessNode) To(target ProcessNode) ProcessNode
```

Nodes and edges are free-standing during graph construction — they are not associated with any process until `NewProcess()` processes the `Graph` list. A node can only belong to one process; if a node already associated with one process appears in another process's `Graph` list, `NewProcess()` produces a `ProcessDefinitionError`.

---

## Registry

`NewProcess()` registers the returned `*Process` in a package-level `blkit` registry keyed by `(Namespace, Id, Version)`. The registry is the single source of truth used by the worker to resolve fetched `Job`s to a `*Process` — see [../worker/worker.spec.md](../worker/worker.spec.md#the-process-registry). Importing a package that defines processes is therefore sufficient to make those processes runnable in any worker binary that links the package; no explicit registration call is required.

### Namespace Derivation

`Namespace` is set automatically by `NewProcess()` and cannot be supplied by the caller — there is no `Namespace` field in `ProcessOpts`. The value is the Go package import path of the file that called `NewProcess()`.

The derivation uses `runtime.Caller(1)` to obtain the program counter of the caller, then `runtime.FuncForPC(pc).Name()` to retrieve the fully-qualified function name (formatted as `<package-import-path>.<function-name>`), then strips the trailing function/method segment to recover the package path.

Examples:

| `NewProcess()` call site | Derived `Namespace` |
|---|---|
| `var X = NewProcess(...)` in `example.com/area/lendingflows/v1/loan.go` | `"example.com/area/lendingflows/v1"` |
| Inside an `init()` in `example.com/area/lendingflows/v1` | `"example.com/area/lendingflows/v1"` |
| Inside `main.main()` of a `package main` binary | `"main"` |
| Inside a `_test.go` file in package `lendingflows_test` | `"example.com/area/lendingflows/v1_test"` |

### Registry Helpers

```go
// Resolve a process from the registry. Returns false if no such (Namespace, Id, Version) is registered.
func LookupProcess(namespace, id, version string) (*Process, bool)

// Return a snapshot of every registered process. The returned slice is a fresh
// copy — callers may sort, filter, or retain it without affecting the registry,
// and re-registrations after the call are not reflected. Order is unspecified.
func AllProcesses() []*Process

// Clear the registry. Intended for test isolation only.
func ResetRegistry()
```

### Wire-Protocol Implication

Because `Namespace` is part of the broker routing key produced by `MessageGateway.Submit` (see [../messagegateway/overview.spec.md](../messagegateway/overview.spec.md)), the namespace value is part of the operator-visible API contract. Renaming a Go module path, or moving a process file between packages, changes the namespace and is therefore a breaking change for any client enqueuing requests against the old namespace. Authors should treat module paths that contain processes as stable interfaces.

### Edge Cases

- `NewProcess()` called twice with the same `(Namespace, Id, Version)` panics. Within a single Go package this means two `NewProcess` calls with the same `Id` and `Version` collide as before; across packages, the differing namespace prevents collision automatically.
- A process defined in package `main` produces `Namespace = "main"`. Multiple `package main` binaries that each define a process with the same `Id` and `Version` will not collide at runtime (different binaries), but two `NewProcess` calls with the same `Id` and `Version` inside one `package main` will.
- Tests in a `_test`-suffixed package (e.g. `lendingflows_test`) produce a namespace ending in `_test`. Tests in the same package as the code under test share the package's namespace.
- Tests that intentionally register the same `(Namespace, Id, Version)` more than once must call `blkit.ResetRegistry()` between calls.
- Vendored or forked deployments where the module import path changes will produce a different namespace for the same process source. This is intentional — the namespace tracks where the code lives, not what it does.
- Inlining: the derivation depends on the call frame for `NewProcess()` being present on the stack. `NewProcess` is large enough that the Go compiler will not inline it; if the implementation is later refactored such that inlining becomes possible, a `//go:noinline` directive should be added to preserve the derivation.

---

## Example

```go
// Tasks (declared package-scope alongside their function bodies; see
// ../processes/native-function-task.spec.md). Each task is
// generic over a typed Outputs struct; for these wiring-focused examples
// we share a single one-field StepOutputs across every task in the snippet.
type StepOutputs struct{ Status BlString }

var validateApplication = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "validate", Name: "Validate Application",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var pullCreditReport = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "credit-report", Name: "Pull Credit Report",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var checkIncome = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "check-income", Name: "Check Income",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var issueOfferLetter = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "offer", Name: "Issue Offer Letter",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var proposeCounter = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "counter", Name: "Propose Counter",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var declineApplication = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "decline", Name: "Decline Application",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var notifyApplicant = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "notify", Name: "Notify Applicant",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var updateCreditBureau = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "update-bureau", Name: "Update Credit Bureau",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var archiveApplication = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "archive", Name: "Archive Application",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})

// Sub-process tasks
verifyIdentity := NewSubProcessTask("verify-id", "Verify Identity",
    "kyc-verification", "start")
fraudRiskCheck := NewSubProcessTask("fraud-check", "Fraud Risk Check",
    "fraud-check-process", "start")

// Decision tasks — clone the decision-logic templates with this process's id/name/mappings.
// The templates (affordabilityTemplate, scoringTemplate) are *DecisionTask values built
// in the constructor-function idiom — see ../decision-tasks/decision-task.spec.md.
runAffordability := affordabilityTemplate().Clone(DecisionTaskOpts{
    Id:   "affordability",
    Name: "Run Affordability",
    InputMappings: NewVariableMapping(
        [2]string{"start.applicant",   "applicant"},
        [2]string{"start.loan_amount", "loan_amount"},
    ),
    OutputMappings: NewVariableMapping(
        [2]string{"affordable", "affordability.affordable"},
    ),
})
calculateScore := scoringTemplate().Clone(DecisionTaskOpts{
    Id:   "calc-score",
    Name: "Calculate Score",
    InputMappings: NewVariableMapping(
        [2]string{"credit-report.score", "credit_score"},
        [2]string{"check-income.income", "income"},
    ),
    OutputMappings: NewVariableMapping(
        [2]string{"score", "calc-score.score"},
    ),
})

// Gateway conditions
riskConditions := NewGatewayConditions(
    NewBranch("offer", Bl.StringVar("fraud-check.risk").Equals(Bl.String("low"))),
    NewBranch("counter", Bl.StringVar("fraud-check.risk").Equals(Bl.String("medium"))),
    DefaultBranch("decline"),
)

// Boundary events
start := Start("start", "Start", NewInputContract(
    RequiredField("applicant", BlContext),
    RequiredField("loan_amount", BlNumber),
))
done := End("done", "Done")

// Process
loanApplication := NewProcess("loan-application", "1.0", ProcessOpts{
    Name: "Loan Application Process",
    Graph: []ProcessNode{
        start.To(validateApplication),

        validateApplication.To(verifyIdentity),

        verifyIdentity.To(And(pullCreditReport,
                              checkIncome,
                              runAffordability)),

        Join(pullCreditReport,
            checkIncome,
            runAffordability).To(calculateScore),

        calculateScore.To(fraudRiskCheck),

        fraudRiskCheck.To(Xor(riskConditions, map[string]ProcessNode{
            "offer":   issueOfferLetter,
            "counter": proposeCounter,
            "decline": declineApplication,
        })),

        Join(issueOfferLetter,
            proposeCounter,
            declineApplication).To(notifyApplicant),

        notifyApplicant.To(updateCreditBureau),

        updateCreditBureau.To(archiveApplication),

        archiveApplication.To(done),
    },
})


// Run the process
store := blkit.NewInMemoryStateStore()

ctx, hist, err := store.NewExecutionState(loanApplication, NewExecutionStateOpts{
    StartId: "start",
    Input:   map[string]any{"applicant": applicantData},
})
if err != nil { /* ... */ }

result, err := loanApplication.Evaluate(EvaluateOpts{Context: ctx, History: hist})
if err != nil { /* ... */ }

// Caller decides how to handle a Suspended result — persist result.History and
// re-run Evaluate later when the awaited event arrives, or wire a MessageGateway
// for managed continuation. Direct callers get no library-provided continuation.
```

---

## Multiple Start and End Nodes

A process can define multiple entrypoints and exit points. Each start and terminating event has a unique `id`. The `StartId` passed to `store.NewExecutionState(...)` selects which entrypoint the resulting Context/History will begin from. Execution terminates when a token reaches any `EndEvent`, `CancelEvent`, `ErrorEvent`, or `TerminateEvent` (see [event-nodes.spec.md](event-nodes.spec.md)).

```go
// Tasks
type StepOutputs struct{ Status BlString }

var review = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "review", Name: "Review Application",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var generateOffer = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "generate-offer", Name: "Generate Offer",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var sendRejection = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "send-rejection", Name: "Send Rejection",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})
var notify = NewNativeFunctionTask(NativeFunctionTaskOpts[StepOutputs]{
    Id: "notify", Name: "Notify Applicant",
    Fn: func(ctx *ExecutionContext) (StepOutputs, error) { /* body */ },
})

// Decision task — clone of the risk-assessment template
assessRisk := riskAssessmentTemplate().Clone(DecisionTaskOpts{
    Id:   "assess-risk",
    Name: "Assess Risk",
    InputMappings: NewVariableMapping(
        [2]string{"start.applicant",   "applicant"},
        [2]string{"start.risk_level",  "risk_level"},
    ),
    OutputMappings: NewVariableMapping(
        [2]string{"risk_level", "assess-risk.risk_level"},
    ),
})

// Gateway conditions
decisionConditions := NewGatewayConditions(
    NewBranch("approve", Bl.StringVar("assess-risk.risk_level").Equals(Bl.String("low"))),
    DefaultBranch("reject"),
)

// Boundary events — each entrypoint declares its own input shape
newApp := Start("new", "New Application", NewInputContract(
    RequiredField("applicant", BlContext),
    RequiredField("loan_amount", BlNumber),
))
reassess := Start("reassess", "Re-assessment", NewInputContract(
    RequiredField("applicant", BlContext),
    RequiredField("risk_level", BlString),
))
approved := End("approved", "Loan Approved")
rejected := End("rejected", "Loan Rejected")

// Process with two start nodes and two end nodes
loanDecision := NewProcess("loan-decision", "1.0", ProcessOpts{
    Name: "Loan Decision Process",
    Graph: []ProcessNode{
        // New application flow: full review pipeline
        newApp.To(review),
        review.To(assessRisk),

        // Re-assessment flow: skip review, go straight to risk assessment
        reassess.To(assessRisk),

        // After risk assessment, branch on outcome
        assessRisk.To(Xor(decisionConditions, map[string]ProcessNode{
            "approve": generateOffer,
            "reject":  sendRejection,
        })),

        // Approved path
        generateOffer.To(notify),
        notify.To(approved),

        // Rejected path
        sendRejection.To(rejected),
    },
})


store := blkit.NewInMemoryStateStore()

// Run as a new application
ctxNew, histNew, _ := store.NewExecutionState(loanDecision, NewExecutionStateOpts{
    StartId: "new",
    Input:   map[string]any{"applicant": applicantData},
})
result, err := loanDecision.Evaluate(EvaluateOpts{Context: ctxNew, History: histNew})

// Or run as a re-assessment
ctxRe, histRe, _ := store.NewExecutionState(loanDecision, NewExecutionStateOpts{
    StartId: "reassess",
    Input:   map[string]any{"applicant": applicantData, "risk_level": Bl.String("low")},
})
result, err = loanDecision.Evaluate(EvaluateOpts{Context: ctxRe, History: histRe})
```

---

## Evaluation

The `Process` class provides `Evaluate()` — the graph-walking algorithm. A registered Process instance is reusable; `Evaluate()` is called with different execution state each time and does not mutate the process object.

### Input

`Evaluate()` always takes a `Context` + `History` pair via `EvaluateOpts`. Both must be obtained from a `StateStore`:

- **Fresh start** — `store.NewExecutionState(process, NewExecutionStateOpts{StartId, Input})` generates a `ProcessInstanceId`, builds an empty `ExecutionContext`, populates an `ExecutionHistory` from the Process attributes (`Id`, `Version`), and records the input variables as the initial transaction against the start node.
- **Resume** — `store.LoadExecutionState(processInstanceID)` reconstructs both objects from the durable event log.

In either case, the returned Context and History are wired to the store's writer channel, so subsequent mutations during `Evaluate` stream through the writer pool. Passing un-wired objects (e.g. ones obtained via `store.Get` / `store.LatestContext`) to `Evaluate` is a programmer error — those objects exist for read-only inspection and silently swallow writes.

```go
result, err := process.Evaluate(EvaluateOpts{Context: ctx, History: hist})
```

### Execution

`Evaluate()` runs a single scheduler goroutine that ticks the process forward. Each tick:

1. **Determine ready nodes.** Token positions are derived from `ExecutionHistory` via the rules in [Token Position Reconstruction](#token-position-reconstruction) below. On initial evaluation (empty history), a token is placed at the start node and a `PROCESS_STARTED` step is recorded.
2. **Process non-task nodes synchronously** on the scheduler goroutine:
   - **StartEvent** — validates input against the `InputContract`, records `NODE_COMPLETED`, moves the token to successors.
   - **Gateway** — evaluates conditions against the `ExecutionContext`, records a `GATEWAY_RESOLVED` step, moves tokens to selected successors.
   - **Terminating events** — consume the token. `EndEvent` validates `OutputContract` (if set) and records `PROCESS_COMPLETED` once all tokens are consumed. `CancelEvent`, `ErrorEvent`, and `TerminateEvent` cancel in-flight task goroutines (see [Cancellation](#cancellation) below) and record `PROCESS_CANCELLED` / `PROCESS_FAILED` / `PROCESS_COMPLETED` respectively. See [event-nodes.spec.md](event-nodes.spec.md).
   - **Suspend events** — `SuspendForDuration`, `SuspendUntilDatetime` initiate the suspension drain (see [Suspension](#suspension) below). **Pause events** (`PauseForDuration`) dispatch a goroutine that sleeps for the configured duration; the scheduler loop continues to tick and other ready tasks continue to advance.
3. **Dispatch ready task nodes as goroutines.** For each ready task node (`NativeFunctionTask`, `DecisionTask`, `SubProcessTask`, `TriggerProcessTask`, `RequestInputTask`):
   - The scheduler records `NODE_SCHEDULED` and `NODE_STARTED`.
   - The scheduler invokes `task.Evaluate(ctx, executionID)` in a **new goroutine** with a `context.Context` derived from the scheduler's parent context (see [Cancellation](#cancellation)).
   - For `NativeFunctionTask[Outputs]` (see [native-function-task.spec.md § Evaluate](native-function-task.spec.md#evaluate)), `Evaluate` invokes the function body and reflects the returned `Outputs` struct into a `map[string]any` recorded as a Pending transaction under `(t.Id, executionID)`. The function body may also call `ctx.Record(...)` directly for intermediate side outputs; those land as additional Pending transactions sharing the same `executionID`. Pending transactions are visible only to the executing node via `ctx.AsExecutor(nodeID)` until commit. See [../data/execution-context.spec.md § Atomic Commit and Visibility](../data/execution-context.spec.md#atomic-commit-and-visibility).
   - **SubProcessTask** is dispatched the same way; its goroutine evaluates a child process recursively under a scoped `ExecutionContext` and a child `ExecutionHistory` with a separate `ProcessInstanceId`.
4. **Check spawned task goroutines for completion.** For each task goroutine that has finished since the previous tick:
   - On success → the scheduler calls `ctx.Commit(nodeID)`, records `NODE_COMPLETED`, and successors become candidates for the next tick.
   - On error → the scheduler calls `ctx.Abort(nodeID)`, records `NODE_FAILED`, and error boundary handling takes over (see [Error Handling](#error-handling)).
5. The loop ticks until all tokens are consumed (`COMPLETED`), a `CancelEvent` is reached (`CANCELLED`), the process suspends waiting for an external event (`SUSPENDED` — see [Suspension](#suspension)), or an unrecoverable error occurs (`FAILED`).

Concurrency emerges from the scheduler dispatching every ready task as its own goroutine within the same tick — there is no per-branch walker and no explicit parallelism primitive. The graph topology decides what becomes ready; the scheduler dispatches everything ready immediately. There is no concurrency limit or backpressure.

The returned `EvaluationResult` contains:
- `Context` — the final `ExecutionContext` after all tasks have executed.
- `History` — the complete `ExecutionHistory` with all steps from the entire run.
- `Status` — `COMPLETED`, `SUSPENDED`, `CANCELLED`, or `FAILED`.
- `Steps` — all steps recorded during this evaluation.

```go
// Run a process to completion
ctx, hist, _ := store.NewExecutionState(loanApplication, NewExecutionStateOpts{
    StartId: "start",
    Input:   map[string]any{"applicant": applicantData},
})

result, err := loanApplication.Evaluate(EvaluateOpts{Context: ctx, History: hist})

fmt.Println(result.Status)               // COMPLETED
fmt.Println(result.Context)              // final variables
fmt.Println(result.History.ToMarkdown()) // full execution history
```

#### Error Handling

If a task fails and there is no error boundary event, the process fails. The scheduler cancels all in-flight task goroutines via the mechanism described in [Cancellation](#cancellation) below; their pending transactions are aborted via `ctx.Abort(nodeID)`. The history records `NODE_FAILED` for the failed task and `PROCESS_FAILED` for the process. The `EvaluationResult` has `Status: FAILED` and the `error` field on the `NODE_FAILED` step contains the error details.

#### Cancellation

Every task goroutine the scheduler dispatches receives a `context.Context` derived from the scheduler's parent context. The scheduler cancels that context — and therefore signals every in-flight task to stop — in any of the following situations:

- An `ErrorEvent`, `CancelEvent`, or `TerminateEvent` is reached.
- A task fails and no error boundary event catches it (Error Handling above).
- `MaxRunTime` or `MaxCompletionTime` expires (see [Timeouts](#timeouts) and [Process Options](#process-options)).

Task implementations are expected to honour `context.Context` cancellation. `NativeFunctionTask.Fn` and any external I/O performed by a task body should be wrapped in `ctx.Done()`-aware patterns; long-running CPU work should periodically check `ctx.Err()`. When a cancelled task goroutine returns, the scheduler calls `ctx.Abort(nodeID)` to discard its Pending transactions and records `NODE_FAILED` (or `NODE_CANCELLED` if cancellation was the explicit cause) for that node.

#### Suspension

A process can suspend mid-evaluation when it reaches a `SuspendForDuration` / `SuspendUntilDatetime` event node (see [event-nodes.spec.md](event-nodes.spec.md)) or a `RequestInputTask` configured for durable wait (see [task-nodes.spec.md](task-nodes.spec.md#requestinputtask)). When this happens, the scheduler enters a **suspension drain**:

1. The scheduler stops dispatching new ready tasks.
2. The scheduler waits for every already-spawned task goroutine to finish, committing or aborting each one through the normal completion path.
3. `Evaluate()` then returns with `Status: SUSPENDED`.

The token rests at the suspending node and is preserved in `result.History` so that a later `Evaluate()` call (passing the persisted `Context` and `History`) can resume from the same position.

**`RequestInputTask` exception** — when a `RequestInputTask` triggers the suspension, the outbound input-request message is sent **immediately** as part of the task's own dispatch, before the surrounding drain begins. The external responder receives the request promptly even if other parallel tasks are still draining. The task itself remains the suspending node; the drain waits on its sibling tasks, not on the response.

`SUSPENDED` is a non-terminal status. The caller is responsible for arranging resumption — typically by persisting `result.History` and signalling the broker via `MessageGateway.ReenqueueSuspended(...)` so the eventual `JobResume` is delivered to some worker when the wait condition is satisfied. For `RequestInputTask`, the wait is satisfied by a `MessageGateway.RespondToInputRequest(processInstanceID, requestID, payload)` call.

`Pause*` event nodes do **not** transition the process to `SUSPENDED` — the pause node's goroutine sleeps for the configured duration while the scheduler loop continues to tick and dispatch other ready tasks. The status remains `RUNNING` throughout the pause.

When evaluation is invoked with no `Suspend*` nodes in the graph, `SUSPENDED` cannot be reached — the process always runs to `COMPLETED`, `CANCELLED`, or `FAILED`.

### How Evaluate() Builds the History

`Evaluate()` does not modify the `ExecutionHistory` object passed to it. Instead, it creates a deep copy and applies the new steps to the copy via `Record()`. The copy — with all new steps integrated — is returned as `result.History`. The original history object remains unchanged, which matters when multiple concurrent evaluations of different process instances share the same worker and may overlap in time.

When the input History is freshly built via `store.NewExecutionState(...)`, it begins empty (apart from the initial-transaction metadata) and `Evaluate()` applies all steps to its working copy as the process progresses.

For a detailed description of `Record()` and the `ExecutionHistory` structure, see [execution-history.spec.md](../data/execution-history.spec.md).

### Token Position Reconstruction

When resuming from existing execution state, `Evaluate()` derives the current token positions from the execution history rather than storing them explicitly:

- A node is **ready** if an incoming predecessor completed more recently than the node's own last execution (or the node has never executed).
- **Parallel joins**: ALL incoming branches must have completed more recently than the join's last execution.
- **Implicit merges** (task with multiple incoming edges, no explicit gateway): ANY incoming edge with a fresh token triggers the node.
- **Loopbacks**: handled naturally — a gateway resolving to a backward path creates a fresh token on that edge, because the gateway resolution is more recent than the target node's last execution.

### Idempotent Resumption

`Evaluate()` is idempotent with respect to its input state: calling it again with the same `Context` and `History` for an already-completed or already-suspended process produces no new work. On resumption, `Evaluate()` reads the history, identifies what is genuinely ready (e.g. a parallel join only fires when all branches have completed), and advances only from there. This matters when a suspended process is re-enqueued and may be picked up more than once before its awaited event arrives.

---

## Retry

If `Retry` is set on a Process, the runtime automatically retries the process when it fails (`PROCESS_FAILED`). Each retry is a fresh execution of the same process with the same input — a new `ProcessInstanceId`. Retry orchestration is the responsibility of the runtime (the long-running worker), not `Evaluate` itself.

- **`MaxRetries`** — the maximum number of retry attempts after the initial failure. A value of `3` means up to 4 total executions (1 initial + 3 retries). `nil` means no count limit — retries continue until `RetryFor` expires.
- **`RetryFor`** — the total duration during which retries are permitted. The timer starts when the process was first submitted, not when it first fails. `nil` means no time limit — retries continue until `MaxRetries` is exhausted.
- **`RetryDelay`** — the min delay before retrying (first retry delay if exponential_backoff is true). Default `30s`.
- **`ExponentialBackoff`** — when `true`, the delay doubles after each retry (e.g. 30s, 60s, 120s, 240s).

At least one of `MaxRetries` or `RetryFor` must be set. A `RetryConfig` with both as `nil` produces a `ProcessDefinitionError` (no limit would mean infinite retries).

When both are set, whichever limit is reached first stops retrying. For example, `MaxRetries=5, RetryFor=Duration("1h")` means: retry up to 5 times, but stop if 1 hour has passed since the first failure, whichever comes first.

### Examples

```go
// Retry up to 3 times with default 30s delay
payment := NewProcess("payment", "1.0", ProcessOpts{
    Retry: NewRetryConfig(RetryOpts{MaxRetries: 3}),
    Graph: []ProcessNode{...},
})

// Retry for up to 48 hours with exponential backoff
dataIngestion := NewProcess("data-ingestion", "1.0", ProcessOpts{
    Retry: NewRetryConfig(RetryOpts{
        RetryFor:           48 * time.Hour,
        RetryDelay:         1 * time.Minute,
        ExponentialBackoff: true,
    }),
    Graph: []ProcessNode{...},
})

// Retry up to 10 times or for 6 hours, whichever comes first
reportGen := NewProcess("report-gen", "1.0", ProcessOpts{
    Retry: NewRetryConfig(RetryOpts{
        MaxRetries:         10,
        RetryFor:           6 * time.Hour,
        RetryDelay:         5 * time.Minute,
        ExponentialBackoff: true,
    }),
    Graph: []ProcessNode{...},
})
```

---

## Markdown Rendering

`ToMarkdown()` returns a complete markdown document representing the process graph. It traverses the graph from the start node and renders each element, including the full markdown representation of every constituent task.

### Format

- **Title** — the process's name (or id) as a level-1 heading
- **Description** — the process's description, if set
- **Flow** — the sequence of nodes rendered in execution order, showing the graph structure:
  - Sequential flows shown as node names connected by arrows (`→`)
  - Parallel gateways (AND) shown with all branches listed
  - Exclusive gateways (XOR) shown with branch conditions
  - Inclusive gateways (OR) shown with branch conditions
  - Join points indicated where branches converge
- **Tasks** — each task rendered via its own `ToMarkdown()` under a level-2 heading, including its id, name, type, and description. `DecisionTask` sections include the decision-logic markdown inline. `SubProcessTask` sections include a link to the referenced process's markdown document.

### Example

```go
fmt.Println(loanApplication.ToMarkdown())
```

Output:

```text
# Loan Application Process

End-to-end loan application pipeline: validates the applicant, runs credit and income checks in parallel, scores the application, and routes to approval, counter-offer, or decline.

## Flow

1. **Start**
2. **Validate Application** (validate)
3. **Verify Identity** (verify-id)
4. **AND Split** →
   - Pull Credit Report (credit-report)
   - Check Income (check-income)
   - Run Affordability (affordability)
5. **AND Join** → **Calculate Score** (calc-score)
6. **Fraud Risk Check** (fraud-check)
7. **XOR Split** (risk) →
   - offer: risk = "low" → **Issue Offer Letter**
   - counter: risk = "medium" → **Propose Counter**
   - decline (default) → **Decline Application**
8. **XOR Join** → **Notify Applicant** (notify)
9. **Update Credit Bureau** (update-bureau)
10. **Archive Application** (archive)
11. **End**

## Tasks

### Validate Application (validate)
NativeFunctionTask
Checks applicant data for completeness and basic eligibility.

### Verify Identity (verify-id)
SubProcessTask — [kyc-verification](./kyc-verification.md) (start: start)
Runs KYC checks against the applicant's identity documents.

### Pull Credit Report (credit-report)
NativeFunctionTask

### Check Income (check-income)
NativeFunctionTask

### Run Affordability (affordability)
DecisionTask — [Affordability Model](./affordability-model.md)
Evaluates applicant affordability using a DMN decision model.

### Calculate Score (calc-score)
DecisionTask — [Scoring Model](./scoring-model.md)
Combines credit, income, and affordability into a single score.

### Fraud Risk Check (fraud-check)
SubProcessTask — [fraud-check-process](./fraud-check-process.md) (start: start)
Runs the fraud detection sub-process.

### Issue Offer Letter (offer)
NativeFunctionTask

### Propose Counter (counter)
NativeFunctionTask

### Decline Application (decline)
NativeFunctionTask

### Notify Applicant (notify)
NativeFunctionTask

### Update Credit Bureau (update-bureau)
NativeFunctionTask

### Archive Application (archive)
NativeFunctionTask
```

---

## Edge Cases

- `NewProcess()` with missing or empty `id` or `version` produces a `ProcessDefinitionError`.
- `NewProcess()` walks the nodes in the `Graph` list to discover the full graph structure, storing it as a `ProcessGraph` on the `Graph` attribute. Start and terminating nodes are identified by type (`StartEvent`, `EndEvent`, `CancelEvent`, `ErrorEvent`, `TerminateEvent`). If no `StartEvent` is reachable, `NewProcess()` raises `ProcessDefinitionError`. If a `StartEvent` has no path to any terminating event, `NewProcess()` raises `ProcessDefinitionError`.
- The `Graph` list must contain at least one node chain. An empty `Graph` produces a `ProcessDefinitionError`.
- A process with no `StartEvent` cannot be run; `Evaluate()` returns a `ProcessDefinitionError`.
- A process may have multiple start nodes. Each is identified by its `id` (set via the first argument to `Start(id, name, contract)`). The `StartId` passed to `store.NewExecutionState(...)` determines which entrypoint is used.
- A process may have multiple end nodes. Each is identified by its `id` (set via the first argument to `End(id, name, ...)`). Execution terminates when any `EndEvent` is reached.
- Duplicate start node ids on the same process produce a `ProcessDefinitionError`.
- Duplicate end node ids on the same process produce a `ProcessDefinitionError`.
- A process graph may contain cycles (loopbacks to earlier nodes). A cycle is valid as long as at least one conditional branch leads to a reachable terminating event. A cycle with no conditional exit is detected and returns a `ProcessDefinitionError` before execution begins.
- If `MaxRunTime` is set and the elapsed time since execution began exceeds the limit, the engine produces a `ProcessTimeoutError` and the process fails. The timer starts when the engine begins executing the process (status transitions to `RUNNING`), not when the request is enqueued. In-progress tasks may be cancelled (implementation-defined).
- If `MaxCompletionTime` is set and the elapsed time since the process was first queued exceeds the limit, the worker produces a `ProcessCompletionTimeoutError` and the process fails. This includes any time spent queued, running, waiting for tasks, and retrying. In-progress tasks may be cancelled (implementation-defined).
- `MaxCompletionTime` and `MaxRunTime` can both be set. `MaxRunTime` limits a single execution attempt; `MaxCompletionTime` limits the entire lifecycle from submission to completion, including retries. If `MaxCompletionTime` is reached during a retry, no further retries are attempted.
- A `RetryConfig` with both `MaxRetries` and `RetryFor` set to `nil` produces a `ProcessDefinitionError`.
- A `RetryConfig` with `MaxRetries=0` is valid but performs no retries (equivalent to no retry config).
- If `MaxRunTime` is set alongside `Retry`, each retry attempt gets a fresh `MaxRunTime` timer.
- With `ExponentialBackoff`, if the next computed delay would exceed the remaining `RetryFor` window, the retry is not attempted.
- A process that fails due to `DataContractValidationError` from a `StartEvent` contract at submission time is not retried — the input is invalid and retrying would produce the same error. Only `PROCESS_FAILED` failures trigger retries. See [data-contract.spec.md](../data/data-contract.spec.md) for boundary-validation semantics.
- See [gateway-nodes.spec.md](gateway-nodes.spec.md) for edge cases related to gateway constructors and `Join`. See [event-nodes.spec.md](event-nodes.spec.md) for edge cases on event nodes.
- A node already associated with one process cannot appear in another process's `Graph` list — `NewProcess()` produces a `ProcessDefinitionError`.
- `version` is mandatory and must be a non-empty string.
- Two processes with the same `id` but different `version` values are distinct and can be registered simultaneously.
- `Evaluate()` requires both `Context` and `History`. Providing one without the other (or neither) raises `ValueError`. Both must come from a `StateStore` factory — `NewExecutionState(...)` for a fresh run or `LoadExecutionState(...)` for a resume.
- `Evaluate()` with a freshly-built History (no prior steps) is an initial evaluation. The `ProcessInstanceId` was generated by the factory; `Input` is recorded as the initial transaction by the factory and is already present on the Context.
- `Evaluate()` does not mutate the input `ExecutionHistory`. A deep copy is returned in the result.
- `Evaluate()` runs a single scheduler loop that dispatches ready tasks as goroutines and awaits their completion. It returns when the process completes, suspends waiting for an external event, or fails.
- Ready tasks are each dispatched as their own goroutine. Concurrency emerges from the scheduler dispatching multiple ready tasks per tick — there is no concurrency limit or backpressure.
- If a task fails and there is no error boundary event, the process fails. The scheduler cancels every in-flight task goroutine via its `context.Context` (see [Cancellation](#cancellation)); their pending transactions are aborted via `ctx.Abort(nodeID)`.
- `NativeFunctionTask` tasks invoke the registered `Fn` directly.
- `SubProcessTask` tasks are evaluated recursively, creating a child `ExecutionHistory` under a separate `ProcessInstanceId`.
- `MaxRunTime` is enforced by `Evaluate()` — if the elapsed time exceeds the limit, pending tasks are cancelled and the process fails with `ProcessTimeoutError`. `MaxCompletionTime` is also enforced (measured from the start of the `Evaluate()` call). `Retry` is not enforced by `Evaluate()` — if set, it is ignored (retries are a runtime-level concern).
- Resuming a completed or suspended process by re-calling `Evaluate()` with the same `Context` and `History` produces no new work — `Evaluate()` advances only from positions that the history shows are genuinely ready.
- `Evaluate()` does not mutate the process object. All mutable state is returned in the `EvaluationResult`. Process instances are safe to reuse across concurrent evaluations of different process instances.
- `ToMarkdown()` does not require execution state — it renders the graph structure from the process definition.
- See [../worker/worker.spec.md](../worker/worker.spec.md) for long-running execution.
