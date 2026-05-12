---
name: DecisionTask
description: A reusable decision task — directed graph of DecisionNodes with InputContract/OutputContract that is itself a ProcessNode. Cloneable for use across multiple processes; Id/Name/InputMappings/OutputMappings are mandatory at process-graph incorporation time.
targets:
  - ../decisions/decision_task.go
---

# DecisionTask

`DecisionTask` is the top-level container for a blkit decision, inspired by the DMN standard. It holds a directed acyclic graph of `DecisionNode`s with typed input and output contracts, and is itself a `ProcessNode` that can be placed directly in a process graph.

A single `DecisionTask` template can be reused across multiple processes by calling `task.Clone(opts)`. Each clone is an independent `*DecisionTask` whose decision-logic fields are shared by reference with the source and whose task-level fields come **only from `opts`** — the source's task-level fields are reset, not inherited.

This collapses the prior `DecisionModel` + `DecisionModelTask` two-type split into a single type. Decision logic and task-level metadata live on the same struct; reuse is via `Clone`, not via wrapping.

```go
type DecisionTask struct {
    // Decision-logic fields — set in DecisionTaskOpts at creation; treated as
    // immutable by convention thereafter. Shared by reference with clones.
    Namespace      *string         // optional URI namespace (e.g. "https://example.com/decisions")
    Description    *string
    InputContract  *InputContract  // optional; see ../data/data-contract.spec.md
    OutputContract *OutputContract // optional; see ../data/data-contract.spec.md
    Nodes          []DecisionNode  // grown via AddNode

    // Task-level fields — set in DecisionTaskOpts at creation; treated as
    // immutable by convention. Reset (not inherited) on Clone.
    Id                              string
    Name                            string
    InputMappings                   *VariableMapping
    OutputMappings                  *VariableMapping
    Loop                            *LoopConfig
    MultiInstance                   *MultiInstanceConfig
    MaxExecutionsPerProcessInstance int
    ExitPorts                       map[string]ExitPort // keyed by exit-port id
}

// Construct a DecisionTask. Any field in opts is set; unspecified fields default
// to their zero values.
func NewDecisionTask(opts DecisionTaskOpts) *DecisionTask

type DecisionTaskOpts struct {
    // Decision-logic fields
    Namespace      *string
    Description    *string
    InputContract  *InputContract
    OutputContract *OutputContract

    // Task-level fields — all optional at creation; Id, Name, InputMappings, and
    // OutputMappings are mandatory at the time of incorporation into a process graph.
    Id                              string
    Name                            string
    InputMappings                   *VariableMapping
    OutputMappings                  *VariableMapping
    Loop                            *LoopConfig
    MultiInstance                   *MultiInstanceConfig
    MaxExecutionsPerProcessInstance int
    ExitPorts                       []ExitPort // converted to the keyed map internally
}

// Add a decision node. The only mutation allowed after construction; affects
// only the receiver's Nodes slice. Treat the receiver as frozen once cloning
// begins (Nodes is shared by reference between source and clones, so adding
// a node to the source after cloning has undefined visibility on existing
// clones — see Edge Cases).
func (d *DecisionTask) AddNode(node DecisionNode) *DecisionTask

// Decision-logic validation: cycles, requires resolution, contract consistency,
// duplicate node ids, output-name uniqueness. Independent of task-level state;
// callable on a template or a clone with the same result.
func (d *DecisionTask) Validate() error

// Standalone evaluation, bypassing task-level framing (mappings, loop,
// multi-instance, exit ports). Useful for unit-testing the decision logic in
// isolation. Callable on a template or a clone with the same result.
func (d *DecisionTask) Evaluate(input map[string]any) (DecisionResult, error)

// Render the decision logic and structure as a markdown string. Independent
// of task-level state.
func (d *DecisionTask) ToMarkdown() string

// Clone returns a new *DecisionTask. Decision-logic fields (Nodes, contracts,
// Namespace, Description) are shared by reference with the receiver.
// Task-level fields are taken **only** from opts — the receiver's task-level
// fields are reset, not inherited. Specify every task-level field you want
// set on the clone.
func (d *DecisionTask) Clone(opts DecisionTaskOpts) *DecisionTask


// Result of standalone evaluation.
type DecisionResult struct {
    // The declared outputs — keyed by OutputContract field names. Only populated
    // for fields declared in the output contract (or all node results if
    // OutputContract is nil).
    Outputs map[string]BlValue

    // All evaluated node results — keyed by output_name (or id), including
    // intermediaries.
    AllResults map[string]BlValue

    // Ids of all nodes that were evaluated, in evaluation order.
    EvaluatedNodes []string
}
```

---

## Mandatory fields at process-graph incorporation

Most task-level fields are optional at creation time, but four become mandatory the moment the `DecisionTask` is incorporated into a process graph. `NewProcess()` walks the graph and rejects any `DecisionTask` whose:

- `Id` is empty,
- `Name` is empty,
- `InputMappings` is nil,
- or `OutputMappings` is nil,

as a `ProcessDefinitionError`.

Mappings are mandatory **even when the decision has no `InputContract` or `OutputContract`**. If the decision genuinely takes no inputs or produces no outputs, an empty `NewVariableMapping()` is a valid explicit declaration — but the field must be set deliberately rather than left nil. This forces every decision incorporation to make its data-flow boundaries explicit.

`Loop`, `MultiInstance`, `MaxExecutionsPerProcessInstance`, and `ExitPorts` remain individually optional always.

---

## Reuse pattern

Build the decision logic once as a template (no task-level fields), then `Clone` for each use:

```go
// Build the template — decision logic only
template := NewDecisionTask(DecisionTaskOpts{
    Description: ptr("Approves or denies a loan based on eligibility and amount"),
    InputContract: NewInputContract(
        RequiredField("applicant", applicantSchema),
        RequiredField("loan_amount", BlNumber),
    ),
    OutputContract: NewOutputContract(
        RequiredField("approval", BlString),
    ),
})
template.AddNode(eligibilityTable())
template.AddNode(approvalDecision())

// Use in process A — Id, Name, InputMappings, OutputMappings all required
riskCheckA := template.Clone(DecisionTaskOpts{
    Id:   "risk-check",
    Name: "Risk Assessment",
    InputMappings: NewVariableMapping(
        [2]string{"start.applicant",   "applicant"},
        [2]string{"start.loan_amount", "loan_amount"},
    ),
    OutputMappings: NewVariableMapping(
        [2]string{"approval", "risk-check.approval"},
    ),
})

// Use in process B with different mappings, a Loop, and an SLA exit port
slaTimer := NewInterruptingTimerExitPort("sla", Bl.DaysTimeDuration("PT1M"))

riskEvalB := template.Clone(DecisionTaskOpts{
    Id:   "risk-eval",
    Name: "Risk Evaluation",
    InputMappings: NewVariableMapping(
        [2]string{"start.applicant_v2", "applicant"},
        [2]string{"start.loan_amount",  "loan_amount"},
    ),
    OutputMappings: NewVariableMapping(
        [2]string{"approval", "risk-eval.approval"},
    ),
    Loop: NewLoopConfig(
        Bl.StringVar("risk-eval.approval").Equals(Bl.String("indeterminate")),
        3,
    ),
    ExitPorts: []ExitPort{slaTimer},
})

// Process A
processA := NewProcess("loan-app-a", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        startA.To(riskCheckA).To(approvedA),
    },
})

// Process B — exit-port flow target wired in the graph block as usual
processB := NewProcess("loan-app-b", "2.0", ProcessOpts{
    Graph: []ProcessNode{
        startB.To(riskEvalB).To(approvedB),
        riskEvalB.ExitPort("sla").To(escalateB).To(escalatedB),
    },
})
```

A `DecisionTask` can also be constructed fully baked in one step (skipping the template-and-clone pattern) if it is only ever used in one place:

```go
oneShotDecision := NewDecisionTask(DecisionTaskOpts{
    Id:   "decide",
    Name: "Decide",
    InputContract:  ic,
    OutputContract: oc,
    InputMappings:  im,
    OutputMappings: om,
})
oneShotDecision.AddNode(...)
```

---

## Building the Decision Logic

Each non-trivial decision node lives in its own dedicated, domain-named Go function (the **constructor-function idiom** — see [decision-node.spec.md](decision-node.spec.md)).

```go
var applicantSchema = NewContextContract(
    RequiredField("age", BlNumber),
    RequiredField("income", BlNumber),
)

func eligibilityTable() *DecisionTable {
    elig := &DecisionTable{
        Id:   "eligibility",
        Name: "Eligibility Check",
    }
    applicant := elig.RequireContext("applicant", applicantSchema)

    age    := elig.NumberInput("Age",    applicant.Get("age"))
    income := elig.NumberInput("Income", applicant.Get("income"))

    elig.AddOutput(OutputClause{Name: "eligibility"})

    elig.AddRule(*NewRule().
        AddInputEntry(age,    age.GreaterThanOrEqual(Bl.Number(18))).
        AddInputEntry(income, income.GreaterThanOrEqual(Bl.Number(30000))).
        AddOutputEntry("eligibility", Bl.String("eligible")))

    elig.AddRule(*NewRule().
        AddInputEntry(age, age.LessThan(Bl.Number(18))).
        AddOutputEntry("eligibility", Bl.String("ineligible")))

    elig.AddRule(*NewRule().
        AddInputEntry(income, income.LessThan(Bl.Number(30000))).
        AddOutputEntry("eligibility", Bl.String("ineligible")))

    return elig
}

func approvalDecision() *LiteralExpression {
    approval := &LiteralExpression{
        Id:   "approval",
        Name: "Loan Approval",
    }
    eligibility := approval.RequireString("eligibility")
    approval.RequireNumber("loan_amount") // declared dependency; not used in body

    approval.Body = Bl.If(
        eligibility.Equals(Bl.String("eligible")),
        Bl.String("approved"),
        Bl.String("denied"),
    )
    return approval
}

func loanApproval() *DecisionTask {
    return NewDecisionTask(DecisionTaskOpts{
        Description: ptr("Approves or denies a loan based on eligibility and amount"),
        InputContract: NewInputContract(
            RequiredField("applicant", applicantSchema),
            RequiredField("loan_amount", BlNumber),
        ),
        OutputContract: NewOutputContract(
            RequiredField("approval", BlString),
        ),
    }).
        AddNode(eligibilityTable()).
        AddNode(approvalDecision())
}

// Standalone evaluation works on the template — no Clone needed
result, err := loanApproval().Evaluate(map[string]any{
    "applicant":   Bl.Context(map[string]BlValue{"age": Bl.Number(30), "income": Bl.Number(50000)}),
    "loan_amount": Bl.Number(200000),
})

// result.Outputs         // {"approval": BlString("approved")}
// result.AllResults      // {"eligibility": ..., "approval": ...}
// result.EvaluatedNodes  // ["eligibility", "approval"]
```

The template's `InputContract` and `OutputContract` declare its external interface (callers must supply `applicant` and `loan_amount`; the decision exposes `approval`). Each node's `Require*` calls describe how it consumes data from the evaluation scope — the template auto-derives node-level dependencies from those declarations.

---

## Evaluation

### In a process

When the runtime reaches an instantiated `DecisionTask` node:

1. Apply `InputMappings` to the current `ExecutionContext` to compute the decision's `input` map.
2. Validate `input` against `InputContract` (if set). Failure produces a `DataContractValidationError`.
3. Run the decision-logic algorithm (below) against `input`.
4. Filter results by `OutputContract` (if set).
5. Apply `OutputMappings` to map result fields into target variable names.
6. Merge the mapped result into the `ExecutionContext` under the task's `Id`.

### Standalone

`Evaluate(input)` runs the decision logic without task-level framing. It performs steps 2–4 above, returning a `DecisionResult`. Useful for unit testing.

### Internal evaluation algorithm

When `Evaluate(input)` runs:

1. Topologically sort all nodes by their `requires` dependencies.
2. Nodes with no node-to-node dependencies are evaluated first, receiving only the caller-provided `input` variables.
3. Each node's output is stored in the evaluation context under its `output_name` (or `id`).
4. Downstream nodes receive the accumulated context (caller inputs + upstream outputs).
5. All nodes are evaluated, regardless of whether they appear in the output contract.
6. The `DecisionResult` contains the full results, with `Outputs` filtered to fields declared in `OutputContract`.

### Input Resolution

- `InputContract` declares the expected input variable names and types.
- When a node lists an input field name in its `requires`, the corresponding variable is resolved from the caller-provided `input` map.
- When a node lists another `DecisionNode` id in its `requires`, the dependency's output is resolved from the evaluation context.
- At evaluation time, input values are validated against `InputContract` types. A mismatch produces a `DataContractValidationError`.

---

## Input and Output Contracts

`InputContract` and `OutputContract` are independent and optional (see [data-contract.spec.md](../data/data-contract.spec.md)).

- **`InputContract`** — declares the named, typed variables the decision expects from callers. Validated at evaluation entry.
- **`OutputContract`** — declares which computed values are exposed in `DecisionResult.Outputs` and their expected types. The field names must match node `output_name` (or `id`) values. All nodes are still evaluated (intermediary results may be needed by output nodes), but only fields declared in `OutputContract` appear in `Outputs`. Full results are available in `AllResults`.

If `OutputContract == nil`, `Outputs` contains all node results (equivalent to exposing everything). If `InputContract == nil`, no input validation runs.

---

## Validation

Validation splits across two layers.

### Decision-logic validation

`task.Validate()` checks the decision logic (callable on template or clone — they share these fields):

- No circular dependencies among nodes.
- All ids in `requires` resolve to a `DecisionNode` id or an `InputContract` field name within the task.
- No duplicate node ids.
- No duplicate `output_name` values across nodes.
- Every node has a non-empty `id`.
- If `InputContract` is set: every field is referenced by at least one node's `requires`.
- If `OutputContract` is set: every field matches a node's `output_name` (or `id`); node output types are compatible with the declared contract types (where statically determinable).

### Task-level validation (at process construction)

When `NewProcess()` walks the graph and encounters a `DecisionTask`, it checks:

- `Id` is non-empty.
- `Name` is non-empty.
- `InputMappings` is non-nil.
- `OutputMappings` is non-nil.
- `Loop` and `MultiInstance` are not both set (inherited Task constraint).
- `MaxExecutionsPerProcessInstance >= 0`.
- Mappings reference variables that exist in the surrounding process input or upstream task outputs (subject to the same rules as other Tasks).

A violation produces a `ProcessDefinitionError`.

---

## Markdown Rendering

`ToMarkdown()` returns a complete markdown document representing the decision template — its logic, contracts, and node structure. It is independent of task-level state; called on a template or a clone produces identical output.

### Format

- **Title** — the task's `Name` (or `Id`) as a level-1 heading.
- **Description** — the task's `Description`, if set.
- **Inputs** — a table listing each `InputContract` field name, its FEEL type, and required/optional status.
- **Outputs** — a table listing each `OutputContract` field name, its FEEL type, and required/optional status.
- **Nodes** — each node rendered via its own `ToMarkdown()`, in dependency order, separated by horizontal rules.
- **Dependencies** — a summary listing each node and its `requires` dependencies.

To render a process's overall structure including instantiated decision tasks, use `Process.ToMarkdown()` (see [../processes/process.spec.md](../processes/process.spec.md)).

---

## Edge Cases

- A `DecisionTask` whose `Id`, `Name`, `InputMappings`, or `OutputMappings` is empty/nil cannot be added to a process graph. `NewProcess()` raises `ProcessDefinitionError`.
- An empty `NewVariableMapping()` is a valid `InputMappings` or `OutputMappings` value — it explicitly declares "no variables flow in/out." A nil `*VariableMapping` is rejected; use the empty constructor instead.
- Calling `Clone` repeatedly on the same template produces independent instances. Mutations on one (in opts at clone time) do not propagate to others or to the source.
- `Clone` resets task-level fields — it does not inherit them from the source. Specify every task-level field you want on the clone via opts.
- Decision-logic fields (`Nodes`, `InputContract`, `OutputContract`, `Namespace`, `Description`) are shared by reference between a source and its clones. Adding a `DecisionNode` to the source after `Clone` has produced instances has undefined visibility on existing clones — treat the source as frozen once cloning begins.
- Adding a `DecisionNode` to a clone via `clone.AddNode(node)` mutates the underlying shared `Nodes` slice. To produce a clone with extended decision logic, build a fresh template and clone from it.
- `Evaluate(input)` is callable on a template or a clone and produces identical results — task-level framing is not exercised by the standalone path.
- A template with a single node and no contracts is valid and can be cloned.
- `Evaluate(input)` with an input variable not referenced by any node is silently ignored.
- A missing required input variable is resolved as `BlNull` (consistent with FEEL semantics) — but `InputContract` validation runs first and may reject it before evaluation begins.
- `Evaluate(input)` on an invalid template (circular dependencies, unresolved references) returns a `DecisionDefinitionError` without evaluating any nodes.
- If a node faults during evaluation, the error propagates — downstream dependents also fault. Nodes not in the dependency chain of the faulted node are still evaluated.
- A `DecisionTask` with no nodes evaluates successfully, returning an empty result.
- A task with `InputContract == nil` accepts any input variables without type checking.
- A task with `OutputContract == nil` exposes all node results in `Outputs`.
- A clone with `Loop` and `MultiInstance` both set produces a `ProcessDefinitionError`.
- Exit ports are configured **only** via `opts.ExitPorts`. The inherited `Task.AddInterruptingWaitForDuration(...)` / `AddErrorExitPort(...)` / `AddInterruptingConditional(...)` etc. methods do not apply to `DecisionTask`. Use the standalone constructors (`NewInterruptingTimerExitPort`, `NewErrorExitPort`, etc. — see [../processes/task-nodes.spec.md](../processes/task-nodes.spec.md)) to build exit ports for `opts.ExitPorts`.
- Exit-port flow targets are wired via `task.ExitPort(id).To(...)` in the graph block, as for any other Task. This is graph-construction wiring, not task mutation.
- Exit-port ids must be unique within a single `DecisionTask` instance. Ids are not shared across clones — two clones of the same template can each have an exit port called `"sla"`.
