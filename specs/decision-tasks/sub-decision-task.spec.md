---
name: SubDecisionTask
description: A DecisionNode that wraps a whole child DecisionTask so it can be composed as a single node inside another DecisionTask's graph. Derives its input and output contracts from the child's InputSchema/OutputSchema, with optional name remapping; Evaluate runs the child task fresh and returns its declared outputs.
targets:
  - ../../core/sub_decision_task.go
---

# SubDecisionTask

A `SubDecisionTask` is a [`DecisionNode`](decision-node.spec.md) that wraps a whole child [`DecisionTask`](decision-task.spec.md) so it can be embedded as a single node inside another task's graph. It is how decisions **compose**: a complete, independently-authored decision becomes a black-box node in a larger decision.

Like every node, it declares its contracts as plain data (see [decision-node.spec.md § Contracts are plain data](decision-node.spec.md#contracts-are-plain-data-not-go-generics)) — but it does not declare them by hand. `Inputs()` is derived from the child task's `InputSchema` and `Outputs()` from its `OutputSchema` (or all produced outputs if the child has no `OutputSchema`). Optional `InputMappings`/`OutputMappings` rename those names to fit the parent task's flat namespace and to let the same child task be embedded more than once.

```go
type SubDecisionTask struct {
    Id          string
    Name        string
    Description string

    Task *DecisionTask // the child task this node evaluates

    // Optional name remapping. Default (nil/empty) is name-transparent: the
    // child's own input/output names are used verbatim.
    InputMappings  map[string]string // parent input name -> child input name
    OutputMappings map[string]string // child output name -> parent output name

    // inputs / outputs hold the derived contracts; exposed via the
    // DecisionNode interface methods Inputs() / Outputs().
    inputs  []Field
    outputs []Field
}

func NewSubDecisionTask(config SubDecisionTaskConfig) *SubDecisionTask

type SubDecisionTaskConfig struct {
    Id          string
    Name        string
    Description string

    // Task is the child DecisionTask to evaluate. Required.
    Task *DecisionTask

    // InputMappings renames child input names to parent-facing names. A child
    // input not listed keeps its own name. Each value must name a real child
    // input (a field of Task.InputSchema).
    InputMappings map[string]string

    // OutputMappings renames child output names to parent-facing names. A child
    // output not listed keeps its own name. Each key must name a real child
    // output (a field of Task.OutputSchema, or any produced output if the child
    // has no OutputSchema).
    OutputMappings map[string]string
}

// DecisionNode interface satisfaction.
func (d *SubDecisionTask) GetId() string
func (d *SubDecisionTask) GetName() string
func (d *SubDecisionTask) GetDescription() string
func (d *SubDecisionTask) Inputs() []Field
func (d *SubDecisionTask) Outputs() []Field

// Evaluate the child task against the input variables, returning a map keyed by
// this node's (possibly remapped) output names (see decision-node.spec.md).
func (d *SubDecisionTask) Evaluate(input map[string]BlValue) (map[string]BlValue, error)

// Render as a markdown string
func (d *SubDecisionTask) ToMarkdown() string
```

`NewSubDecisionTask` validates that `Task` is non-nil, that every `InputMappings` value names a real child input and every `OutputMappings` key names a real child output, that the parent-facing names are well-formed, and that no two outputs collapse onto the same parent name. A nil `Task`, a mapping that names a non-existent child field, or a remapped output-name collision is a `DecisionDefinitionError`.

---

## Deriving the contracts

`Inputs()` and `Outputs()` are computed once at construction from the child task:

- **Inputs()** — one `Field` per field of `Task.InputSchema` (a [`BlSchema`](../expressions/schema.spec.md)), with the name replaced by its `InputMappings` entry where present. The type is the child input's declared type.
- **Outputs()** — one `Field` per declared child output: the fields of `Task.OutputSchema` if it is set, otherwise every output the child produces (matching `DecisionResult.Outputs`, which is all produced outputs when the child has no `OutputSchema`). Names are replaced by their `OutputMappings` entry where present.

Because the child task is itself already construction-valid (it is a built `*DecisionTask`), its internal nodes, wiring, and contracts are not re-checked here — only the wrapper's mappings.

---

## Building a SubDecisionTask

### Name-transparent

```go
// creditModel is a *bl.DecisionTask declared elsewhere, with InputSchema
// {income, age} and OutputSchema {score}.
var creditCheck = bl.NewSubDecisionTask(bl.SubDecisionTaskConfig{
    Id:   "credit_check",
    Name: "Credit Check",
    Task: creditModel,
})
// creditCheck.Inputs()  == creditModel.InputSchema  fields (income, age)
// creditCheck.Outputs() == creditModel.OutputSchema fields (score)
```

`income` and `age` are wired by the parent `NewDecisionTask` to producers of those names, exactly as for any node; `score` is the produced output downstream nodes consume.

### With remapping

Remapping fits the child into the parent's namespace and lets one child task be embedded more than once.

```go
var creditCheck = bl.NewSubDecisionTask(bl.SubDecisionTaskConfig{
    Id:   "credit_check",
    Name: "Credit Check",
    Task: creditModel,
    InputMappings: map[string]string{
        "applicant_income": "income", // parent name -> child input name
    },
    OutputMappings: map[string]string{
        "score": "credit_score", // child output -> parent name
    },
})
// creditCheck.Inputs()  advertises applicant_income (+ age)
// creditCheck.Outputs() advertises credit_score
```

---

## Validation and type safety

A `SubDecisionTask` derives its contracts from the child as plain data, so safety is contract-matching, not name-inference. Following the family rule (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)), the wrapper's checks are concentrated at **construction**, with value correctness left to evaluation.

These are **runtime** checks, not Go compile-time ones, and `NewSubDecisionTask` **panics once** with a `*DecisionDefinitionError` per the decision-family convention (see [decision-task.spec.md](decision-task.spec.md)). Because the node is typically a package-scope `var`, a malformed wrapper aborts the program at startup — deterministic load-time fail-fast.

| Moment | Trigger | What it catches | Raised as |
|--------|---------|-----------------|-----------|
| **Node construction** | `NewSubDecisionTask` | A nil `Task`; an `InputMappings` value or `OutputMappings` key that names no child input/output; an ill-formed parent-facing name; two outputs remapped onto the same parent name. | `DecisionDefinitionError` |
| **Task construction** | `NewDecisionTask` | A derived input with no producer of matching name **and** type; a remapped output name or `Id` that collides with another node in the parent task; a cross-node cycle. | `DecisionDefinitionError` |
| **Evaluation** | `Evaluate` | A supplied input value, or a value produced by the child, whose runtime type disagrees with the declared contract; a runtime fault inside the child (propagated). | `bl.TypeError` (or the child's error) |

A **cyclic** sub-reference — a child task that, directly or transitively, embeds a `SubDecisionTask` back to an ancestor — cannot always be caught at construction, because child tasks are independently-built black boxes. Such a cycle surfaces at evaluation via guarded recursion and is returned as an error rather than overflowing the stack.

---

## Evaluation

`Evaluate` is stateless: the `SubDecisionTask` is immutable after construction and the child task is evaluated fresh on each call, so concurrent calls do not interfere.

1. The supplied `input` map — keyed by this node's (possibly remapped) input names — is translated into the child's input names via `InputMappings`.
2. The child task is run: `Task.Evaluate(childInput)` (see [decision-task.spec.md](decision-task.spec.md)), returning a `DecisionResult`.
3. `DecisionResult.Outputs` — the child's declared outputs (or all produced outputs if the child has no `OutputSchema`) — is translated back into this node's output names via `OutputMappings` and returned.

Only the child's declared outputs cross the boundary; intermediate node outputs inside the child (`DecisionResult.AllResults`) stay encapsulated.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing the node's name, its (remapped) input and output contracts, and a reference to the child task it wraps.

```go
fmt.Println(creditCheck.ToMarkdown())
```

Output:

```text
### Credit Check

_Sub-decision task: Credit Model (credit_model)_

| Input            | Type   |
|------------------|--------|
| applicant_income | Number |
| age              | Number |

| Output       | Type   |
|--------------|--------|
| credit_score | Number |
```

---

## Edge Cases

- A `SubDecisionTask` whose `Task` is nil is invalid; `NewSubDecisionTask` raises `DecisionDefinitionError`.
- An `InputMappings` value that names no field of `Task.InputSchema`, or an `OutputMappings` key that names no child output, is a `DecisionDefinitionError`.
- Two `OutputMappings` entries that remap distinct child outputs onto the same parent name are a `DecisionDefinitionError`.
- A remapped parent name that is malformed (per the `Field` name rules) is a `DecisionDefinitionError`.
- A child task with no `OutputSchema` exposes **all** of its produced outputs as this node's `Outputs()`; supply `OutputMappings` to rename them or pin a child `OutputSchema` to narrow them.
- The same child `DecisionTask` may be wrapped by multiple `SubDecisionTask` nodes (with different mappings) within one parent task.
- A cyclic sub-reference is detected at evaluation and returned as an error, not caught at construction.
