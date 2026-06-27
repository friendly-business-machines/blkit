---
name: DecisionTask
description: A reusable decision task — a graph of generic DecisionNode[I,O]s wired into a compile-checked netlist over a typed TaskIn/TaskOut, that is itself a ProcessNode (and itself a DecisionNode). Built from a DecisionTaskConfig (identity + task-level fields) plus a task.Graph(...) of bl.Edge connections; the node set and reference data are derived from the edges. The constructor topologically sorts the edges, rejects cycles, and panics on invalid definitions. Cloned for reuse across processes.
targets:
  - ../../core/decision_task.go
---

# DecisionTask

`DecisionTask[TaskIn, TaskOut]` is the top-level container for a blkit decision, inspired by the DMN standard. It is generic over two concrete Go structs — an external **input** struct `TaskIn` and **output** struct `TaskOut`, each a struct of [`bl.Handle`](decision-node.spec.md#handlet--the-typed-io-field) fields. You build the task from a config, then wire it with `task.Graph(...)` — a list of `bl.Edge` connections between the nodes' handles and the task's own `In`/`Out`. The constructor assembles them into a runnable directed acyclic graph. A `DecisionTask` is itself a `ProcessNode` that can be placed directly in a process graph — and itself a `DecisionNode[TaskIn, TaskOut]`, so a whole decision composes into a larger one (see [§ A DecisionTask is a node](#a-decisiontask-is-a-node)).

The whole type-safety story is split between the Go compiler and construction, and the mental model is one sentence: **if it compiles and `Graph` does not complain, the decision is sound.** Every connection is a typed `bl.Edge` the compiler checks (see [§ Wiring](#wiring)); `Graph` then derives the node set and reference data from the edges, orders the graph, rejects cycles, and reports every remaining problem at once (see [§ Validation](#validation)).

A single `DecisionTask` template can be reused across multiple processes by calling `task.Clone(...)`. Each clone is an independent `*DecisionTask` whose decision logic (its graph) is shared by reference with the source and whose task-level fields come **only from the clone config** — the source's task-level fields are reset, not inherited.

```go
// NewDecisionTask builds the task from its config and stamps the In/Out boundary
// handle surfaces (reflected from TaskIn/TaskOut). The returned task is not yet
// wired — call task.Graph(...) to supply the edges.
func NewDecisionTask[TaskIn, TaskOut any](config DecisionTaskConfig) *DecisionTask[TaskIn, TaskOut]

type DecisionTaskConfig struct {
    // Identity.
    Id          string
    Name        string
    Description string

    // Task-level fields (process incorporation; reset on Clone). Mandatory at
    // incorporation: Id, Name, and InputMappings.
    InputMappings                   *VariableMapping
    OutputMappings                  *VariableMapping
    Loop                            *LoopConfig
    MultiInstance                   *MultiInstanceConfig
    MaxExecutionsPerProcessInstance int
    ExitPorts                       []ExitPort
}

// In / Out are the task's boundary handle surfaces, stamped by NewDecisionTask
// from TaskIn / TaskOut. Reference them in Graph to wire task inputs into nodes
// and node outputs into the task output; they also let a DecisionTask be wired as
// a child node. Shared by reference with clones.
//   d.In  TaskIn
//   d.Out TaskOut

// Graph supplies the wiring and finalises the task: it derives the node set and
// reference data from the edges, topologically sorts them, rejects cycles, checks
// every node input is wired exactly once and every TaskOut field is produced,
// accumulates every problem, and panics once with a *DecisionDefinitionError. It
// returns the same task. See § Wiring.
func (d *DecisionTask[TaskIn, TaskOut]) Graph(edges ...Edge) *DecisionTask[TaskIn, TaskOut]

// Edge connects a source handle (a task input d.In.Field, a node output
// node.Out.Field, or reference data refData.Value) to a destination handle (a node
// input node.In.Field or the task output d.Out.Field). T must match across the two
// handles, so a mis-typed or non-existent-field connection is a Go compile error.
// It is a free function because Go methods cannot be generic.
func Edge[T BlValue](src, dst Handle[T]) Edge

// DecisionNode[TaskIn, TaskOut] interface satisfaction — a DecisionTask is a node.
func (d *DecisionTask[TaskIn, TaskOut]) GetId() string
func (d *DecisionTask[TaskIn, TaskOut]) GetName() string
func (d *DecisionTask[TaskIn, TaskOut]) GetDescription() string
func (d *DecisionTask[TaskIn, TaskOut]) Inputs() []Field  // reflected from TaskIn
func (d *DecisionTask[TaskIn, TaskOut]) Outputs() []Field // reflected from TaskOut

// Standalone evaluation, bypassing task-level framing (mappings, loop,
// multi-instance, exit ports). Useful for unit-testing the decision logic in
// isolation. Callable on a template or a clone with the same result.
func (d *DecisionTask[TaskIn, TaskOut]) Evaluate(in TaskIn) (TaskOut, error)

// Render the decision logic and structure as a markdown string.
func (d *DecisionTask[TaskIn, TaskOut]) ToMarkdown() string

// Clone returns a new *DecisionTask sharing the receiver's graph (and its derived
// nodes, reference data, and In/Out boundary) by reference, with task-level fields
// taken only from the clone config. Calling Graph on a clone is a
// *DecisionDefinitionError — a clone always shares the receiver's wiring.
func (d *DecisionTask[TaskIn, TaskOut]) Clone(config DecisionTaskConfig) *DecisionTask[TaskIn, TaskOut]

type DecisionDefinitionError struct {
    TaskId   string
    Problems []string // one actionable message per violation
}
```

`InputSchema()` and `OutputSchema()` (used at the process boundary and for markdown) are **reflected** from `TaskIn`/`TaskOut`; there is no separate schema config. There is no `Nodes` or `ReferenceData` field either — the node set and the reference-data sources are **discovered from the edges** (every handle carries its owning node or source), so the graph is the single place each object appears.

---

## Wiring

`task.Graph(...)` takes a list of `bl.Edge` connections. Each `bl.Edge(src, dst)` connects a **source** handle to a **destination** handle:

- a **source** is a task input (`task.In.Field`), a node output (`node.Out.Field`), or a [reference data](reference-data.spec.md) value (`refData.Value`);
- a **destination** is a node input (`node.In.Field`) or the task output (`task.Out.Field`).

Because `bl.Edge[T any](src, dst Handle[T])` requires `T` to unify across the two handles, **the netlist is type-checked at `go build`**: connecting a `Handle[BlString]` to a `Handle[BlNumber]`, or naming a field a node does not have, is a Go compile error — not a construction error (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)).

The set of edges **is** the graph, and the single source of truth: each handle carries a reference to its owning node (or reference-data source), so `Graph` **derives the node set and the reference data from the connections themselves** — nothing is listed twice. It then draws the producer→consumer dependency edges and **topologically sorts** them. There is no name matching: two nodes may produce outputs of the same `expr` name without colliding, because an edge connects specific handles, not names.

`Graph` checks:

- **Every node input is wired exactly once** — a node input with no incoming edge, or more than one, is a `DecisionDefinitionError`.
- **Every `TaskOut` field is produced** — an output boundary handle with no incoming edge is a `DecisionDefinitionError`.
- **At least one node** is reachable from the edges.
- **Node ids are unique** and non-empty (handles are stamped with node id; duplicates make handles ambiguous).
- **The graph is acyclic** — a cycle in the connections is a `DecisionDefinitionError`.

A task input wired to no node (e.g. a `loan_amount` the rules never consult) is allowed and simply ignored.

---

## Mandatory steps

A `DecisionTask` is finalised in two phases:

- **At construction** — `NewDecisionTask(config)` followed by `task.Graph(...)`. The `Graph` call is mandatory: it must wire every node input and every `TaskOut` field, with at least one node (see [§ Wiring](#wiring)). A missing or malformed wiring is a `DecisionDefinitionError`. The graph is decision logic, fixed once and shared by every clone.
- **At process-graph incorporation** (`bl.NewProcess()`) — three task-level fields; a missing one is a `ProcessDefinitionError`.
  - `Id`, `Name`, and `InputMappings` — task-level fields reset on `Clone`, so each incorporation must supply them.

An empty `bl.NewVariableMapping()` is a valid `InputMappings` — it explicitly declares "no variables flow in." A nil `*VariableMapping` is rejected; use the empty constructor instead. `OutputMappings`, `Loop`, `MultiInstance`, `MaxExecutionsPerProcessInstance`, and `ExitPorts` remain optional.

---

## Building the Decision Logic

Each node is built via its constructor ([`NewDecisionTable`](decision-table.spec.md), [`NewDecisionExpression`](decision-expression.spec.md), [`NewDecisionNativeFunction`](decision-native-fn.spec.md)) as a package-scope `var`, with its input/output structs of `bl.Handle` fields. The task's `TaskIn`/`TaskOut` are likewise structs of handles; after `NewDecisionTask` the task exposes them as `task.In`/`task.Out`, which the graph wires.

```go
// The task's external contracts.
type LoanInputs struct {
    Age        bl.Handle[bl.BlNumber] `expr:"age"`
    Income     bl.Handle[bl.BlNumber] `expr:"income"`
    LoanAmount bl.Handle[bl.BlNumber] `expr:"loan_amount"`
}
type LoanOutputs struct {
    Status bl.Handle[bl.BlString] `expr:"status"`
}

// Reference-data constant: the minimum qualifying income (see reference-data.spec.md).
var minIncome = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlNumber]{
    Id:    "min_income",
    Name:  "Minimum Income",
    Value: bl.Number(30000),
})

// A table: consumes age, income, and the min_income constant; produces eligibility.
type EligibilityInputs struct {
    Age       bl.Handle[bl.BlNumber] `expr:"age"`
    Income    bl.Handle[bl.BlNumber] `expr:"income"`
    MinIncome bl.Handle[bl.BlNumber] `expr:"min_income"`
}
type EligibilityOutputs struct {
    Eligibility bl.Handle[bl.BlString] `expr:"eligibility"`
}
var eligibility = bl.NewDecisionTable[EligibilityInputs, EligibilityOutputs](bl.DecisionTableConfig{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: bl.HitPolicyUnique,
    Columns: []bl.Column{
        {Label: "Age", Expr: `age`, Type: bl.TypeNumber},
        {Label: "Income", Expr: `income`, Type: bl.TypeNumber},
    },
    Rules: bl.Rules{
        {`>= 18`, `>= min_income`, `"eligible"`},
        {`< 18`, `-`, `"ineligible"`},
        {`-`, `< min_income`, `"ineligible"`},
    },
})

// An expression: consumes the table's eligibility output; produces status.
type ApprovalInputs struct {
    Eligibility bl.Handle[bl.BlString] `expr:"eligibility"`
}
type ApprovalOutputs struct {
    Status bl.Handle[bl.BlString] `expr:"status"`
}
var approval = bl.NewDecisionExpression[ApprovalInputs, ApprovalOutputs](bl.DecisionExpressionConfig{
    Id:   "approval",
    Name: "Loan Approval",
    Entries: bl.Entries{
        "status": `if eligibility = "eligible" then "approved" else "denied"`,
    },
})

// The task: build it, then wire it. Nodes (eligibility, approval) and reference
// data (minIncome) are discovered from the edges — nothing is listed separately.
var loanApproval = bl.NewDecisionTask[LoanInputs, LoanOutputs](bl.DecisionTaskConfig{
    Description: "Approves or denies a loan based on eligibility",
})

var _ = loanApproval.Graph(
    bl.Edge(loanApproval.In.Age,         eligibility.In.Age),
    bl.Edge(loanApproval.In.Income,      eligibility.In.Income),
    bl.Edge(minIncome.Value,             eligibility.In.MinIncome),
    bl.Edge(eligibility.Out.Eligibility, approval.In.Eligibility),
    bl.Edge(approval.Out.Status,         loanApproval.Out.Status),
)

// Standalone evaluation works on the template — no Clone needed.
var age, _    = bl.Number(30)
var income, _ = bl.Number(50000)
var amount, _ = bl.Number(200000)
var result, err = loanApproval.Evaluate(LoanInputs{
    Age:        bl.NewHandle(age),
    Income:     bl.NewHandle(income),
    LoanAmount: bl.NewHandle(amount),
})
// result.Status.Get() == bl.String("approved")
```

The graph draws the edges: `loanApproval.In.Age`/`In.Income` feed the table; `minIncome.Value` feeds its `min_income` input; the table's `eligibility` output feeds `approval`; `approval`'s `status` feeds the task output. `loan_amount` is a task input consumed by no node — allowed and simply ignored. The `var _ = loanApproval.Graph(...)` statement runs at package initialisation, so a malformed graph panics at startup (load-time fail-fast).

### Including reference data

A [`ReferenceData`](reference-data.spec.md) constant exposes a single `.Value` handle, so it is wired exactly like a node output: `bl.Edge(minIncome.Value, eligibility.In.MinIncome)`. It is not a `DecisionNode`; the task discovers it from the edge (the handle carries its owning source) and binds its value before the consuming node runs. Because reference data is part of the wiring, clones share it by reference.

---

## Construction

The common case constructs a `DecisionTask` fully baked in one place — config plus wiring — when it is only used once:

```go
var oneShotDecision = bl.NewDecisionTask[LoanInputs, LoanOutputs](bl.DecisionTaskConfig{
    Id:             "decide",
    Name:           "Decide",
    InputMappings:  im,
    OutputMappings: om,
})

var _ = oneShotDecision.Graph( /* the same edges as above */ )
```

### Reuse via Clone

To reuse the same decision logic across multiple processes, build and wire it once as a template (no task-level fields), then `Clone` for each use. A clone shares the template's graph — and its derived nodes, reference data, and `In`/`Out` boundary — by reference; its task-level fields come only from the clone config. Calling `Graph` on a clone is a `DecisionDefinitionError`.

```go
// Use in process A — Id, Name, InputMappings all required.
var riskCheckA = loanApproval.Clone(bl.DecisionTaskConfig{
    Id:   "risk-check",
    Name: "Risk Assessment",
    InputMappings: bl.NewVariableMapping(
        [2]string{"start.age",         "age"},
        [2]string{"start.income",      "income"},
        [2]string{"start.loan_amount", "loan_amount"},
    ),
    OutputMappings: bl.NewVariableMapping(
        [2]string{"status", "risk-check.status"},
    ),
})

// Use in process B with different mappings, a Loop, and an SLA exit port.
var slaTimer = bl.NewInterruptingTimerExitPort("sla", bl.DaysTimeDuration("PT1M"))
var riskEvalB = loanApproval.Clone(bl.DecisionTaskConfig{
    Id:   "risk-eval",
    Name: "Risk Evaluation",
    InputMappings: bl.NewVariableMapping(
        [2]string{"start.age_v2",      "age"},
        [2]string{"start.income",      "income"},
        [2]string{"start.loan_amount", "loan_amount"},
    ),
    OutputMappings: bl.NewVariableMapping(
        [2]string{"status", "risk-eval.status"},
    ),
    Loop: bl.NewLoopConfig(
        bl.StringVar("risk-eval.status").Equals(bl.String("indeterminate")),
        3,
    ),
    ExitPorts: []bl.ExitPort{slaTimer},
})
```

The clones are placed in their process graphs, and exit-port flow targets are wired in the graph block as for any other Task (`riskEvalB.ExitPort("sla").To(escalateB)…`).

---

## Evaluation

### Standalone

`Evaluate(in)` runs the decision logic directly, without task-level framing (mappings, loop, multi-instance, exit ports). It seeds the input boundary handles from `in`, runs the algorithm below, and returns the typed `TaskOut`.

### In a process

When the runtime reaches an instantiated `DecisionTask` node:

1. Apply `InputMappings` to the current `ExecutionContext` to compute the decision's input values by name.
2. Build `TaskIn` from those values — each value bound into the handle whose `expr` name (or field name) it matches; a missing or mistyped value is a `DataContractValidationError` (this is the name-keyed boundary where the reflected `InputSchema()` is validated).
3. Run the algorithm below against `TaskIn`.
4. Reflect the produced `TaskOut` back into named values.
5. Apply `OutputMappings` to map those into target variable names.
6. Merge the mapped result into the `ExecutionContext` under the task's `Id`.

### Internal evaluation algorithm

The task threads one shared, handle-keyed value environment. When `Evaluate(in)` runs:

1. Seed the environment with the input boundary handles (from `in`) and each reference-data value.
2. Iterate the nodes in stored order (already topologically sorted by `Graph`).
3. For each node: read its input handles from the environment (each populated by an earlier edge), run the node — through its typed `Evaluate`, driven internally (synchronously, unless the node is concurrent — see below) — and write its output handles back into the environment along their outgoing edges.
4. After every node has run, read the output boundary handles into a fresh `TaskOut` and return it.

Because the graph is topologically sorted, every handle a node reads is already produced by the time that node runs.

**Concurrent nodes.** A node that reports `Concurrent()` true — a [`DecisionNativeFunction`](decision-native-fn.spec.md#concurrent-execution) configured with `Concurrent: true` — is not awaited in place at step 3. The task captures its already-resolved inputs, runs it in a goroutine, and proceeds to later independent nodes; it joins the goroutine (routing its outputs, or returning its error tagged with the node `Id`) before evaluating the first later node that consumes one of its outputs, and joins any still-running concurrent nodes before step 4. Every join precedes the consumer, so the result is identical to fully sequential evaluation; only the overlap of independent work differs.

---

## Input and Output contracts

`TaskIn` and `TaskOut` are concrete Go structs of `bl.Handle` fields. The reflected `InputSchema()`/`OutputSchema()` (a [`BlSchema`](../expressions/schema.spec.md) view) are derived from them and used at the process boundary and in markdown.

- **`TaskIn`** — the named, typed variables the decision expects from callers. Validated at the process boundary via the reflected schema (closed: an undeclared input variable is rejected).
- **`TaskOut`** — the values the decision exposes. Every field must be produced by exactly one edge; intermediary node outputs that are not wired to `TaskOut` stay internal.

Because the contracts are Go types, a caller passing the wrong input struct, or reading an output field that does not exist, fails `go build`.

---

## A DecisionTask is a node

`DecisionTask[TaskIn, TaskOut]` has a typed `Evaluate(in TaskIn) (TaskOut, error)` and `In`/`Out` boundary handle surfaces, so it **satisfies [`DecisionNode[TaskIn, TaskOut]`](decision-node.spec.md)**. A complete decision therefore composes into a larger one exactly like any other node — wire its `child.In.*` / `child.Out.*` handles in the parent's graph:

```go
// creditModel is a *bl.DecisionTask[CreditInputs, CreditOutputs] declared elsewhere.
var application = bl.NewDecisionTask[AppInputs, AppOutputs](bl.DecisionTaskConfig{
    Id: "application",
})

var _ = application.Graph(
    bl.Edge(application.In.Income,  creditModel.In.Income),
    bl.Edge(application.In.Age,     creditModel.In.Age),
    bl.Edge(creditModel.Out.Score,  decision.In.Score),   // child output → sibling input
    bl.Edge(decision.Out.Verdict,   application.Out.Verdict),
)
```

There is no separate sub-task wrapper type: the name-remapping a wrapper used to provide is simply *which handles you connect*, and it is now compile-checked. The child task runs fresh on each parent evaluation; its intermediary node outputs stay encapsulated, and only its `TaskOut` crosses the boundary.

---

## Validation

### Definition validation (in `task.Graph` / `Clone`)

`Graph` validates the wiring, accumulates **every** problem, and panics once with a `*DecisionDefinitionError`. Each node was already validated at its own construction (contracts well-formed, expressions compiled — see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)); the connection types were already checked by the compiler via `bl.Edge`. `Graph`'s job is the graph-level checks:

- The edges reach at least one node.
- Wiring: every node input is wired exactly once; every `TaskOut` field is produced.
- Graph validity: no cycles; node ids unique and non-empty.
- `MaxExecutionsPerProcessInstance >= 0`; exit-port ids unique within the task; `Loop` and `MultiInstance` not both set (these config checks run at `NewDecisionTask`).
- Calling `Graph` on a clone, or calling it twice, is a violation (a clone shares the receiver's wiring; a task is wired once).

Because tasks are typically package-scope `var`s (and `Graph` is called from a package-scope `var _ = task.Graph(...)`), a bad definition panics during package initialisation; the panic value lists each problem and the stack trace pins the offending declaration.

What definition validation does **not** check is whether a node's expression, when run, produces a value of its declared output type — the expression engine is runtime-typed, so a value-versus-declaration mismatch surfaces as a `bl.TypeError` during evaluation (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)).

### Task-level validation (at process construction)

When `bl.NewProcess()` walks the graph and encounters a `DecisionTask`, it checks the incorporation-mandatory fields and mapping references:

- `Id` and `Name` are non-empty.
- The task is wired (at least one node and a wired `TaskOut`; guaranteed by construction, re-asserted here).
- `InputMappings` is non-nil.
- Mappings reference variables that exist in the surrounding process input or upstream task outputs, and target / source the reflected `TaskIn`/`TaskOut` fields.

A violation produces a `ProcessDefinitionError`.

---

## Markdown Rendering

`ToMarkdown()` returns a complete markdown document representing the decision template — its logic, contracts, and node structure. It is independent of task-level state; called on a template or a clone it produces identical output.

### Format

- **Title** — the task's `Name` (or `Id`) as a level-1 heading.
- **Description** — the task's `Description`, if set.
- **Inputs** — a table of each `TaskIn` field name and type.
- **Outputs** — a table of each `TaskOut` field name and type.
- **Nodes** — each node rendered via its own `ToMarkdown()`, in dependency order, separated by horizontal rules.
- **Graph** — a summary listing each edge (source handle → destination handle).

To render a process's overall structure including instantiated decision tasks, use `Process.ToMarkdown()` (see [../processes/process.spec.md](../processes/process.spec.md)).

---

## Edge Cases

- A `DecisionTask` whose `Id`, `Name`, or `InputMappings` is empty/nil cannot be added to a process graph. `bl.NewProcess()` raises `ProcessDefinitionError`.
- An empty `bl.NewVariableMapping()` is a valid `InputMappings`. A nil `InputMappings` is rejected; use the empty constructor. `OutputMappings` may be left nil.
- A task that is never wired (no `Graph` call) is incomplete; evaluating it or incorporating it into a process is a definition error.
- Calling `Graph` more than once on a task, or on a clone, is a `DecisionDefinitionError`.
- Calling `Clone` repeatedly on the same template produces independent instances; options applied to one clone do not propagate to others or the source.
- `Clone` resets task-level fields — it does not inherit them. A clone shares the receiver's graph (and its derived nodes, reference data, and `In`/`Out` boundary) by reference. Because `DecisionTask` is immutable after wiring, this sharing has no observable effect beyond memory layout.
- `Evaluate(in)` is callable on a template or a clone and produces identical results.
- A template with a single node and a fully-wired `TaskOut` is valid and can be cloned.
- A `TaskIn` field consumed by no node is silently ignored.
- A node input that is wired more than once, or not at all, is a `DecisionDefinitionError`.
- A `TaskOut` field produced by no edge is a `DecisionDefinitionError`.
- A `bl.Edge` connecting handles of different `T`, or naming a field that does not exist, is a **Go compile error** — caught before construction.
- Cyclic graphs, duplicate node ids, and empty node ids are rejected by `Graph` with a `DecisionDefinitionError`; a wired `*DecisionTask` value therefore always holds a valid graph.
- If a node faults during evaluation, the error propagates and the task fails; nodes not in the faulted node's dependency chain may already have run.
- A clone with `Loop` and `MultiInstance` both set produces a `ProcessDefinitionError`.
- Exit ports are configured **only** via the `ExitPorts` config field. The inherited `Task.AddInterruptingWaitForDuration(...)` / `AddErrorExitPort(...)` / `AddInterruptingConditional(...)` methods do not apply to `DecisionTask`. Use the standalone constructors (`NewInterruptingTimerExitPort`, `NewErrorExitPort`, etc. — see [../processes/task-nodes.spec.md](../processes/task-nodes.spec.md)) to build the exit ports you list in `ExitPorts`.
- Exit-port flow targets are wired via `task.ExitPort(id).To(...)` in the graph block, as for any other Task.
- Exit-port ids must be unique within a single `DecisionTask` instance. Ids are not shared across clones — two clones of the same template can each have an exit port called `"sla"`.
