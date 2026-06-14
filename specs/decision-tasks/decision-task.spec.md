---
name: DecisionTask
description: A reusable decision task — directed graph of DecisionNodes with BlSchema-typed InputSchema/OutputSchema that is itself a ProcessNode. Built and cloned via functional options (panicking on invalid definitions); Id/Name/InputMappings/OutputMappings are mandatory at process-graph incorporation time.
targets:
  - ../../core/decision_task.go
---

# DecisionTask

`DecisionTask` is the top-level container for a blkit decision, inspired by the DMN standard. It holds a directed acyclic graph of `DecisionNode`s with typed input and output contracts, and is itself a `ProcessNode` that can be placed directly in a process graph.

A single `DecisionTask` template can be reused across multiple processes by calling `task.Clone(...)`. Each clone is an independent `*DecisionTask` whose decision-logic fields are shared by reference with the source and whose task-level fields come **only from the options passed to `Clone`** — the source's task-level fields are reset, not inherited.

This collapses the prior `DecisionModel` + `DecisionModelTask` two-type split into a single type. Decision logic and task-level metadata live on the same struct; reuse is via `Clone`, not via wrapping.

```go
type DecisionTask struct {
    // Decision-logic fields — set via With… options at creation; treated as
    // immutable by convention thereafter. Shared by reference with clones.
    Description   string
    InputSchema   BlSchema        // optional; nil = absent. Validated via ValidateInput (closed)
    OutputSchema  BlSchema        // optional; nil = absent. Validated via ValidateOutput (permissive); also drives output projection
    DecisionGraph DecisionGraph   // processed, sorted graph (built by NewDecisionTask)
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

// DecisionGraph is the processed, structured representation of a decision-node
// graph: validated (no cycles among nodes — derived from output-handle
// references inside each node's expression trees — no duplicate ids, no
// duplicate output names) and topologically sorted in evaluation order. Built
// by NewDecisionTask from the graph argument.
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
    inputSchema   BlSchema
    outputSchema  BlSchema
    referenceData []ReferenceValue

    // Task-level fields (reset on Clone). InputMappings and OutputMappings (and
    // Id / Name from identity) are mandatory at incorporation into a process graph.
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

// Construct a DecisionTask from the raw decision-node graph plus options. The
// graph is validated (no cycles among nodes — derived from output-handle
// references inside each node's expression trees — no duplicate node ids, no
// duplicate output names, every node has a non-empty id, OutputSchema type
// compatibility where statically determinable) and the topologically-sorted
// result is stored on DecisionTask.DecisionGraph. Definition errors are
// accumulated and the constructor panics once with a *DecisionDefinitionError
// (see § Validation). graph may be empty/nil — a no-node task is valid.
func NewDecisionTask(graph []DecisionNode, opts ...DecisionTaskOption) *DecisionTask

// DecisionTask-specific option constructors (identity uses the shared
// WithId / WithName / WithDescription above). Each returns a DecisionTaskOption.
//
// Decision-logic options — WithInputSchema, WithOutputSchema, WithReferenceData,
// and the shared WithDescription — are valid only on NewDecisionTask; passing any
// of them to Clone panics with a *DecisionDefinitionError (see Clone).
func WithInputSchema(schema BlSchema) DecisionTaskOption
func WithOutputSchema(schema BlSchema) DecisionTaskOption
func WithReferenceData(refs ...ReferenceValue) DecisionTaskOption // see reference-data.spec.md
func WithInputMappings(m *VariableMapping) DecisionTaskOption
func WithOutputMappings(m *VariableMapping) DecisionTaskOption
func WithLoop(l *LoopConfig) DecisionTaskOption
func WithMultiInstance(mi *MultiInstanceConfig) DecisionTaskOption
func WithMaxExecutionsPerProcessInstance(n int) DecisionTaskOption
func WithExitPorts(ports ...ExitPort) DecisionTaskOption

// Standalone evaluation, bypassing task-level framing (mappings, loop,
// multi-instance, exit ports). Useful for unit-testing the decision logic in
// isolation. Callable on a template or a clone with the same result.
func (d *DecisionTask) Evaluate(input map[string]any) (DecisionResult, error)

// Render the decision logic and structure as a markdown string. Independent
// of task-level state.
func (d *DecisionTask) ToMarkdown() string

// Clone returns a new *DecisionTask. Decision-logic fields (DecisionGraph,
// ReferenceData, InputSchema, OutputSchema, Description) are shared by reference
// with the receiver. Task-level fields are taken **only** from the options — the
// receiver's task-level fields are reset, not inherited. Specify every
// task-level field you want set on the clone via With… options.
//
// There is no graph option, and the decision-logic options (WithDescription,
// WithInputSchema, WithOutputSchema, WithReferenceData) panic with a
// *DecisionDefinitionError if passed to Clone: a clone always shares the
// receiver's decision logic, so changing a clone's nodes, schemas, or reference
// data is not expressible.
func (d *DecisionTask) Clone(opts ...DecisionTaskOption) *DecisionTask


// Result of standalone evaluation.
type DecisionResult struct {
    // The declared outputs — keyed by OutputSchema field names. Only populated
    // for fields declared in the output schema (or all node results if
    // OutputSchema is nil).
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

Most task-level fields are optional at creation time, but four become mandatory the moment the `DecisionTask` is incorporated into a process graph. `bl.NewProcess()` walks the graph and rejects any `DecisionTask` whose:

- `Id` is empty,
- `Name` is empty,
- `InputMappings` is nil,
- or `OutputMappings` is nil,

as a `ProcessDefinitionError`.

Mappings are mandatory **even when the decision has no `InputSchema` or `OutputSchema`**. If the decision genuinely takes no inputs or produces no outputs, an empty `bl.NewVariableMapping()` is a valid explicit declaration — but the field must be set deliberately rather than left nil. This forces every decision incorporation to make its data-flow boundaries explicit.

`Loop`, `MultiInstance`, `MaxExecutionsPerProcessInstance`, and `ExitPorts` remain individually optional always.

---

## Construction

The common case is to construct a `DecisionTask` fully baked in a single step — decision logic and task-level fields together — when it is only used in one place:

```go
var oneShotDecision = bl.NewDecisionTask(
    []bl.DecisionNode{eligibility, approval},
    bl.WithId("decide"),
    bl.WithName("Decide"),
    bl.WithInputSchema(is),
    bl.WithOutputSchema(os),
    bl.WithInputMappings(im),
    bl.WithOutputMappings(om),
)
```

(`eligibility` and `approval` are package-scope `var` declarations — see **Building the Decision Logic** below.)

### Reuse via Clone

To reuse the same decision logic across multiple processes, build the decision logic once as a template (no task-level fields), then `Clone` for each use:

```go
// Build the template — decision logic only.
// loanInputSchema / loanOutputSchema are declared with bl.Schema — see
// "Building the Decision Logic" below.
var template = bl.NewDecisionTask(
    []bl.DecisionNode{eligibility, approval},
    bl.WithDescription("Approves or denies a loan based on eligibility and amount"),
    bl.WithInputSchema(loanInputSchema),
    bl.WithOutputSchema(loanOutputSchema),
)

// Use in process A — Id, Name, InputMappings, OutputMappings all required
var riskCheckA = template.Clone(
    bl.WithId("risk-check"),
    bl.WithName("Risk Assessment"),
    bl.WithInputMappings(bl.NewVariableMapping(
        [2]string{"start.applicant",   "applicant"},
        [2]string{"start.loan_amount", "loan_amount"},
    )),
    bl.WithOutputMappings(bl.NewVariableMapping(
        [2]string{"approval", "risk-check.approval"},
    )),
)

// Use in process B with different mappings, a Loop, and an SLA exit port
var slaTimer = bl.NewInterruptingTimerExitPort("sla", bl.DaysTimeDuration("PT1M"))

var riskEvalB = template.Clone(
    bl.WithId("risk-eval"),
    bl.WithName("Risk Evaluation"),
    bl.WithInputMappings(bl.NewVariableMapping(
        [2]string{"start.applicant_v2", "applicant"},
        [2]string{"start.loan_amount",  "loan_amount"},
    )),
    bl.WithOutputMappings(bl.NewVariableMapping(
        [2]string{"approval", "risk-eval.approval"},
    )),
    bl.WithLoop(bl.NewLoopConfig(
        bl.StringVar("risk-eval.approval").Equals(bl.String("indeterminate")),
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

Each concrete decision node is built via its generic constructor (`NewDecisionTable`, `NewLiteralExpression`, etc. — see [decision-node.spec.md](decision-node.spec.md)). Nodes are package-scope `var` declarations; downstream nodes reference upstream outputs through typed `.Outputs.X` fields. The constructor functions used in the previous spec revision are gone.

```go
var applicantSchema, _ = bl.Schema(
    bl.Field{Name: "age",    Type: bl.TypeNumber},
    bl.Field{Name: "income", Type: bl.TypeNumber},
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
        *bl.NewRule().
            AddInputEntry(eligAgeCol,    eligAgeCol.GreaterThanOrEqual(bl.Number(18))).
            AddInputEntry(eligIncomeCol, eligIncomeCol.GreaterThanOrEqual(bl.Number(30000))).
            AddOutputEntry("eligibility", bl.String("eligible")),
        *bl.NewRule().
            AddInputEntry(eligAgeCol, eligAgeCol.LessThan(bl.Number(18))).
            AddOutputEntry("eligibility", bl.String("ineligible")),
        *bl.NewRule().
            AddInputEntry(eligIncomeCol, eligIncomeCol.LessThan(bl.Number(30000))).
            AddOutputEntry("eligibility", bl.String("ineligible")),
    },
})

type ApprovalOutputs struct {
    Status BlString
}

var approval = NewLiteralExpression[ApprovalOutputs](LiteralExpressionOpts{
    Id:   "approval",
    Name: "Loan Approval",
    Body: bl.If(
        eligibility.Outputs.Eligibility.Equals(bl.String("eligible")),
        bl.String("approved"),
        bl.String("denied"),
    ),
})

var loanInputSchema, _ = bl.Schema(
    bl.Field{Name: "applicant",   Type: bl.TypeDictionary, Fields: applicantSchema},
    bl.Field{Name: "loan_amount", Type: bl.TypeNumber},
)
var loanOutputSchema, _ = bl.Schema(
    bl.Field{Name: "approval", Type: bl.TypeString},
)

var loanApproval = bl.NewDecisionTask(
    []bl.DecisionNode{eligibility, approval},
    bl.WithDescription("Approves or denies a loan based on eligibility and amount"),
    bl.WithInputSchema(loanInputSchema),
    bl.WithOutputSchema(loanOutputSchema),
)

// Standalone evaluation works on the template — no Clone needed.
result, err := loanApproval.Evaluate(map[string]any{
    "applicant":   bl.Context(map[string]bl.BlValue{"age": bl.Number(30), "income": bl.Number(50000)}),
    "loan_amount": bl.Number(200000),
})

// result.Outputs         // {"approval": bl.BlString("approved")}
// result.AllResults      // {"eligibility": ..., "approval": ...}
// result.EvaluatedNodes  // ["eligibility", "approval"]
```

The template's `InputSchema` and `OutputSchema` declare its external interface (callers must supply `applicant` and `loan_amount`; the decision exposes `approval`).

Node-level dependencies are derived from the typed output handles referenced in each node's expression trees (rules, body, entries, rows, bindings). `NewDecisionTask` walks those trees, collects every output handle, and uses the handle's source pointer to build the producer→consumer graph. There are no string-keyed name lookups.

### Including reference data

Static constants — `ReferenceData` value sources (see [reference-data.spec.md](reference-data.spec.md)) — are added to the task with `bl.WithReferenceData` and referenced from node expressions **by their `Id`**. They are not `DecisionNode`s, so they never appear in the `[]bl.DecisionNode` list; the task binds each value into the evaluation context under its `Id` and adds its `Id`/type to the reference scope nodes compile against. Because reference data is decision-logic, clones inherit it by reference (it is never re-added — see [§ Reuse via Clone](#reuse-via-clone)).

```go
var taxRate = bl.NewReferenceData(bl.Number(0.2),
    bl.WithId("tax_rate"),
    bl.WithName("Tax Rate"),
)

// grossPrice is a node whose body references the constant by name, e.g.
// `net_price.amount * (1 + tax_rate)`.
var pricing = bl.NewDecisionTask(
    []bl.DecisionNode{netPrice, grossPrice},
    bl.WithReferenceData(taxRate),
    bl.WithId("pricing"),
    bl.WithName("Pricing"),
    bl.WithInputSchema(pricingInputSchema),
)
```

Pass several at once — `bl.WithReferenceData(taxRate, baseCurrency, shippingRates)` — or call it more than once; the sources accumulate.

---

## Evaluation

### In a process

When the runtime reaches an instantiated `DecisionTask` node:

1. Apply `InputMappings` to the current `ExecutionContext` to compute the decision's `input` map.
2. Validate `input` against `InputSchema` (if set), via `ValidateInput`. Failure produces a `DataContractValidationError`.
3. Run the decision-logic algorithm (below) against `input`.
4. Filter results by `OutputSchema` (if set), via `ValidateOutput`.
5. Apply `OutputMappings` to map result fields into target variable names.
6. Merge the mapped result into the `ExecutionContext` under the task's `Id`.

### Standalone

`Evaluate(input)` runs the decision logic without task-level framing. It performs steps 2–4 above, returning a `DecisionResult`. Useful for unit testing.

### Internal evaluation algorithm

When `Evaluate(input)` runs:

1. Iterate `DecisionGraph.DecisionNodes` in stored order (already topologically sorted by `NewDecisionTask`).
2. The first nodes have no node-to-node dependencies and receive only the caller-provided `input` variables.
3. Each node's output is stored in the evaluation context under its `Id`. Multi-output nodes' results are `bl.BlDictionary` values keyed by the per-field output names declared on their outputs struct.
4. Downstream nodes receive the accumulated context (caller inputs + upstream outputs).
5. All nodes are evaluated, regardless of whether they appear in the output contract.
6. The `DecisionResult` contains the full results, with `Outputs` filtered to fields declared in `OutputSchema`.

### Input Resolution

- `InputSchema` declares the expected input variable names and types.
- Each node's dependencies are derived statically from the typed output handles referenced inside its expression trees (rules, body, entries, rows, bindings). A handle whose source is another node produces a node-to-node edge; a handle whose source is a DecisionTask-level input is resolved from the caller-provided `input` map at evaluation time.
- At evaluation time, input values are validated against `InputSchema` (via `ValidateInput`). A mismatch produces a `DataContractValidationError`.
- Reference data registered via `WithReferenceData` is bound into the evaluation context under each source's `Id` before nodes run, and each source's `Id` and type are added to the environment node expressions are compiled against — a **reference scope** separate from `InputSchema`. Referencing a reference-data name forms no dependency edge; the source is a constant leaf (see [reference-data.spec.md](reference-data.spec.md)).

---

## Input and Output Schemas

`InputSchema` and `OutputSchema` are independent and optional, and are both `bl.BlSchema` values (see [schema.spec.md](../expressions/schema.spec.md)). Direction is expressed by which field a schema occupies rather than by distinct Go types.

- **`InputSchema`** — declares the named, typed variables the decision expects from callers. Validated at evaluation entry via `ValidateInput` (closed: undeclared input variables are rejected).
- **`OutputSchema`** — declares which computed values are exposed in `DecisionResult.Outputs` and their expected types, validated via `ValidateOutput` (permissive). The field names must match a node `Id` (for single-output nodes) or a `<node-id>.<output-name>` path (for individual fields of multi-output nodes); this field-name set also drives output projection. All nodes are still evaluated (intermediary results may be needed by output nodes), but only fields declared in `OutputSchema` appear in `Outputs`. Full results are available in `AllResults`.

If `OutputSchema == nil`, `Outputs` contains all node results (equivalent to exposing everything). If `InputSchema == nil`, no input validation runs.

> **Scope note.** This `bl.BlSchema` typing currently applies to `DecisionTask` only. Decision-node constructors and the `StartEvent`/`EndEvent` `InputContract`/`OutputContract` ([data-contract.spec.md](../data/data-contract.spec.md)) still use the older contract types, pending the family-wide migration described in [schema.spec.md § Migration](../expressions/schema.spec.md#migration).

---

## Validation

### Definition validation (in `NewDecisionTask` / `Clone`)

`NewDecisionTask` and `Clone` validate the definition, accumulate **every** problem, and panic once with a `*DecisionDefinitionError`. The `With…` options are plain setters and do not validate individually — centralising the checks in the constructor is what lets it report all problems together (a panic inside an option would abort at the first one). Checks:

- Graph validity: no cycles among nodes (derived from output-handle references inside each node's expression trees), no duplicate node ids, no duplicate output names, every node has a non-empty id.
- `OutputSchema` field names each match a node `Id` or `<node-id>.<output-name>` path; node output types are compatible with the declared types where statically determinable.
- `MaxExecutionsPerProcessInstance >= 0`.
- Exit-port ids are unique within the task.
- `Loop` and `MultiInstance` are not both set.
- Reference-data `Id`s are unique and collide with no node `Id` or `InputSchema` variable name; every name a node references resolves to an input, an upstream node output, or a registered reference-data `Id`.

```go
type DecisionDefinitionError struct {
    TaskId   string
    Problems []string // one actionable message per violation
}
```

Because tasks are typically declared as package-scope `var`s, a bad definition panics during package initialisation (program start / `go test`); the panic value lists each problem and the stack trace pins the offending declaration. `bl.Schema(...)` is independently fallible (`(BlSchema, error)`), so a malformed `InputSchema`/`OutputSchema` surfaces there, before it reaches the constructor.

### Task-level validation (at process construction)

When `bl.NewProcess()` walks the graph and encounters a `DecisionTask`, it checks the incorporation-mandatory fields and mapping references:

- `Id` is non-empty.
- `Name` is non-empty.
- `InputMappings` is non-nil.
- `OutputMappings` is non-nil.
- Mappings reference variables that exist in the surrounding process input or upstream task outputs (subject to the same rules as other Tasks).

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
- **Dependencies** — a summary listing each node and the upstream nodes whose outputs it references in its expression trees.

To render a process's overall structure including instantiated decision tasks, use `Process.ToMarkdown()` (see [../processes/process.spec.md](../processes/process.spec.md)).

---

## Edge Cases

- A `DecisionTask` whose `Id`, `Name`, `InputMappings`, or `OutputMappings` is empty/nil cannot be added to a process graph. `bl.NewProcess()` raises `ProcessDefinitionError`.
- An empty `bl.NewVariableMapping()` is a valid `InputMappings` or `OutputMappings` value — it explicitly declares "no variables flow in/out." A nil `*VariableMapping` is rejected; use the empty constructor instead.
- Calling `Clone` repeatedly on the same template produces independent instances. Options applied to one clone do not propagate to others or to the source.
- `Clone` resets task-level fields — it does not inherit them from the source. Specify every task-level field you want on the clone via `With…` options.
- Decision-logic fields (`DecisionGraph`, `ReferenceData`, `InputSchema`, `OutputSchema`, `Description`) are shared by reference between a source and its clones. Because `DecisionTask` is immutable after `NewDecisionTask`, this sharing has no observable effect beyond memory layout.
- Passing a decision-logic option (`WithDescription`, `WithInputSchema`, `WithOutputSchema`, `WithReferenceData`) to `Clone` is a `DecisionDefinitionError` — decision logic is shared from the source, never re-set on the clone. Reference data therefore needs no re-adding: a clone keeps the source's value sources by reference.
- `Evaluate(input)` is callable on a template or a clone and produces identical results — task-level framing is not exercised by the standalone path.
- A template with a single node and no contracts is valid and can be cloned.
- `Evaluate(input)` with an input variable not referenced by any node is silently ignored.
- A missing required input variable is resolved as `bl.BlNull` — but `InputSchema` validation runs first and may reject it before evaluation begins.
- Invalid decision graphs (cycles among nodes, duplicate node ids, duplicate output names, empty node ids) are rejected by `NewDecisionTask` with a `DecisionDefinitionError`; a `*DecisionTask` value therefore always holds a valid graph. An output handle from outside the graph (referencing a node that is not in `DecisionGraph`) is also a `DecisionDefinitionError`.
- If a node faults during evaluation, the error propagates — downstream dependents also fault. Nodes not in the dependency chain of the faulted node are still evaluated.
- A `DecisionTask` with no nodes evaluates successfully, returning an empty result.
- A task with `InputSchema == nil` accepts any input variables without type checking.
- A task with `OutputSchema == nil` exposes all node results in `Outputs`.
- A clone with `Loop` and `MultiInstance` both set produces a `ProcessDefinitionError`.
- Exit ports are configured **only** via the `WithExitPorts(...)` option. The inherited `Task.AddInterruptingWaitForDuration(...)` / `AddErrorExitPort(...)` / `AddInterruptingConditional(...)` etc. methods do not apply to `DecisionTask`. Use the standalone constructors (`NewInterruptingTimerExitPort`, `NewErrorExitPort`, etc. — see [../processes/task-nodes.spec.md](../processes/task-nodes.spec.md)) to build the exit ports you pass to `WithExitPorts(...)`.
- Exit-port flow targets are wired via `task.ExitPort(id).To(...)` in the graph block, as for any other Task. This is graph-construction wiring, not task mutation.
- Exit-port ids must be unique within a single `DecisionTask` instance. Ids are not shared across clones — two clones of the same template can each have an exit port called `"sla"`.
