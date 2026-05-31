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
    Description    string
    InputContract  *InputContract  // optional; see ../data/data-contract.spec.md
    OutputContract *OutputContract // optional; see ../data/data-contract.spec.md
    DecisionGraph  DecisionGraph   // processed, sorted graph (built by NewDecisionTask)

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

// DecisionGraph is the processed, structured representation of a decision-node
// graph: validated (no cycles among nodes — derived from output-handle
// references inside each node's expression trees — no duplicate ids, no
// duplicate output names) and topologically sorted in evaluation order. Built
// by NewDecisionTask from opts.DecisionGraph.
type DecisionGraph struct {
    DecisionNodes []DecisionNode // in topologically-sorted evaluation order
}

// Construct a DecisionTask. Validates the raw node list in opts.DecisionGraph
// (no cycles among nodes — derived from output-handle references inside each
// node's expression trees — no duplicate node ids, no duplicate output names,
// every node has a non-empty id, OutputContract type compatibility where
// statically determinable) and stores the topologically-sorted result on
// DecisionTask.DecisionGraph. A validation failure raises a
// DecisionDefinitionError. Any other field in opts is set verbatim;
// unspecified fields default to their zero values.
func NewDecisionTask(opts DecisionTaskOpts) *DecisionTask

type DecisionTaskOpts struct {
    // Decision-logic fields
    Description    string
    InputContract  *InputContract
    OutputContract *OutputContract
    DecisionGraph  []DecisionNode // raw list; validated + sorted by NewDecisionTask into DecisionTask.DecisionGraph

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

// Standalone evaluation, bypassing task-level framing (mappings, loop,
// multi-instance, exit ports). Useful for unit-testing the decision logic in
// isolation. Callable on a template or a clone with the same result.
func (d *DecisionTask) Evaluate(input map[string]any) (DecisionResult, error)

// Render the decision logic and structure as a markdown string. Independent
// of task-level state.
func (d *DecisionTask) ToMarkdown() string

// Clone returns a new *DecisionTask. Decision-logic fields (DecisionGraph,
// contracts, Description) are shared by reference with the receiver
// — opts.DecisionGraph and the other decision-logic fields in opts are
// ignored at Clone time. Task-level fields are taken **only** from opts —
// the receiver's task-level fields are reset, not inherited. Specify every
// task-level field you want set on the clone.
func (d *DecisionTask) Clone(opts DecisionTaskOpts) *DecisionTask


// Result of standalone evaluation.
type DecisionResult struct {
    // The declared outputs — keyed by OutputContract field names. Only populated
    // for fields declared in the output contract (or all node results if
    // OutputContract is nil).
    Outputs map[string]BlValue

    // All evaluated node results — keyed by node Id, including intermediaries.
    // Multi-output nodes' values are BlDictionaries keyed by the per-field output
    // names declared on each node's outputs struct.
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

## Construction

The common case is to construct a `DecisionTask` fully baked in a single step — decision logic and task-level fields together — when it is only used in one place:

```go
var oneShotDecision = NewDecisionTask(DecisionTaskOpts{
    Id:             "decide",
    Name:           "Decide",
    InputContract:  ic,
    OutputContract: oc,
    InputMappings:  im,
    OutputMappings: om,
    DecisionGraph:  []DecisionNode{eligibility, approval},
})
```

(`eligibility` and `approval` are package-scope `var` declarations — see **Building the Decision Logic** below.)

### Reuse via Clone

To reuse the same decision logic across multiple processes, build the decision logic once as a template (no task-level fields), then `Clone` for each use:

```go
// Build the template — decision logic only
var template = NewDecisionTask(DecisionTaskOpts{
    Description: "Approves or denies a loan based on eligibility and amount",
    InputContract: NewInputContract(
        RequiredField("applicant", applicantSchema),
        RequiredField("loan_amount", BlNumber),
    ),
    OutputContract: NewOutputContract(
        RequiredField("approval", BlString),
    ),
    DecisionGraph: []DecisionNode{eligibility, approval},
})

// Use in process A — Id, Name, InputMappings, OutputMappings all required
var riskCheckA = template.Clone(DecisionTaskOpts{
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
var slaTimer = NewInterruptingTimerExitPort("sla", Bl.DaysTimeDuration("PT1M"))

var riskEvalB = template.Clone(DecisionTaskOpts{
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
var processA = NewProcess("loan-app-a", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        startA.To(riskCheckA).To(approvedA),
    },
})

// Process B — exit-port flow target wired in the graph block as usual
var processB = NewProcess("loan-app-b", "2.0", ProcessOpts{
    Graph: []ProcessNode{
        startB.To(riskEvalB).To(approvedB),
        riskEvalB.ExitPort("sla").To(escalateB).To(escalatedB),
    },
})
```

---

## Building the Decision Logic

Each concrete decision node is built via its generic constructor (`NewDecisionTable`, `NewLiteralExpression`, etc. — see [decision-node.spec.md](decision-node.spec.md)). Nodes are package-scope `var` declarations; downstream nodes reference upstream outputs through typed `.Outputs.X` fields. The constructor functions used in the previous spec revision are gone.

```go
var applicantSchema = NewDictionaryContract(
    RequiredField("age", BlNumber),
    RequiredField("income", BlNumber),
)

// `applicant` and `loanAmount` are typed handles supplied by the surrounding
// DecisionTask's input contract — see the deferred input-handle story in
// decision-node.spec.md. They appear here as placeholders.

type EligibilityOutputs struct {
    Eligibility BlString
}

var (
    eligAgeCol    = NumberInput("Age",    applicant.Get("age"))
    eligIncomeCol = NumberInput("Income", applicant.Get("income"))
)

var eligibility = NewDecisionTable[EligibilityOutputs](DecisionTableOpts{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: HitPolicyUnique,
    Inputs:    []TableInput{eligAgeCol, eligIncomeCol},
    Rules: []Rule{
        *NewRule().
            AddInputEntry(eligAgeCol,    eligAgeCol.GreaterThanOrEqual(Bl.Number(18))).
            AddInputEntry(eligIncomeCol, eligIncomeCol.GreaterThanOrEqual(Bl.Number(30000))).
            AddOutputEntry("eligibility", Bl.String("eligible")),
        *NewRule().
            AddInputEntry(eligAgeCol, eligAgeCol.LessThan(Bl.Number(18))).
            AddOutputEntry("eligibility", Bl.String("ineligible")),
        *NewRule().
            AddInputEntry(eligIncomeCol, eligIncomeCol.LessThan(Bl.Number(30000))).
            AddOutputEntry("eligibility", Bl.String("ineligible")),
    },
})

type ApprovalOutputs struct {
    Status BlString
}

var approval = NewLiteralExpression[ApprovalOutputs](LiteralExpressionOpts{
    Id:   "approval",
    Name: "Loan Approval",
    Body: Bl.If(
        eligibility.Outputs.Eligibility.Equals(Bl.String("eligible")),
        Bl.String("approved"),
        Bl.String("denied"),
    ),
})

var loanApproval = NewDecisionTask(DecisionTaskOpts{
    Description: "Approves or denies a loan based on eligibility and amount",
    InputContract: NewInputContract(
        RequiredField("applicant", applicantSchema),
        RequiredField("loan_amount", BlNumber),
    ),
    OutputContract: NewOutputContract(
        RequiredField("approval", BlString),
    ),
    DecisionGraph: []DecisionNode{eligibility, approval},
})

// Standalone evaluation works on the template — no Clone needed.
result, err := loanApproval.Evaluate(map[string]any{
    "applicant":   Bl.Context(map[string]BlValue{"age": Bl.Number(30), "income": Bl.Number(50000)}),
    "loan_amount": Bl.Number(200000),
})

// result.Outputs         // {"approval": BlString("approved")}
// result.AllResults      // {"eligibility": ..., "approval": ...}
// result.EvaluatedNodes  // ["eligibility", "approval"]
```

The template's `InputContract` and `OutputContract` declare its external interface (callers must supply `applicant` and `loan_amount`; the decision exposes `approval`).

Node-level dependencies are derived from the typed output handles referenced in each node's expression trees (rules, body, entries, rows, bindings). `NewDecisionTask` walks those trees, collects every output handle, and uses the handle's source pointer to build the producer→consumer graph. There are no string-keyed name lookups.

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

1. Iterate `DecisionGraph.DecisionNodes` in stored order (already topologically sorted by `NewDecisionTask`).
2. The first nodes have no node-to-node dependencies and receive only the caller-provided `input` variables.
3. Each node's output is stored in the evaluation context under its `Id`. Multi-output nodes' results are `BlDictionary` values keyed by the per-field output names declared on their outputs struct.
4. Downstream nodes receive the accumulated context (caller inputs + upstream outputs).
5. All nodes are evaluated, regardless of whether they appear in the output contract.
6. The `DecisionResult` contains the full results, with `Outputs` filtered to fields declared in `OutputContract`.

### Input Resolution

- `InputContract` declares the expected input variable names and types.
- Each node's dependencies are derived statically from the typed output handles referenced inside its expression trees (rules, body, entries, rows, bindings). A handle whose source is another node produces a node-to-node edge; a handle whose source is a DecisionTask-level input is resolved from the caller-provided `input` map at evaluation time.
- At evaluation time, input values are validated against `InputContract` types. A mismatch produces a `DataContractValidationError`.

---

## Input and Output Contracts

`InputContract` and `OutputContract` are independent and optional (see [data-contract.spec.md](../data/data-contract.spec.md)).

- **`InputContract`** — declares the named, typed variables the decision expects from callers. Validated at evaluation entry.
- **`OutputContract`** — declares which computed values are exposed in `DecisionResult.Outputs` and their expected types. The field names must match a node `Id` (for single-output nodes) or a `<node-id>.<output-name>` path (for individual fields of multi-output nodes). All nodes are still evaluated (intermediary results may be needed by output nodes), but only fields declared in `OutputContract` appear in `Outputs`. Full results are available in `AllResults`.

If `OutputContract == nil`, `Outputs` contains all node results (equivalent to exposing everything). If `InputContract == nil`, no input validation runs.

---

## Validation

Decision-logic validation runs inside `NewDecisionTask` — see the constructor's documentation. Task-level validation runs later, when the task is incorporated into a process graph.

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
- **Inputs** — a table listing each `InputContract` field name, its `Bl` type, and required/optional status.
- **Outputs** — a table listing each `OutputContract` field name, its `Bl` type, and required/optional status.
- **Nodes** — each node rendered via its own `ToMarkdown()`, in dependency order, separated by horizontal rules.
- **Dependencies** — a summary listing each node and the upstream nodes whose outputs it references in its expression trees.

To render a process's overall structure including instantiated decision tasks, use `Process.ToMarkdown()` (see [../processes/process.spec.md](../processes/process.spec.md)).

---

## Edge Cases

- A `DecisionTask` whose `Id`, `Name`, `InputMappings`, or `OutputMappings` is empty/nil cannot be added to a process graph. `NewProcess()` raises `ProcessDefinitionError`.
- An empty `NewVariableMapping()` is a valid `InputMappings` or `OutputMappings` value — it explicitly declares "no variables flow in/out." A nil `*VariableMapping` is rejected; use the empty constructor instead.
- Calling `Clone` repeatedly on the same template produces independent instances. Mutations on one (in opts at clone time) do not propagate to others or to the source.
- `Clone` resets task-level fields — it does not inherit them from the source. Specify every task-level field you want on the clone via opts.
- Decision-logic fields (`DecisionGraph`, `InputContract`, `OutputContract`, `Description`) are shared by reference between a source and its clones. Because `DecisionTask` is immutable after `NewDecisionTask`, this sharing has no observable effect beyond memory layout.
- `Evaluate(input)` is callable on a template or a clone and produces identical results — task-level framing is not exercised by the standalone path.
- A template with a single node and no contracts is valid and can be cloned.
- `Evaluate(input)` with an input variable not referenced by any node is silently ignored.
- A missing required input variable is resolved as `BlNull` — but `InputContract` validation runs first and may reject it before evaluation begins.
- Invalid decision graphs (cycles among nodes, duplicate node ids, duplicate output names, empty node ids) are rejected by `NewDecisionTask` with a `DecisionDefinitionError`; a `*DecisionTask` value therefore always holds a valid graph. An output handle from outside the graph (referencing a node that is not in `DecisionGraph`) is also a `DecisionDefinitionError`.
- If a node faults during evaluation, the error propagates — downstream dependents also fault. Nodes not in the dependency chain of the faulted node are still evaluated.
- A `DecisionTask` with no nodes evaluates successfully, returning an empty result.
- A task with `InputContract == nil` accepts any input variables without type checking.
- A task with `OutputContract == nil` exposes all node results in `Outputs`.
- A clone with `Loop` and `MultiInstance` both set produces a `ProcessDefinitionError`.
- Exit ports are configured **only** via `opts.ExitPorts`. The inherited `Task.AddInterruptingWaitForDuration(...)` / `AddErrorExitPort(...)` / `AddInterruptingConditional(...)` etc. methods do not apply to `DecisionTask`. Use the standalone constructors (`NewInterruptingTimerExitPort`, `NewErrorExitPort`, etc. — see [../processes/task-nodes.spec.md](../processes/task-nodes.spec.md)) to build exit ports for `opts.ExitPorts`.
- Exit-port flow targets are wired via `task.ExitPort(id).To(...)` in the graph block, as for any other Task. This is graph-construction wiring, not task mutation.
- Exit-port ids must be unique within a single `DecisionTask` instance. Ids are not shared across clones — two clones of the same template can each have an exit port called `"sla"`.
