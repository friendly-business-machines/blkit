---
name: NativeFunctionTask
description: A Go-function-backed task node, generic over a caller-supplied outputs struct; declared together with its function body and reused across processes via Clone
targets:
  - ../processes/native-function-task.go
---

# NativeFunctionTask

Invokes a Go function directly. The function and its task-level framing are declared together via `NewNativeFunctionTask`; the resulting `*NativeFunctionTask[Outputs]` is itself a `ProcessNode` that can be placed in a process graph. Reuse across multiple processes is via `Clone(opts)` — function-logic fields are shared by reference with clones; task-level fields are reset (not inherited) on `Clone`.

`NativeFunctionTask` is **generic over a caller-supplied outputs struct** that declares the task's typed outputs. This is the same pattern [`DecisionNode`](../decision-tasks/decision-node.spec.md#outputs-structs) and [`BusinessKnowledgeModel`](../decision-tasks/business-knowledge-model.spec.md) already use. The framework reflects on the `Outputs` struct at construction to derive the output column names and validate field types. At execution time, `Evaluate` reflects on the returned value to build the `map[string]any` passed to `ctx.Record`.

The shape mirrors [`DecisionTask`](../decision-tasks/decision-task.spec.md): a single type holding both the logic and the task-level metadata, reused via `Clone`, not via wrapping or a separate factory.

```go
type NativeFunctionTask[Outputs any] struct {
    // Function-logic fields — set in NativeFunctionTaskOpts at creation;
    // treated as immutable by convention thereafter. Shared by reference
    // with clones.
    Fn          func(*ExecutionContext) (Outputs, error)
    Description *string

    // Task-level fields — set in NativeFunctionTaskOpts at creation;
    // treated as immutable by convention. Reset (not inherited) on Clone.
    Id                              string
    Name                            string
    InputMappings                   *VariableMapping
    OutputMappings                  *VariableMapping
    MaxRetries                      int
    InitialRetryDelay               *time.Duration
    ExponentialBackoff              bool
    Timeout                         *time.Duration
    TimeoutBehavior                 string // "error" (default), "abort", "wait"
    Loop                            *LoopConfig
    MultiInstance                   *MultiInstanceConfig
    MaxExecutionsPerProcessInstance int
    ExitPorts                       map[string]ExitPort // keyed by exit-port id
}

// Construct a NativeFunctionTask. opts.Fn is required and must be non-nil;
// opts.Id and opts.Name are mandatory at the time of incorporation into a
// process graph. NewNativeFunctionTask reflects on the Outputs type
// parameter and validates the field rules described in "Outputs struct"
// below — invalid Outputs produce a ProcessDefinitionError at construction.
// opts.ExitPorts is converted to the keyed map internally. Any other field
// in opts is set verbatim; unspecified fields default to their zero values.
func NewNativeFunctionTask[Outputs any](opts NativeFunctionTaskOpts[Outputs]) *NativeFunctionTask[Outputs]

type NativeFunctionTaskOpts[Outputs any] struct {
    // Function-logic fields
    Fn          func(*ExecutionContext) (Outputs, error)
    Description *string

    // Task-level fields — all optional at creation; Id, Name, InputMappings, and
    // OutputMappings are mandatory at the time of incorporation into a process graph.
    Id                              string
    Name                            string
    InputMappings                   *VariableMapping
    OutputMappings                  *VariableMapping
    MaxRetries                      int
    InitialRetryDelay               *time.Duration
    ExponentialBackoff              bool
    Timeout                         *time.Duration
    TimeoutBehavior                 string // "error" (default), "abort", "wait"
    Loop                            *LoopConfig
    MultiInstance                   *MultiInstanceConfig
    MaxExecutionsPerProcessInstance int
    ExitPorts                       []ExitPort // converted to the keyed map internally
}

// Clone returns a new *NativeFunctionTask[Outputs] with the same type
// parameter. Function-logic fields (Fn, Description) are shared by
// reference with the receiver — opts.Fn and opts.Description are ignored
// at Clone time. Task-level fields are taken **only** from opts — the
// receiver's task-level fields are reset, not inherited. Specify every
// task-level field you want set on the clone.
func (t *NativeFunctionTask[Outputs]) Clone(opts NativeFunctionTaskOpts[Outputs]) *NativeFunctionTask[Outputs]

// Evaluate is the runtime entry point invoked by the scheduler. It calls
// Fn, reflects the returned struct into a values map, and records it as a
// Pending transaction. See § Evaluate below.
func (t *NativeFunctionTask[Outputs]) Evaluate(ctx *ExecutionContext, executionID string) error
```

- `Fn` receives the current `ExecutionContext` (after any `InputMappings` have been applied) and returns the declared `Outputs` struct.
- Every exported field of `Outputs` is recorded as a separate output column under `(t.Id, executionID)`; consumers read individual columns via `ctx.Get("<task-id>.<column-name>")`.

## Outputs struct

The `Outputs` type parameter declares the task's typed outputs. The rules match [`DecisionNode` § Outputs structs](../decision-tasks/decision-node.spec.md#outputs-structs):

- **Every exported field is an output.** Unexported fields are ignored.
- **Field types must implement `BlValue`** (`BlNumber`, `BlString`, `BlBoolean`, `BlList`, `BlContext`, `BlDateTime`, `BlDaysTimeDuration`, etc.). A field whose type does not implement `BlValue` produces a `ProcessDefinitionError` at construction time.
- **Column name** defaults to the lowercased field name. Override with a `` `bl:"name"` `` struct tag.
- **At least one exported field** is required. An `Outputs` struct with no exported fields produces a `ProcessDefinitionError` — a task must declare at least one output. If the function genuinely emits no useful value, declare a single-field struct (e.g. `Status BlString`) so the recorded transaction has a meaningful column name.
- **Duplicate effective names** (one field's `bl:"name"` collides with another field's default or tagged name within the same struct) produce a `ProcessDefinitionError`.
- **Single-output ergonomics.** An `Outputs` struct with exactly one field is the natural shape for tasks that produce a single named value (e.g. `type FetchScoreOutputs struct { Score BlNumber }`). Consumers still read it via the qualified key (`ctx.Get("fetch-score.score")`).

`NewNativeFunctionTask` runs this reflection once at construction and caches the result on the task; subsequent `Evaluate` calls reuse the cached field-to-name map.

## Evaluate

```go
func (t *NativeFunctionTask[Outputs]) Evaluate(ctx *ExecutionContext, executionID string) error
```

`Evaluate` is the runtime entry point invoked by the scheduler. It calls the function body and records the returned struct as a Pending transaction under `(t.Id, executionID)`. Behaviour:

1. Call `t.Fn(ctx)`. The function operates on `ctx` after the scheduler has already applied `InputMappings`.
2. If `Fn` returns a non-nil error, return that error immediately. `Evaluate` does **not** call `ctx.Abort` — the scheduler still owns abort decisions (see [process.spec.md § Execution](./process.spec.md#execution)).
3. On success: reflect on the returned `Outputs` value to build a `map[string]any` keyed by effective name (the field's `bl:"name"` tag or its lowercased Go field name). If `t.OutputMappings` is set, apply it to that map before recording.
4. Call `ctx.Record(t.Id, executionID, values)`. The transaction lands as Pending; the scheduler drives `Commit` on success and `Abort` on failure (unchanged from today).
5. Return `nil`.

Tests and ad-hoc callers that want the typed `Outputs` value should call `task.Fn(ctx)` directly — the same call `Evaluate` ultimately makes — and apply any post-processing they need (validation, assertions) on the returned struct. `Evaluate` itself discards the typed return because its job is to record into `ExecutionContext`, not to surface values back to callers.

Function bodies that need to emit additional transactions during execution (rare but allowed) may continue to call `ctx.Record(t.Id, executionID, ...)` themselves. Those land as additional Pending transactions sharing the same `executionID`. `Evaluate`'s automatic `Record` of the return value is additive, not exclusive.

**Asymmetry with `DecisionTask.Evaluate`.** `DecisionTask.Evaluate(input map[string]any) (DecisionResult, error)` ([decision-task.spec.md](../decision-tasks/decision-task.spec.md)) is a **standalone unit-testing helper** that bypasses task-level framing — it neither reads from nor writes to an `ExecutionContext`. `NativeFunctionTask.Evaluate(ctx, executionID) error` is the **runtime entry point** the scheduler calls. The shared name is intentional (one canonical verb for "run this task") but the methods are not interchangeable.

## Retry and Timeout

- **`max_retries`** — the maximum number of retry attempts after the initial failure. Default `0` (no retries). A value of `3` means up to 4 total executions (1 initial + 3 retries).
- **`initial_retry_delay`** — the delay before the first retry attempt after a failure. If `nil`, the first retry begins immediately. When `exponential_backoff` is `true`, this also seeds the doubling sequence (e.g. 1s, 2s, 4s, 8s). The name reflects that this is the delay before the first *retry*, not before the initial attempt.
- **`exponential_backoff`** — when `true`, the delay doubles after each retry (e.g. 1s, 2s, 4s, 8s).
- **`timeout`** — the total time budget for all attempts including delays. The timer starts when the first execution begins. If `nil`, there is no time limit.
- **`timeout_behavior`** — controls what happens when the timeout is reached while an attempt is in progress:
  - `"abort"` — kill the in-progress attempt immediately and fail the task.
  - `"wait"` — let the in-progress attempt finish. If it succeeds, the task succeeds. If it fails, no further retries are attempted.
  - `"error"` — let the in-progress attempt finish, then produce an error regardless of the outcome. This is the default.

## Exit Ports

Exit ports on a `NativeFunctionTask` are configured **only** via `opts.ExitPorts` using the standalone constructors (`NewInterruptingTimerExitPort`, `NewNonInterruptingTimerExitPort`, `NewInterruptingTimerExitPortUntilDateTime`, `NewNonInterruptingTimerExitPortUntilDateTime`, `NewErrorExitPort`, `NewInterruptingConditionalExitPort`, `NewNonInterruptingConditionalExitPort` — see [task-nodes.spec.md § Timer Exit Port](./task-nodes.spec.md#timer-exit-port), [§ Error Exit Port](./task-nodes.spec.md#error-exit-port), [§ Conditional Exit Port](./task-nodes.spec.md#conditional-exit-port)). The legacy `task.AddInterruptingWaitForDuration(...)` / `AddErrorExitPort(...)` / `AddInterruptingConditional(...)` mutator methods do not apply to `NativeFunctionTask`.

## Writing a Native Function

The function body is declared inline as the `Fn` field of `NativeFunctionTaskOpts`. The receiver is the full `ExecutionContext` — after any `InputMappings` have been applied — and the function returns the declared `Outputs` struct. Authoring a task therefore creates the function and its task-level framing in one declaration that lives in the function's package, exported as a package-scope `var`.

### Using the execution context

`Fn` reads variables from earlier tasks or inspects process state via `ctx`.

```go
// loan_app/scoring.go
package scoring

type CalculateScoreOutputs struct {
    Score  BlNumber
    Reason BlString
}

var CalculateScore = blkit.NewNativeFunctionTask(blkit.NativeFunctionTaskOpts[CalculateScoreOutputs]{
    Id:   "calc-score",
    Name: "Calculate Score",
    Fn: func(ctx *ExecutionContext) (CalculateScoreOutputs, error) {
        isValid := ctx.Get("validate.is_valid").ToNativeBool()
        creditScore := ctx.Get("credit-report.score").ToNativeFloat()
        income := ctx.Get("check-income.annual_income").ToNativeFloat()

        if !isValid {
            return CalculateScoreOutputs{
                Score:  Bl.Number(0),
                Reason: Bl.String("Failed validation"),
            }, nil
        }

        score := creditScore*0.6 + min(income/1000, 200)*0.4

        return CalculateScoreOutputs{
            Score:  Bl.Number(score),
            Reason: Bl.String("Calculated from credit and income"),
        }, nil
    },
})

// Consumers read individual columns via the qualified key:
//   ctx.Get("calc-score.score")
//   ctx.Get("calc-score.reason")
//
// At the process site (e.g. loan_app/loan.go), reference the package-scope task directly:
//   start.To(scoring.CalculateScore).To(end)
```

### Using mapped variables

When `InputMappings` are set, they reshape the context before `Fn` is called — variables are accessible under their mapped names. This decouples the function from the specific variable paths in the calling process, which is useful when the same function is reused across processes (via [`Clone`](#reuse-via-clone)).

```go
// loan_app/validation.go
package validation

type ValidateApplicationOutputs struct {
    IsValid         BlBoolean `bl:"is_valid"`
    ValidationNotes BlString  `bl:"validation_notes"`
}

var ValidateApplication = blkit.NewNativeFunctionTask(blkit.NativeFunctionTaskOpts[ValidateApplicationOutputs]{
    Id:   "validate",
    Name: "Validate Application",
    Fn: func(ctx *ExecutionContext) (ValidateApplicationOutputs, error) {
        applicant := ctx.Get("applicant").(*BlContext)
        loanAmount := ctx.Get("loan_amount").ToNativeFloat()

        age := applicant.Get("age").ToNativeInt()

        isValid := age >= 18 && loanAmount > 0
        notes := "Failed validation"
        if isValid {
            notes = "All checks passed"
        }

        return ValidateApplicationOutputs{
            IsValid:         Bl.Boolean(isValid),
            ValidationNotes: Bl.String(notes),
        }, nil
    },
    InputMappings: NewVariableMapping(
        [2]string{"start.applicant", "applicant"},
        [2]string{"start.loan_amount", "loan_amount"},
    ),
})

// In a process file (e.g. loan_app/loan.go):
var loanProcess = NewProcess("loan-process", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        Start("start", "Start", NewInputContract()).
            To(validation.ValidateApplication).
            To(End("done", "Done")),
    },
})
```

## Reuse via Clone

To use the same function across multiple processes, declare the task once in its package and `Clone` it at each additional use site. The function body is shared by reference; task-level fields (`Id`, `Name`, mappings, retry config, exit ports) are **reset on Clone** and must be supplied in `opts`. The one-node-one-process rule means the original package-scope task can participate directly in one process; subsequent uses require a `Clone`.

```go
// loan_app/scoring.go — declared once
package scoring

type CalculateScoreOutputs struct {
    Score  BlNumber
    Reason BlString
}

var CalculateScore = blkit.NewNativeFunctionTask(blkit.NativeFunctionTaskOpts[CalculateScoreOutputs]{
    Id:   "calc-score",
    Name: "Calculate Score",
    Fn:   func(ctx *ExecutionContext) (CalculateScoreOutputs, error) { /* body */ },
})

// Process A — uses the package-scope task directly.
var processA = NewProcess("loan-fast-track", "1.0", ProcessOpts{
    Graph: []ProcessNode{startA.To(scoring.CalculateScore).To(endA)},
})

// Process B — Clone, supplying task-level fields fresh. The [Outputs]
// type parameter is preserved automatically.
var processB = NewProcess("loan-full-review", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        startB.To(scoring.CalculateScore.Clone(NativeFunctionTaskOpts[CalculateScoreOutputs]{
            Id:                 "calc-score",
            Name:               "Calculate Score (Full Review)",
            MaxRetries:         3,
            ExponentialBackoff: true,
        })).To(endB),
    },
})
```

`Clone` resets task-level fields — it does not inherit them from the source. Specify every task-level field you want on the clone via `opts`. Repeated calls to `Clone` on the same source produce independent instances; mutations to one (via `opts` at clone time) do not propagate to others or to the source.

## Edge Cases

- A `NativeFunctionTask` whose `Fn` is nil is invalid and produces a `ValidationError`.
- A `NativeFunctionTask[Outputs]` whose `Outputs` type parameter has no exported fields is a `ProcessDefinitionError` at construction. A field whose type does not implement `BlValue` is also a `ProcessDefinitionError`. Duplicate effective output names (collision between an explicit `bl:"name"` and a default lowercased field name) are a `ProcessDefinitionError`.

See [task-nodes.spec.md § Edge Cases](./task-nodes.spec.md#edge-cases) for cross-cutting task edge cases (missing `Id`, conflicting `loop` and `multi_instance`, exit-port id collisions, etc.) that apply to every task type including this one.

## See Also

- [task-nodes.spec.md](./task-nodes.spec.md) — base `Task` struct, other task types (`SubProcessTask`, `TriggerProcessTask`, `RequestInputTask`), cross-cutting Loop / Multi-Instance / Exit-Port sections.
- [process.spec.md](./process.spec.md) — `Process.Evaluate`, the scheduler that dispatches tasks.
- [../decision-tasks/decision-task.spec.md](../decision-tasks/decision-task.spec.md) — `DecisionTask`, the sibling generic-over-outputs task type.
- [../data/execution-context.spec.md](../data/execution-context.spec.md) — `ctx.Record`, the transaction-append primitive `Evaluate` uses.
