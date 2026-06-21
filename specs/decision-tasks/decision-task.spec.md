---
name: DecisionTask
description: A reusable decision task — a graph of DecisionNodes with BlSchema-typed InputSchema/OutputSchema that is itself a ProcessNode. The constructor assembles the graph from the nodes and reference data it is given by matching each node's declared input contract to a producer; it is built and cloned via functional options (panicking on invalid definitions). Nodes and InputSchema are mandatory at construction, with Id/Name/InputMapping additionally required at process-graph incorporation time.
targets:
  - ../../core/decision_task.go
---

# DecisionTask

`DecisionTask` is the top-level container for a blkit decision, inspired by the DMN standard. You hand it the decision's [`DecisionNode`](decision-node.spec.md)s (tables and expressions) and any [`ReferenceData`](reference-data.spec.md), and the constructor assembles them into a runnable directed acyclic graph — you never hand-wire the call order. It is itself a `ProcessNode` that can be placed directly in a process graph.

The whole type-safety story is concentrated at construction, and the mental model is one sentence: **if `NewDecisionTask` does not complain, the decision is sound.** It resolves every node's declared inputs against the available producers, checks the types line up, orders the graph, and reports every problem at once (see [§ Validation](#validation)).

A single `DecisionTask` template can be reused across multiple processes by calling `task.Clone(...)`. Each clone is an independent `*DecisionTask` whose decision-logic fields are shared by reference with the source and whose task-level fields come **only from the options passed to `Clone`** — the source's task-level fields are reset, not inherited.

```go
type DecisionTask struct {
    // Decision-logic fields — set via With… options at creation; treated as
    // immutable by convention thereafter. Shared by reference with clones.
    Description   string
    InputSchema   BlSchema         // required (set at NewDecisionTask). Validated via ValidateInput (closed)
    OutputSchema  BlSchema         // optional; nil = absent. Validated via ValidateOutput (permissive); also drives output projection
    DecisionGraph DecisionGraph    // processed, sorted graph (built by NewDecisionTask)
    ReferenceData []ReferenceValue // static value sources, bound by Id into the eval context (see reference-data.spec.md)

    // Task-level fields — set via With… options at creation; treated as
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

// DecisionGraph is the processed, structured representation of the node graph:
// validated (every node input resolves to exactly one producer of matching
// type; no cycles; no duplicate node ids; output names unique across the task)
// and topologically sorted in evaluation order. Built by NewDecisionTask from
// the nodes supplied via WithNodes and the reference data via WithReferenceData.
type DecisionGraph struct {
    DecisionNodes []DecisionNode // in topologically-sorted evaluation order
}

// --- Shared identity options (package-wide) ---
//
// identity carries the Id / Name / Description common to every decisions
// construct. Each construct's config embeds it, so one set of identity options
// serves NewDecisionTask, NewReferenceData, and any future construct.
type identity struct{ id, name, description string }

func (i *identity) ident() *identity { return i }

// hasIdentity is satisfied by any config that embeds identity.
type hasIdentity interface{ ident() *identity }

// WithId / WithName / WithDescription are defined once for the whole package,
// generic over the concrete config. Go infers the type parameter from the option
// slot (reverse inference), so call sites read bl.WithId("…").
func WithId[C hasIdentity](id string) func(C)
func WithName[C hasIdentity](name string) func(C)
func WithDescription[C hasIdentity](description string) func(C)

// decisionTaskConfig is the unexported staging struct the With… options write
// into. NewDecisionTask / Clone fill it from the supplied options, validate it,
// then build the *DecisionTask. Callers never touch it — they configure
// exclusively through the With… constructors.
type decisionTaskConfig struct {
    identity // id, name, description

    // Decision-logic fields (shared by reference with clones)
    nodes         []DecisionNode
    inputSchema   BlSchema
    outputSchema  BlSchema
    referenceData []ReferenceValue

    // Task-level fields (reset on Clone). InputMappings (and Id / Name from
    // identity) are mandatory at incorporation into a process graph; OutputMappings
    // is optional.
    inputMappings                   *VariableMapping
    outputMappings                  *VariableMapping
    loop                            *LoopConfig
    multiInstance                   *MultiInstanceConfig
    maxExecutionsPerProcessInstance int
    exitPorts                       []ExitPort // converted to the keyed map internally
}

// DecisionTaskOption configures a DecisionTask. Options are plain setters that
// write into the staging config; they do not validate. All validation is
// centralized in the constructor — see § Validation.
type DecisionTaskOption func(*decisionTaskConfig)

// Construct a DecisionTask from options. WithNodes (at least one node) and
// WithInputSchema are mandatory. The graph is assembled by matching each node's
// declared input contract to a producer (an upstream node output, a task input,
// or reference data), validated (no cycles, no duplicate node ids, output names
// unique, every input resolves to a matching-typed producer), and the
// topologically-sorted result stored on DecisionTask.DecisionGraph. Definition
// errors are accumulated and the constructor panics once with a
// *DecisionDefinitionError (see § Validation).
func NewDecisionTask(opts ...DecisionTaskOption) *DecisionTask

// DecisionTask-specific option constructors (identity uses the shared
// WithId / WithName / WithDescription above). Each returns a DecisionTaskOption.
//
// Decision-logic options — WithNodes, WithInputSchema, WithOutputSchema,
// WithReferenceData, and the shared WithDescription — are valid only on
// NewDecisionTask; passing any of them to Clone panics with a
// *DecisionDefinitionError (see Clone).
func WithNodes(nodes ...DecisionNode) DecisionTaskOption // the decision-node graph
func WithInputSchema(schema BlSchema) DecisionTaskOption
func WithOutputSchema(schema BlSchema) DecisionTaskOption
func WithReferenceData(refs ...ReferenceValue) DecisionTaskOption // see reference-data.spec.md
func WithInputMapping(m *VariableMapping) DecisionTaskOption
func WithOutputMapping(m *VariableMapping) DecisionTaskOption
func WithLoop(l *LoopConfig) DecisionTaskOption
func WithMultiInstance(mi *MultiInstanceConfig) DecisionTaskOption
func WithMaxExecutionsPerProcessInstance(n int) DecisionTaskOption
func WithExitPorts(ports ...ExitPort) DecisionTaskOption

// Standalone evaluation, bypassing task-level framing (mappings, loop,
// multi-instance, exit ports). Useful for unit-testing the decision logic in
// isolation. Callable on a template or a clone with the same result.
func (d *DecisionTask) Evaluate(input map[string]BlValue) (DecisionResult, error)

// Render the decision logic and structure as a markdown string. Independent
// of task-level state.
func (d *DecisionTask) ToMarkdown() string

// Clone returns a new *DecisionTask. Decision-logic fields (DecisionGraph,
// ReferenceData, InputSchema, OutputSchema, Description) are shared by reference
// with the receiver. Task-level fields are taken **only** from the options — the
// receiver's task-level fields are reset, not inherited. Specify every
// task-level field you want set on the clone via With… options.
//
// The decision-logic options (WithNodes, WithDescription, WithInputSchema,
// WithOutputSchema, WithReferenceData) panic with a *DecisionDefinitionError if
// passed to Clone: a clone always shares the receiver's decision logic, so
// changing a clone's nodes, schemas, or reference data is not expressible.
func (d *DecisionTask) Clone(opts ...DecisionTaskOption) *DecisionTask


// Result of standalone evaluation. All three maps key on the task's single,
// flat value namespace: output names (unique across the task).
type DecisionResult struct {
    // The declared outputs — only the output names listed in OutputSchema (or
    // all produced outputs if OutputSchema is nil).
    Outputs map[string]BlValue

    // Every produced output value, keyed by output name, including
    // intermediaries not exposed by OutputSchema.
    AllResults map[string]BlValue

    // Ids of all nodes that were evaluated, in evaluation order.
    EvaluatedNodes []string
}
```

---

## Design Pattern

- **Constructor function.** A `DecisionTask` is only ever built through
  `NewDecisionTask(opts…)` or `Clone(opts…)`.
  - Every input — mandatory and optional alike — is supplied as a named functional
    option (a `With…` function); there are no positional arguments.
  - `WithNodes` (at least one node) and `WithInputSchema` are mandatory; every other
    option is optional. An invalid construction is caught by runtime validation, which
    panics (see **Constructor validation panics** below).
  - Both return a bare `*DecisionTask` — no `error` — so the package-scope
    `var x = bl.NewDecisionTask(…)` idiom stays clean.
- **Functional options.** Fields are set with `With…` options of type
  `DecisionTaskOption func(*decisionTaskConfig)`.
  - Options are plain setters that write into the staging config and never validate.
  - An option is made generic only when it is **shared across constructs**. The
    identity options — `WithId`, `WithName`, `WithDescription` — are defined once and
    generic over the config (`func WithId[C hasIdentity](id string) func(C)`), so call
    sites read `bl.WithId("…")` via reverse inference.
  - The remaining options are concrete `func(*decisionTaskConfig)` setters.
  - Options are order-independent, but for readability list them in the canonical
    order: `WithId`, `WithName`, `WithDescription`, `WithNodes`, `WithInputSchema`,
    `WithInputMapping`, `WithReferenceData`, `WithOutputSchema`, `WithOutputMapping`.
    Any further options (`WithLoop`, `WithExitPorts`, …) follow.
- **Constructor validation panics.** All validation is centralized in the constructor.
  - It accumulates every problem and panics once with a `*DecisionDefinitionError`
    (validating inside a plain setter would abort at the first error).
  - Because tasks are declared as package-scope `var`s, a bad definition fails loudly at
    package init / `go test`.

---

## Mandatory fields

A `DecisionTask` is validated in two phases, each with its own required fields:

- **At construction** (`NewDecisionTask`) — a missing one is a `DecisionDefinitionError`.
  - `Nodes` — at least one, via `WithNodes`.
  - `InputSchema` — via `WithInputSchema`.
  - Both are decision-logic fields, so they are fixed once and shared by every clone.
- **At process-graph incorporation** (`bl.NewProcess()`) — five fields; a missing one is a `ProcessDefinitionError`.
  - `Nodes` and `InputSchema` — carry over from construction.
  - `Id`, `Name`, and `InputMappings` — task-level fields reset on `Clone`, so each incorporation must supply them.

An empty `bl.NewVariableMapping()` is a valid `InputMappings` — it explicitly declares "no variables flow in." A nil `*VariableMapping` is rejected; use the empty constructor instead.

`OutputSchema`, `OutputMappings`, `Loop`, `MultiInstance`, `MaxExecutionsPerProcessInstance`, and `ExitPorts` remain optional.

---

## Construction

The common case is to construct a `DecisionTask` fully baked in a single step — decision logic and task-level fields together — when it is only used in one place:

```go
var oneShotDecision = bl.NewDecisionTask(
    bl.WithId("decide"),
    bl.WithName("Decide"),
    bl.WithNodes(eligibility, approval),
    bl.WithInputSchema(loanInputSchema),
    bl.WithInputMapping(im),
    bl.WithReferenceData(minIncome),
    bl.WithOutputSchema(loanOutputSchema),
    bl.WithOutputMapping(om),
)
```

(`eligibility` and `approval` are package-scope `var` declarations — see **Building the Decision Logic** below.)

### Reuse via Clone

To reuse the same decision logic across multiple processes, build the decision logic once as a template (no task-level fields), then `Clone` for each use:

```go
// Build the template — decision logic only.
var template = bl.NewDecisionTask(
    bl.WithDescription("Approves or denies a loan based on eligibility and amount"),
    bl.WithNodes(eligibility, approval),
    bl.WithInputSchema(loanInputSchema),
    bl.WithReferenceData(minIncome),
    bl.WithOutputSchema(loanOutputSchema),
)

// Use in process A — Id, Name, InputMapping all required (Nodes + InputSchema come from the template)
var riskCheckA = template.Clone(
    bl.WithId("risk-check"),
    bl.WithName("Risk Assessment"),
    bl.WithInputMapping(bl.NewVariableMapping(
        [2]string{"start.age",         "age"},
        [2]string{"start.income",      "income"},
        [2]string{"start.loan_amount", "loan_amount"},
    )),
    bl.WithOutputMapping(bl.NewVariableMapping(
        [2]string{"status", "risk-check.status"},
    )),
)

// Use in process B with different mappings, a Loop, and an SLA exit port
var slaTimer = bl.NewInterruptingTimerExitPort("sla", bl.DaysTimeDuration("PT1M"))

var riskEvalB = template.Clone(
    bl.WithId("risk-eval"),
    bl.WithName("Risk Evaluation"),
    bl.WithInputMapping(bl.NewVariableMapping(
        [2]string{"start.age_v2",      "age"},
        [2]string{"start.income",      "income"},
        [2]string{"start.loan_amount", "loan_amount"},
    )),
    bl.WithOutputMapping(bl.NewVariableMapping(
        [2]string{"status", "risk-eval.status"},
    )),
    bl.WithLoop(bl.NewLoopConfig(
        bl.StringVar("risk-eval.status").Equals(bl.String("indeterminate")),
        3,
    )),
    bl.WithExitPorts(slaTimer),
)

// Process A
var processA = bl.NewProcess("loan-app-a", "1.0", ProcessOpts{
    Graph: []ProcessNode{
        startA.To(riskCheckA).To(approvedA),
    },
})

// Process B — exit-port flow target wired in the graph block as usual
var processB = bl.NewProcess("loan-app-b", "2.0", ProcessOpts{
    Graph: []ProcessNode{
        startB.To(riskEvalB).To(approvedB),
        riskEvalB.ExitPort("sla").To(escalateB).To(escalatedB),
    },
})
```

---

## Building the Decision Logic

Each node is built via its constructor ([`NewDecisionTable`](decision-table.spec.md), [`NewDecisionExpression`](decision-expression.spec.md), [`NewDecisionNativeFunction`](decision-native-fn.spec.md), [`NewSubDecisionTask`](sub-decision-task.spec.md)) as a package-scope `var`. Every node declares its input contract (what it consumes) and output contract (what it produces) as plain `[]bl.Field` data; `NewDecisionTask` reads those contracts to wire the graph.

```go
// The task's external input contract — what callers must supply.
var loanInputSchema, _ = bl.Schema(
    bl.Field{Name: "age", Type: bl.TypeNumber},
    bl.Field{Name: "income", Type: bl.TypeNumber},
    bl.Field{Name: "loan_amount", Type: bl.TypeNumber},
)

// The task's output contract — what the decision exposes.
var loanOutputSchema, _ = bl.Schema(
    bl.Field{Name: "status", Type: bl.TypeString},
)

// Reference-data constant: the minimum qualifying income, referenced by its Id
// "min_income" (the Go name minIncome is irrelevant inside expressions — see
// reference-data.spec.md).
var minIncome = bl.NewReferenceData(bl.Number(30000),
    bl.WithId("min_income"),
    bl.WithName("Minimum Income"),
)

// A table: consumes age, income, and the min_income reference data; produces
// eligibility.
var eligibility = bl.NewDecisionTable(bl.DecisionTableOpts{
    Id:        "eligibility",
    Name:      "Eligibility Check",
    HitPolicy: bl.HitPolicyUnique,
    Inputs: []bl.Field{
        {Name: "age", Type: bl.TypeNumber},
        {Name: "income", Type: bl.TypeNumber},
        {Name: "min_income", Type: bl.TypeNumber}, // reference data, by Id
    },
    Outputs: []bl.Field{
        {Name: "eligibility", Type: bl.TypeString},
    },
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
var approval = bl.NewDecisionExpression(bl.DecisionExpressionConfig{
    Id:   "approval",
    Name: "Loan Approval",
    Inputs: []bl.Field{
        {Name: "eligibility", Type: bl.TypeString},
    },
    Outputs: []bl.Field{
        {Name: "status", Type: bl.TypeString},
    },
    Entries: bl.Entries{
        "status": `if eligibility = "eligible" then "approved" else "denied"`,
    },
})

var loanApproval = bl.NewDecisionTask(
    bl.WithDescription("Approves or denies a loan based on eligibility and amount"),
    bl.WithNodes(eligibility, approval),
    bl.WithInputSchema(loanInputSchema),
    bl.WithReferenceData(minIncome),
    bl.WithOutputSchema(loanOutputSchema),
)

// Standalone evaluation works on the template — no Clone needed.
var result, err = loanApproval.Evaluate(map[string]bl.BlValue{
    "age":         bl.Number(30),
    "income":      bl.Number(50000),
    "loan_amount": bl.Number(200000),
})

// result.Outputs        map[string]bl.BlValue{"status": "approved"}
// result.AllResults     map[string]bl.BlValue{"eligibility": "eligible", "status": "approved"}
// result.EvaluatedNodes []string{"eligibility", "approval"}
```

`NewDecisionTask` wires the graph from the contracts: `approval` declares an input `eligibility`, which matches the `eligibility` table's output of the same name and type, so an `eligibility → approval` edge is drawn. The table's `age`/`income` inputs match task inputs; its `min_income` input matches the registered reference data. `loan_amount` is a task input consumed by no node — that is allowed and simply ignored.

### Including reference data

Static constants — `ReferenceData` value sources (see [reference-data.spec.md](reference-data.spec.md)) — are added with `bl.WithReferenceData` and consumed by a node that declares an input whose **name is the reference data's `Id`**. They are not `DecisionNode`s, so they never appear in the `[]bl.DecisionNode` list; the task binds each value into the evaluation context under its `Id`. Because reference data is decision-logic, clones inherit it by reference (it is never re-added).

Pass several at once — `bl.WithReferenceData(taxRate, baseCurrency, shippingRates)` — or call it more than once; the sources accumulate.

---

## Evaluation

### Standalone

`Evaluate(input)` runs the decision logic directly, without task-level framing (mappings, loop, multi-instance, exit ports). It validates `input` against `InputSchema` (via `ValidateInput`), runs the algorithm below, projects results by `OutputSchema`, and returns a `DecisionResult`.

### In a process

When the runtime reaches an instantiated `DecisionTask` node:

1. Apply `InputMappings` to the current `ExecutionContext` to compute the decision's `input` map.
2. Validate `input` against `InputSchema`, via `ValidateInput`. Failure produces a `DataContractValidationError`.
3. Run the algorithm below against `input`.
4. Project results by `OutputSchema` (if set), via `ValidateOutput`.
5. Apply `OutputMappings` to map result fields into target variable names.
6. Merge the mapped result into the `ExecutionContext` under the task's `Id`.

### Internal evaluation algorithm

The task threads one shared, name-keyed context. When `Evaluate(input)` runs:

1. Seed the context with the caller-provided `input` variables and each reference data value, bound under its `Id`.
2. Iterate `DecisionGraph.DecisionNodes` in stored order (already topologically sorted by `NewDecisionTask`).
3. Call each node's `Evaluate(context)` — it reads the inputs its contract declares and returns a `map[string]BlValue` keyed by its output names.
4. Merge that result into the shared context under those output names. Because output names are unique across the task, a downstream node consumes an upstream output simply by declaring an input of that name.
5. All nodes are evaluated, regardless of whether their outputs appear in the output contract.
6. The `DecisionResult` carries every produced output in `AllResults`, with `Outputs` projected to the names declared in `OutputSchema`.

### Input Resolution

- `InputSchema` declares the expected input variable names and types.
- Each node's dependencies are resolved **at construction** by matching its declared `Inputs()` against the available producers — upstream node outputs, task inputs, or reference data — by name and type. An input matched to another node produces a node-to-node edge; an input matched to a task input is read from the caller-provided `input` map at evaluation time.
- At evaluation time, input values are validated against `InputSchema` (via `ValidateInput`). A mismatch produces a `DataContractValidationError`.
- Reference data registered via `WithReferenceData` is bound into the context under each source's `Id` before nodes run. Consuming reference data forms no node-to-node edge; the source is a constant leaf (see [reference-data.spec.md](reference-data.spec.md)).

---

## Wiring

`NewDecisionTask` builds the graph purely from the nodes' declared contracts — **there is no expression parsing for wiring.** For each node, for each field in its `Inputs()`, the constructor finds the matching producer:

1. an **upstream node output** of the same name, else
2. a **task input** (`InputSchema` field) of the same name, else
3. a **reference data** value source whose `Id` is that name.

The producer's declared type must equal the input's declared type. A match against an upstream node draws a producer→consumer edge; the constructor topologically sorts the resulting graph. An input that matches no producer, matches more than one, or matches one of a different type is a `DecisionDefinitionError`.

The task's value namespace — task-input names, all node output names, and reference-data `Id`s — is flat and must be mutually unique, so every name resolves unambiguously.

---

## Input and Output Schemas

`InputSchema` and `OutputSchema` are both `bl.BlSchema` values (see [schema.spec.md](../expressions/schema.spec.md)). Direction is expressed by which field a schema occupies rather than by distinct Go types. `InputSchema` is **required**; `OutputSchema` is optional.

- **`InputSchema`** — declares the named, typed variables the decision expects from callers. Validated at evaluation entry via `ValidateInput` (closed: undeclared input variables are rejected).
- **`OutputSchema`** — declares which produced output names are exposed in `DecisionResult.Outputs` and their expected types, validated via `ValidateOutput` (permissive). Each field name must match a node **output name**; this set drives output projection. All nodes are still evaluated (intermediary outputs may feed exposed ones), but only the declared names appear in `Outputs`. Every produced output is available in `AllResults`.

If `OutputSchema == nil`, `Outputs` contains every produced output.

---

## Validation

### Definition validation (in `NewDecisionTask` / `Clone`)

`NewDecisionTask` and `Clone` validate the definition, accumulate **every** problem, and panic once with a `*DecisionDefinitionError`. The `With…` options are plain setters and do not validate individually. Each node was already validated at its own construction (contracts well-formed, expressions compiled, every expression name a declared input or sibling output — see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)); the task's job is the cross-node checks:

- Mandatory inputs are present: `WithNodes` supplies at least one node, and `WithInputSchema` is set.
- Wiring: every node input resolves to exactly one producer (upstream output, task input, or reference data) **of matching declared type**; no unresolved names; no ambiguous matches.
- Graph validity: no cycles, no duplicate node ids, every node has a non-empty id.
- Namespace: task-input names, all node output names, and reference-data `Id`s are mutually unique.
- `OutputSchema` field names each match a node output name; their declared types are compatible.
- `MaxExecutionsPerProcessInstance >= 0`.
- Exit-port ids are unique within the task.
- `Loop` and `MultiInstance` are not both set.

```go
type DecisionDefinitionError struct {
    TaskId   string
    Problems []string // one actionable message per violation
}
```

Because tasks are typically declared as package-scope `var`s, a bad definition panics during package initialisation; the panic value lists each problem and the stack trace pins the offending declaration. `bl.Schema(...)` is independently fallible (`(BlSchema, error)`), so a malformed `InputSchema`/`OutputSchema` surfaces there, before it reaches the constructor.

What definition validation does **not** check is whether a node's expression, when run, produces a value of its declared output type — the expression engine is runtime-typed, so a value-versus-declaration mismatch surfaces as a `bl.TypeError` during evaluation, not here (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)).

### Task-level validation (at process construction)

When `bl.NewProcess()` walks the graph and encounters a `DecisionTask`, it checks the incorporation-mandatory fields and mapping references:

- `Id` is non-empty.
- `Name` is non-empty.
- The decision graph has at least one node and `InputSchema` is set (both guaranteed by construction, re-asserted here).
- `InputMappings` is non-nil.
- Mappings reference variables that exist in the surrounding process input or upstream task outputs.

A violation produces a `ProcessDefinitionError`.

---

## Markdown Rendering

`ToMarkdown()` returns a complete markdown document representing the decision template — its logic, contracts, and node structure. It is independent of task-level state; called on a template or a clone produces identical output.

### Format

- **Title** — the task's `Name` (or `Id`) as a level-1 heading.
- **Description** — the task's `Description`, if set.
- **Inputs** — a table listing each `InputSchema` field name, its type, and required/optional status.
- **Outputs** — a table listing each `OutputSchema` field name, its type, and required/optional status.
- **Nodes** — each node rendered via its own `ToMarkdown()`, in dependency order, separated by horizontal rules.
- **Dependencies** — a summary listing each node and the producers its declared inputs resolve to.

To render a process's overall structure including instantiated decision tasks, use `Process.ToMarkdown()` (see [../processes/process.spec.md](../processes/process.spec.md)).

---

## Edge Cases

- A `DecisionTask` whose `Id`, `Name`, or `InputMappings` is empty/nil cannot be added to a process graph. `bl.NewProcess()` raises `ProcessDefinitionError`.
- An empty `bl.NewVariableMapping()` is a valid `InputMappings`. A nil `InputMappings` is rejected; use the empty constructor instead. `OutputMappings` may be left nil.
- Calling `Clone` repeatedly on the same template produces independent instances. Options applied to one clone do not propagate to others or to the source.
- `Clone` resets task-level fields — it does not inherit them. Specify every task-level field you want on the clone via `With…` options.
- Decision-logic fields (`DecisionGraph`, `ReferenceData`, `InputSchema`, `OutputSchema`, `Description`) are shared by reference between a source and its clones. Because `DecisionTask` is immutable after `NewDecisionTask`, this sharing has no observable effect beyond memory layout.
- Passing a decision-logic option (`WithDescription`, `WithInputSchema`, `WithOutputSchema`, `WithReferenceData`) to `Clone` is a `DecisionDefinitionError`.
- `Evaluate(input)` is callable on a template or a clone and produces identical results.
- A template with a single node and an `InputSchema` (no `OutputSchema`) is valid and can be cloned.
- An `InputSchema` variable consumed by no node is silently ignored.
- A missing required input variable is resolved as `bl.BlNull` — but `InputSchema` validation runs first and may reject it before evaluation begins.
- Invalid graphs (cycles, duplicate node ids, empty node ids, an input resolving to no producer or to a producer of the wrong type) are rejected by `NewDecisionTask` with a `DecisionDefinitionError`; a `*DecisionTask` value therefore always holds a valid graph.
- If a node faults during evaluation, the error propagates — downstream dependents also fault. Nodes not in the dependency chain of the faulted node are still evaluated.
- A task with `OutputSchema == nil` exposes all produced outputs in `Outputs`.
- A clone with `Loop` and `MultiInstance` both set produces a `ProcessDefinitionError`.
- Exit ports are configured **only** via the `WithExitPorts(...)` option. The inherited `Task.AddInterruptingWaitForDuration(...)` / `AddErrorExitPort(...)` / `AddInterruptingConditional(...)` methods do not apply to `DecisionTask`. Use the standalone constructors (`NewInterruptingTimerExitPort`, `NewErrorExitPort`, etc. — see [../processes/task-nodes.spec.md](../processes/task-nodes.spec.md)) to build the exit ports you pass to `WithExitPorts(...)`.
- Exit-port flow targets are wired via `task.ExitPort(id).To(...)` in the graph block, as for any other Task.
- Exit-port ids must be unique within a single `DecisionTask` instance. Ids are not shared across clones — two clones of the same template can each have an exit port called `"sla"`.
