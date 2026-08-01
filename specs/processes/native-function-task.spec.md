---
name: NativeFunctionTask
description: A Go-function-backed task node, generic over a caller-supplied outputs struct; declared together with its function body and reused across processes via Clone
status: agreed
code:
  - core/native_function_task.go
---

# NativeFunctionTask

Invokes a Go function directly. The function and its task-level framing are declared together via `NewNativeFunctionTask`; the resulting `*NativeFunctionTask[Outputs]` is itself a `ProcessNode` that can be placed in a process graph. Reuse across multiple processes is via `Clone(opts)` — function-logic fields are shared by reference with clones; task-level fields are reset (not inherited) on `Clone`.

`NativeFunctionTask` is **generic over a caller-supplied outputs struct** that declares the task's typed outputs. This is the same pattern [`DecisionNode`](../decision-tasks/decision-node.spec.md#outputs-structs) already uses. The framework reflects on the `Outputs` struct at construction to derive the output field names and validate field types. At execution time, `Evaluate` reflects on the returned value to build the `map[string]any` passed to `ctx.Record`.

The shape mirrors [`DecisionTask`](../decision-tasks/decision-task.spec.md): a single type holding both the logic and the task-level metadata, reused via `Clone`, not via wrapping or a separate factory.

```go
type NativeFunctionTask[Inputs any, Outputs any] struct {
    // Function-logic fields — set in NativeFunctionTaskOpts at creation;
    // treated as immutable by convention thereafter. Shared by reference
    // with clones.
    Fn            func(in *Inputs) (Outputs, error)
    InputBindings []ParameterBinding // resolved into a keyed map internally
    Description   string

    // Task-level fields — set in NativeFunctionTaskOpts at creation;
    // treated as immutable by convention. Reset (not inherited) on Clone.
    Id                              string
    Name                            string
    MaxRetries                      int
    InitialRetryDelay               *time.Duration
    ExponentialBackoff              bool
    Timeout                         *time.Duration
    TimeoutBehavior                 string // "error" (default), "abort", "wait"
    Loop                            *LoopConfig
    MultiInstance                   *MultiInstanceConfig
    MaxExecutionsPerProcessInstance int
    ExitPorts                       map[string]ExitPort // keyed by exit-port id

    // Typed handle fields, populated by NewNativeFunctionTask via reflection.
    // Each field on Inputs / Outputs holds a permanent symbolic handle stamped
    // with (sourceTaskId: t.Id, fieldName: <Go-field-name>, type: <Bl-type>).
    //
    // - Inputs holds the LHS handles that this task's InputBindings closure
    //   pairs with upstream Outputs handles via bl.Bind(in.X, ...).
    // - Outputs holds the RHS handles that downstream tasks reference as the
    //   source side of their own InputBindings (e.g. validate.Outputs.IsValid).
    //
    // These handles never carry runtime values — values flow through
    // ExecutionContext. See "How handles, bindings, and runtime values fit
    // together" below.
    Inputs  Inputs
    Outputs Outputs
}

// Construct a NativeFunctionTask. opts.Fn is required and must be non-nil;
// opts.Id and opts.Name are mandatory at the time of incorporation into a
// process graph. NewNativeFunctionTask reflects on both type parameters,
// validates the field rules described in "Inputs struct" and "Outputs
// struct" below, allocates typed handles into t.Inputs and t.Outputs, then
// calls opts.InputBindings(t.Inputs) to obtain the binding list. Invalid
// Inputs/Outputs structs or invalid bindings produce a
// ProcessDefinitionError at construction. opts.ExitPorts is converted to
// the keyed map internally. Any other field in opts is set verbatim;
// unspecified fields default to their zero values.
func NewNativeFunctionTask[Inputs any, Outputs any](opts NativeFunctionTaskOpts[Inputs, Outputs]) *NativeFunctionTask[Inputs, Outputs]

type NativeFunctionTaskOpts[Inputs any, Outputs any] struct {
    // Function-logic fields
    Fn          func(in *Inputs) (Outputs, error)
    Description string

    // InputBindings is a closure called by NewNativeFunctionTask after Inputs
    // handles are allocated. It receives the populated Inputs value (handles
    // in each field) and returns the list of (parameter, argument) bindings —
    // parameter is an Inputs handle read out of in.X; argument is an
    // expression of the matching Bl* type (typically an upstream task's
    // Outputs handle). See "InputBindings" below.
    InputBindings func(in Inputs) []ParameterBinding

    // Task-level fields — all optional at creation; Id and Name are mandatory
    // at the time of incorporation into a process graph.
    Id                              string
    Name                            string
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

// Clone returns a new *NativeFunctionTask[Inputs, Outputs] with the same
// type parameters. Function-logic fields (Fn, Description) are shared by
// reference with the receiver — opts.Fn and opts.Description are ignored
// at Clone time. Task-level fields and InputBindings are taken **only**
// from opts — the receiver's task-level fields and bindings are reset,
// not inherited. Specify every task-level field and a fresh InputBindings
// closure you want set on the clone.
func (t *NativeFunctionTask[Inputs, Outputs]) Clone(opts NativeFunctionTaskOpts[Inputs, Outputs]) *NativeFunctionTask[Inputs, Outputs]

// Evaluate is the runtime entry point invoked by the scheduler. It resolves
// the InputBindings against ctx to populate a fresh Inputs value, calls Fn,
// reflects the returned Outputs struct into a values map, and records it
// as a Pending transaction. See "Evaluate" below.
func (t *NativeFunctionTask[Inputs, Outputs]) Evaluate(ctx *ExecutionContext, executionID string) error
```

- `Fn` is a **pure function** over the typed `Inputs` struct — it does **not** receive `*ExecutionContext`. The framework populates `*in` from the `InputBindings` before each call.
- Every exported field of `Outputs` is recorded as a separate output field under `(t.Id, executionID)`; consumers read individual fields by referencing this task's `Outputs.<field>` handle in their own `InputBindings`.

## Inputs struct

The `Inputs` type parameter declares the task's typed inputs.

- **Every exported field is an input.** Unexported fields are ignored.
- **Field types must implement `bl.BlValue`** (`bl.BlNumber`, `bl.BlString`, `bl.BlBoolean`, `bl.BlList`, `bl.BlDictionary`, `bl.BlDateTime`, `bl.BlDaysTimeDuration`, etc.). A field whose type does not implement `bl.BlValue` produces a `ProcessDefinitionError` at construction time.
- **Field name is the literal Go field name.** No transformation — `LoanAmount` is bound as `LoanAmount`.
- **Duplicate field names** within the same struct produce a `ProcessDefinitionError`.
- **`Inputs` may be empty** (`struct{}` or a type with no exported fields). A task that genuinely reads nothing declares an empty struct; the `InputBindings` closure returns an empty slice. This differs from `Outputs`, which requires at least one field.

`NewNativeFunctionTask` allocates a typed handle into each exported field of `t.Inputs` at construction, stamped with `(sourceTaskId: t.Id, fieldName: <Go-field-name>, type: <Bl-type>)`. These handles are the LHS of the bindings produced by the `InputBindings` closure — `bl.Bind(in.IsValid, ...)` reads the handle out of the populated `in` value the closure receives.

## InputBindings

```go
InputBindings func(in Inputs) []ParameterBinding
```

`opts.InputBindings` is a closure that wires this task's typed inputs to upstream values. `NewNativeFunctionTask` calls it after allocating handles on the task's `Inputs` field, passing a copy of the populated `Inputs` value (each field holds its typed handle). The closure body pairs each LHS handle from `in.X` with an RHS expression of the matching `Bl*` type — typically an upstream task's `Outputs.X` handle, but any `bl.BlExpr` of the matching type works.

Bindings are expressed with the `Bind` helper, which pairs a parameter handle with an argument expression of the same `Bl*` type:

```go
type ParameterBinding struct {
    Parameter BlExpr // the LHS handle being bound (an Inputs field handle)
    Argument  BlExpr // the RHS expression supplying its value
}

// Bind pairs a parameter handle with an argument expression. The type parameter
// is inferred from the parameter's Bl* type and enforces that the argument has
// the same type — a mismatch is a compile error at the call site.
func Bind[T BlValue](parameter T, argument T) ParameterBinding
```

The type parameter is inferred from the parameter handle's `Bl*` type and enforces that the argument has the same type — mismatches are compile errors at the call site.

### Validation

`NewNativeFunctionTask` validates the returned bindings:

- **Every `Inputs` handle is bound exactly once.** Unbound handles → `ProcessDefinitionError` at construction. No silent `bl.BlNull` defaults — explicit is better for tasks because runtime resolution failures are harder to diagnose than construction-time errors. If a caller genuinely wants a default, they bind to a literal expression (`bl.Boolean(false)`, `bl.Number(0)`, etc.).
- **No duplicate bindings on the same `Inputs` field.** A second binding with an already-bound LHS handle → `ProcessDefinitionError`.
- **The `Parameter` side of every binding is one of this task's `Inputs` handles.** Passing a handle from a different task as the LHS → `ProcessDefinitionError`.

### How handles, bindings, and runtime values fit together

Task instances are immutable and shared across many process evaluations. They cannot store per-run values. The handle/binding machinery is the type-safe "address" half of value flow; the actual values live in `ExecutionContext` (the append-only log defined in [../data/execution-context.spec.md](../data/execution-context.spec.md)).

- **A handle is an interface value with a handle-shaped concrete implementation.** `validate.Outputs.IsValid` is a `bl.BlBoolean`. Its concrete type is a small struct stamped with `(sourceTaskId, fieldName, type)`; it implements the `bl.BlBoolean` interface by *resolving against an `ExecutionContext` at evaluation time*, not by carrying a `bool`. Handles allocated into `t.Inputs` follow the same shape. Handles are immutable for the lifetime of the program.
- **Values flow through `ExecutionContext`.** When `validate.Evaluate(ctx, execId)` runs, `Fn` returns a concrete `ValidateOutputs{IsValid: bl.Boolean(true), ...}` — a fresh struct, separate from `validate.Outputs`, whose fields hold concrete `Bl*` values like `bl.Boolean(true)`. `Evaluate` reflects the struct into a map and calls `ctx.Record("validate", execId, {"IsValid": bl.Boolean(true), ...})`. The handles on `validate.Outputs` never see the value.
- **Downstream `Evaluate` resolves handles against `ctx`.** When `CalculateScore.Evaluate(ctx, execId)` runs later: for each `ParameterBinding` in `t.InputBindings`, the upstream handle (e.g. `validate.Outputs.IsValid`) is resolved against `ctx` — equivalent to `ctx.Get("validate.IsValid")`. The resolved concrete value is assigned into a fresh `*Inputs` struct under the binding's LHS field. `Fn(in)` runs with concrete values throughout.
- **The same handle works across replays, suspends, multi-instance, and parallel branches** — because resolution is parameterised over `ctx`. The task object holds no per-run state; whatever `ctx` is current at `Evaluate` time supplies the value.

The bindings are purely a **construction-time wiring artifact**: they encode "where to look in `ctx`" with full type safety. At runtime they're walked to populate `*Inputs` from `ctx`. The scheduler and `ExecutionContext` contracts are unchanged — handles are just a typed expression of the same `ctx.Get` lookup that string-keyed function bodies were doing before.

## Outputs struct

The `Outputs` type parameter declares the task's typed outputs. The rules match [`DecisionNode` § Outputs structs](../decision-tasks/decision-node.spec.md#outputs-structs):

- **Every exported field is an output.** Unexported fields are ignored.
- **Field types must implement `bl.BlValue`** (`bl.BlNumber`, `bl.BlString`, `bl.BlBoolean`, `bl.BlList`, `bl.BlDictionary`, `bl.BlDateTime`, `bl.BlDaysTimeDuration`, etc.). A field whose type does not implement `bl.BlValue` produces a `ProcessDefinitionError` at construction time.
- **Field name is the literal Go field name.** No transformation — `LoanAmount` records as `LoanAmount`, consumers read it as `ctx.Get("<task-id>.LoanAmount")`.
- **At least one exported field** is required. An `Outputs` struct with no exported fields produces a `ProcessDefinitionError` — a task must declare at least one output. If the function genuinely emits no useful value, declare a single-field struct (e.g. `Status bl.BlString`) so the recorded transaction has a meaningful field name.
- **Duplicate field names** within the same struct produce a `ProcessDefinitionError`.
- **Single-output ergonomics.** An `Outputs` struct with exactly one field is the natural shape for tasks that produce a single named value (e.g. `type FetchScoreOutputs struct { Score bl.BlNumber }`). Consumers still read it via the qualified key (`ctx.Get("fetch-score.Score")`).

`NewNativeFunctionTask` runs this reflection once at construction and caches the result on the task; subsequent `Evaluate` calls reuse the cached field-to-name map. The same pass also allocates a typed handle into every exported field of `t.Outputs`; those handles are the symbolic references downstream tasks use to wire to this task's recorded values (see [§ InputBindings § How handles, bindings, and runtime values fit together](#how-handles-bindings-and-runtime-values-fit-together) above).

## Evaluate

```go
func (t *NativeFunctionTask[Inputs, Outputs]) Evaluate(ctx *ExecutionContext, executionID string) error
```

`Evaluate` is the runtime entry point invoked by the scheduler. It resolves the `InputBindings` against `ctx` to populate a fresh `*Inputs`, calls the pure function `Fn`, and records the returned `Outputs` as a Pending transaction under `(t.Id, executionID)`. Behaviour:

1. **Resolve bindings.** For each `ParameterBinding` in `t.InputBindings`, evaluate the binding's `Argument` expression against `ctx` to produce a concrete `bl.BlValue`. (For the common case of `bl.Bind(in.X, upstream.Outputs.Y)`, evaluation is `ctx.Get("<upstream-id>.<y-field>")`.)
2. **Populate a fresh `*Inputs`.** Allocate a new `Inputs` value and assign each resolved value into the field identified by the binding's `Parameter` handle, using the cached field-to-handle map. `t.Inputs` retains its construction-time handles untouched.
3. **Call `t.Fn(in)`.** If `Fn` returns a non-nil error, return it immediately. `Evaluate` does **not** call `ctx.Abort` — the scheduler still owns abort decisions (see [process.spec.md § Execution](./process.spec.md#execution)).
4. **Record the returned `Outputs`.** Reflect on the returned struct (a fresh value, separate from `t.Outputs`, with concrete `Bl*` values populated by `Fn`) to build a `map[string]any` keyed by each field's Go name (the same key the construction-time handle on `t.Outputs.X` was stamped with). Call `ctx.Record(t.Id, executionID, values)`. The transaction lands as Pending; the scheduler drives `Commit` on success and `Abort` on failure (unchanged from today). `t.Outputs` retains its handles untouched.
5. Return `nil`.

The duality is worth calling out explicitly: **`t.Inputs` / `t.Outputs` are construction-time handle structs that downstream tasks reference for wiring; the `*Inputs` value passed to `Fn` and the `Outputs` value returned by `Fn` are per-evaluation value structs that carry the actual data.** Same Go type on both sides, opposite contents.

Tests and ad-hoc callers that want the typed `Outputs` value should call `task.Fn(&in)` directly with a hand-constructed `Inputs` value — bypasses binding resolution and `ctx.Record` entirely.

### Fn is a pure function over Inputs

`Fn` has signature `func(in *Inputs) (Outputs, error)`. It does **not** receive `*ExecutionContext` — it is a pure function over the typed `Inputs` struct.

Consequences of the pure-function shape:

- **Everything `Fn` reads from process state must be declared as an `Inputs` field and bound.** There is no `ctx.Get` escape hatch.
- **`Fn` cannot record intermediate side transactions via `ctx.Record`** — the only output channel is the returned `Outputs` struct. If a task needs to publish multiple records during a long-running operation, model it as multiple tasks (or include each value as an `Outputs` field).
- **`Fn` cannot inspect `ExecutionHistory`, the current instance id, or any non-Inputs runtime state.** If a task genuinely needs one of these, declare it as an `Inputs` field whose binding resolves to the appropriate built-in expression (e.g. `bl.InstanceID()`).

The benefit: `Fn` is trivially unit-testable (construct an `Inputs` value, call `Fn(&in)`, assert on the returned `Outputs`); a glance at `Inputs` and `Outputs` tells you exactly what the task reads and writes.

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

A task author declares two structs (`Inputs`, `Outputs`), a `Fn` body that maps between them, and an `InputBindings` closure that wires each `Inputs` field to an upstream value. The function and its task-level framing live together in one package-scope `var`.

### Typed inputs from upstream tasks

`InputBindings` pairs each input handle with a `bl.BlValue` expression of the matching type — typically an upstream task's `Outputs` handle. The pure `Fn` body reads from `*in` only; no `ctx.Get`, no string keys.

```go
// loan_app/scoring.go
package scoring

type CalculateScoreInputs struct {
    IsValid     BlBoolean
    CreditScore BlNumber
    Income      BlNumber
}

type CalculateScoreOutputs struct {
    Score  BlNumber
    Reason BlString
}

var CalculateScore = blkit.NewNativeFunctionTask(blkit.NativeFunctionTaskOpts[CalculateScoreInputs, CalculateScoreOutputs]{
    Id:          "calc-score",
    Name:        "Calculate Score",
    Description: "Combines credit score and income into a final score, gated on validation.",
    InputBindings: func(in CalculateScoreInputs) []ParameterBinding {
        return []ParameterBinding{
            bl.Bind(in.IsValid,     validation.ValidateApplication.Outputs.IsValid),
            bl.Bind(in.CreditScore, credit.PullCreditReport.Outputs.Score),
            bl.Bind(in.Income,      income.CheckIncome.Outputs.AnnualIncome),
        }
    },
    Fn: func(in *CalculateScoreInputs) (CalculateScoreOutputs, error) {
        if !in.IsValid.ToNativeBool() {
            return CalculateScoreOutputs{
                Score:  bl.Number(0),
                Reason: bl.String("Failed validation"),
            }, nil
        }
        score := in.CreditScore.ToNativeFloat()*0.6 + min(in.Income.ToNativeFloat()/1000, 200)*0.4
        return CalculateScoreOutputs{
            Score:  bl.Number(score),
            Reason: bl.String("Calculated from credit and income"),
        }, nil
    },
})

// Downstream tasks reference CalculateScore.Outputs.Score and
// CalculateScore.Outputs.Reason as typed handles in their own InputBindings.
//
// At the process site (e.g. loan_app/loan.go), reference the package-scope task directly:
//   start.To(scoring.CalculateScore).To(end)
```

### Inputs from process-start fields

Bindings can also reference the process's `StartEvent` input fields via the start node's `Outputs` handles. The same `Bind` mechanism applies — no special casing.

```go
// loan_app/validation.go
package validation

type ValidateApplicationInputs struct {
    Applicant  BlDictionary
    LoanAmount BlNumber
}

type ValidateApplicationOutputs struct {
    IsValid         BlBoolean
    ValidationNotes BlString
}

var ValidateApplication = blkit.NewNativeFunctionTask(blkit.NativeFunctionTaskOpts[ValidateApplicationInputs, ValidateApplicationOutputs]{
    Id:          "validate",
    Name:        "Validate Application",
    Description: "Checks applicant age and loan-amount eligibility; emits a pass/fail with notes.",
    InputBindings: func(in ValidateApplicationInputs) []ParameterBinding {
        return []ParameterBinding{
            bl.Bind(in.Applicant,  loanapp.Start.Outputs.Applicant),
            bl.Bind(in.LoanAmount, loanapp.Start.Outputs.LoanAmount),
        }
    },
    Fn: func(in *ValidateApplicationInputs) (ValidateApplicationOutputs, error) {
        age := in.Applicant.Get("age").ToNativeInt()
        isValid := age >= 18 && in.LoanAmount.ToNativeFloat() > 0
        notes := "Failed validation"
        if isValid {
            notes = "All checks passed"
        }
        return ValidateApplicationOutputs{
            IsValid:         bl.Boolean(isValid),
            ValidationNotes: bl.String(notes),
        }, nil
    },
})

// In a process file (e.g. loan_app/loan.go):
var loanProcess = bl.NewProcess("loan-process", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        loanapp.Start.
            To(validation.ValidateApplication).
            To(bl.End("done", "Done")),
    },
})
```

## Reuse via Clone

To use the same function across multiple processes, declare the task once in its package and `Clone` it at each additional use site. The function body is shared by reference; **task-level fields and `InputBindings` are reset on Clone** and must be supplied in `opts`. The one-node-one-process rule means the original package-scope task can participate directly in one process; subsequent uses require a `Clone`. Each clone in a different process needs a fresh `InputBindings` closure because upstream tasks are typically different.

```go
// loan_app/scoring.go — declared once
package scoring

type CalculateScoreInputs struct {
    IsValid     BlBoolean
    CreditScore BlNumber
    Income      BlNumber
}

type CalculateScoreOutputs struct {
    Score  BlNumber
    Reason BlString
}

var CalculateScore = blkit.NewNativeFunctionTask(blkit.NativeFunctionTaskOpts[CalculateScoreInputs, CalculateScoreOutputs]{
    Id:          "calc-score",
    Name:        "Calculate Score",
    Description: "Combines credit score and income into a final score, gated on validation.",
    InputBindings: func(in CalculateScoreInputs) []ParameterBinding {
        return []ParameterBinding{
            bl.Bind(in.IsValid,     fasttrack.Validate.Outputs.IsValid),
            bl.Bind(in.CreditScore, fasttrack.CreditReport.Outputs.Score),
            bl.Bind(in.Income,      fasttrack.CheckIncome.Outputs.AnnualIncome),
        }
    },
    Fn: func(in *CalculateScoreInputs) (CalculateScoreOutputs, error) { /* body */ },
})

// Process A — uses the package-scope task directly.
var processA = bl.NewProcess("loan-fast-track", "1.0", ProcessOpts{
    Graph: []ProcessNode{startA.To(scoring.CalculateScore).To(endA)},
})

// Process B — Clone, supplying task-level fields and fresh InputBindings.
// The [Inputs, Outputs] type parameters are preserved automatically.
var processB = bl.NewProcess("loan-full-review", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        startB.To(scoring.CalculateScore.Clone(NativeFunctionTaskOpts[CalculateScoreInputs, CalculateScoreOutputs]{
            Id:                 "calc-score",
            Name:               "Calculate Score (Full Review)",
            Description:        "Scoring step inside the full-review pipeline; same logic, different upstream wiring.",
            MaxRetries:         3,
            ExponentialBackoff: true,
            InputBindings: func(in CalculateScoreInputs) []ParameterBinding {
                return []ParameterBinding{
                    bl.Bind(in.IsValid,     fullreview.Validate.Outputs.IsValid),
                    bl.Bind(in.CreditScore, fullreview.CreditReport.Outputs.Score),
                    bl.Bind(in.Income,      fullreview.CheckIncome.Outputs.AnnualIncome),
                }
            },
        })).To(endB),
    },
})
```

`Clone` resets task-level fields — it does not inherit them from the source. Specify every task-level field you want on the clone via `opts`. Repeated calls to `Clone` on the same source produce independent instances; mutations to one (via `opts` at clone time) do not propagate to others or to the source.

## Edge Cases

- A `NativeFunctionTask` whose `Fn` is nil is invalid and produces a `ValidationError`.
- A `NativeFunctionTask[Inputs, Outputs]` whose `Outputs` type parameter has no exported fields is a `ProcessDefinitionError` at construction. A field on either `Inputs` or `Outputs` whose type does not implement `bl.BlValue` is also a `ProcessDefinitionError`. Duplicate field names within `Inputs` or within `Outputs` are a `ProcessDefinitionError`.
- An empty `Inputs` struct (no exported fields) is **valid** — for tasks that read nothing from process state. The `InputBindings` closure must return an empty slice in that case.
- A `NativeFunctionTask` whose `opts.InputBindings` is `nil` while `Inputs` has at least one exported field is a `ProcessDefinitionError` — every input must be bound. (An empty-Inputs task may have `nil` `InputBindings` or a closure returning an empty slice; both are accepted.)
- `InputBindings` returning a slice that does **not** bind every exported field of `Inputs` exactly once is a `ProcessDefinitionError`: unbound fields and duplicate bindings are both rejected. There is no `bl.BlNull` default for unbound fields — bind to a literal expression (e.g. `bl.Boolean(false)`) if a default is desired.
- A `ParameterBinding` whose `Parameter` is not one of this task's `Inputs` handles (e.g. accidentally referencing another task's input handle on the LHS) is a `ProcessDefinitionError`.
- A `ParameterBinding` whose `Parameter` and `Argument` have different `Bl*` types is a compile error at the `Bind[T]` call site, not a runtime error.

See [task-nodes.spec.md § Edge Cases](./task-nodes.spec.md#edge-cases) for cross-cutting task edge cases (missing `Id`, conflicting `loop` and `multi_instance`, exit-port id collisions, etc.) that apply to every task type including this one.

## See Also

- [task-nodes.spec.md](./task-nodes.spec.md) — base `Task` struct, other task types (`SubProcessTask`, `TriggerProcessTask`, `RequestInputTask`), cross-cutting Loop / Multi-Instance / Exit-Port sections.
- [process.spec.md](./process.spec.md) — `Process.Evaluate`, the scheduler that dispatches tasks.
- [../decision-tasks/decision-task.spec.md](../decision-tasks/decision-task.spec.md) — `DecisionTask`, the sibling generic-over-outputs task type.
- [../data/execution-context.spec.md](../data/execution-context.spec.md) — `ctx.Record`, the transaction-append primitive `Evaluate` uses.
