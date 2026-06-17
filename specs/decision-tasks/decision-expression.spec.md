---
name: DecisionExpression
description: A DecisionNode generic over an outputs struct whose fields are named, text-expression entries with automatic by-name dependency sorting — evaluates to a scalar when single-output, a bl.BlDictionary when multi-output; entry input types are wired from the DecisionTask graph, output types are reflected from the Outputs struct
targets:
  - ../decisions/decision_expression.go
---

# DecisionExpression

A `DecisionExpression` is a `DecisionNode` defined by a set of named entries. Each entry binds an output name to a value expression written in the [blkit expression language](../expressions/bl-expr.spec.md) — a raw-string source compiled via `bl.Expr`. Entries may reference other entries by name; the constructor topologically sorts entries by their inter-entry dependencies.

`DecisionExpression` is generic over an outputs struct (see [decision-node.spec.md](decision-node.spec.md)). Every exported field on the outputs struct is one entry. A **single-field** outputs struct evaluates to that field's `bl.BlValue` directly (the expression-based counterpart to authoring a decision as a single value); a **multi-field** outputs struct evaluates to a `bl.BlDictionary` keyed by the entry names.

`DecisionExpression` is the expression-based sibling of [`DecisionTable`](decision-table.spec.md): both author a decision node and infer their outputs from an `Outputs` struct, but a `DecisionTable` expresses logic as tabular rules whereas a `DecisionExpression` expresses it as named text expressions.

It draws on two DMN boxed-expression forms: the single-output case is DMN's **literal expression** (one expression yielding one value), and the multi-output case is DMN's **boxed context** (named entries, the last of which may serve as a result). `DecisionExpression` unifies them — a single-entry node is just the degenerate boxed context.

```go
type Entries map[string]string // output name -> raw-string source expression

type DecisionExpression[Outputs any] struct {
    Id          string
    Name        string
    Description string

    Entries Entries // parsed and topologically sorted by NewDecisionExpression

    Outputs Outputs // typed handles, populated by NewDecisionExpression
}

func NewDecisionExpression[Outputs any](opts DecisionExpressionOpts) *DecisionExpression[Outputs]

type DecisionExpressionOpts struct {
    Id          string
    Name        string
    Description string

    // Entries maps each output name to its source expression. The key set must
    // be a bijection with the Outputs struct's effective field names.
    Entries Entries
}

// Evaluate the entries against the input variables
func (d *DecisionExpression[Outputs]) Evaluate(input map[string]any) (BlValue, error)

// Render as a markdown string
func (d *DecisionExpression[Outputs]) ToMarkdown() string
```

`NewDecisionExpression` reflects on the `Outputs` type parameter to derive each output's effective name (the lowercased field name, or the `bl:"name"` tag) and `Bl*` type, allocating a typed handle into each field (see [decision-node.spec.md](decision-node.spec.md) for the shared reflection contract). It then validates that the `Entries` keys form a bijection with those output names, compiles each entry source via `bl.Expr`, builds the intra-node dependency graph from sibling-output references, and topologically sorts. Cycles produce a `DecisionDefinitionError`.

---

## Type wiring

A `DecisionExpression` declares **no input schema**. Input types flow from the graph wiring assembled by the containing [`DecisionTask`](decision-task.spec.md):

- A node's **output types are self-describing** — reflected from its `Outputs` struct.
- The task's typed leaves are its `InputSchema` fields plus registered [reference data](reference-data.spec.md).
- The task walks the decision graph in topological order, extending the compile-time environment with each node's declared output types. By the time a node is type-checked, every name its entries reference resolves to one of: a **sibling entry** (typed from this node's `Outputs`), a **task input**, an **upstream node's output** (typed from that node's `Outputs`), or **reference data** — the four reference targets required by [decision-task.spec.md § Validation](decision-task.spec.md).

Responsibility splits across two phases:

- **Node construction** (`NewDecisionExpression`): reflect `Outputs`; validate the `Entries` key bijection; parse each entry's syntax; build the intra-node dependency graph from sibling references and topologically sort; type-check sibling references against this node's `Outputs` types. External names are left unresolved here.
- **Task construction** (`NewDecisionTask`): type-check every external reference against the assembled environment and wire cross-node edges.
- **Standalone**: with no containing task there is no environment, so external references are checked at evaluation time (see [decision-node.spec.md § Standalone Evaluation](decision-node.spec.md#standalone-evaluation)).

---

## Building a DecisionExpression

### Single output

A single-field outputs struct is the expression-based way to author a one-value decision; `Evaluate` returns that value directly, not a dictionary.

```go
type MonthlyPaymentOutputs struct {
    Amount BlNumber
}

var monthlyPayment = NewDecisionExpression[MonthlyPaymentOutputs](DecisionExpressionOpts{
    Id:   "monthly_payment",
    Name: "Monthly Payment",
    Entries: Entries{
        "amount": `loan_amount * rate / 12`,
    },
})

result, err := monthlyPayment.Evaluate(map[string]any{
    "loan_amount": bl.Number(200000),
    "rate":        bl.Number(0.05),
})
// result is a bl.BlNumber matching monthlyPayment.Outputs.Amount
```

Here `loan_amount` and `rate` are names resolved from the task's inputs (or an upstream node's output) at task construction. `monthlyPayment.Outputs.Amount` is the typed `bl.BlNumber` handle downstream nodes reference.

### Conditional single output

```go
type ApplicationStatusOutputs struct {
    Status BlString
}

var applicationStatus = NewDecisionExpression[ApplicationStatusOutputs](DecisionExpressionOpts{
    Id:   "status",
    Name: "Application Status",
    Entries: Entries{
        "status": `if score >= 700 then "approved" else "review"`,
    },
})
```

`score` is a name resolved from an upstream node's output (e.g. a `credit_check` node) or a task input.

### Multiple outputs with cross-entry references

```go
type MonthlyBreakdownOutputs struct {
    Principal BlNumber
    Interest  BlNumber
    Total     BlNumber
}

var monthlyBreakdown = NewDecisionExpression[MonthlyBreakdownOutputs](DecisionExpressionOpts{
    Id:   "monthly_breakdown",
    Name: "Monthly Breakdown",
    Entries: Entries{
        "principal": `loan_amount / term`,
        "interest":  `loan_amount * rate / 12`,
        "total":     `principal + interest`,
    },
})

result, err := monthlyBreakdown.Evaluate(map[string]any{
    "loan_amount": bl.Number(120000),
    "rate":        bl.Number(0.06),
    "term":        bl.Number(12),
})
// result is bl.BlDictionary: {principal: 10000, interest: 600, total: 10600}
//
// Downstream typed access:
// monthlyBreakdown.Outputs.Principal — bl.BlNumber handle
// monthlyBreakdown.Outputs.Total     — bl.BlNumber handle
```

The `total` entry references `principal` and `interest` by name. Those are this node's own outputs — referencing them declares cross-entry dependencies that `NewDecisionExpression` honours when sorting. `loan_amount`, `rate`, and `term` are external names resolved from task inputs or upstream node outputs.

---

## Evaluation

The entry list is already topologically sorted by `NewDecisionExpression`. At evaluation time:

1. Each entry's compiled expression is evaluated against the input variables plus the accumulated entry results.
2. The entry's result is stored under its output name (the lowercased field name, or the `bl:"name"` tag).
3. If the outputs struct has a single field, that value is returned directly as a `bl.BlValue`; otherwise the result is a `bl.BlDictionary` keyed by the output names.

Cycles in the entry graph are rejected at construction time and never observed during evaluation.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing each entry's name and its source expression, in the `Outputs` struct's field declaration order.

```go
fmt.Println(monthlyBreakdown.ToMarkdown())
```

Output:

```text
### Monthly Breakdown

| Name      | Expression              |
|-----------|-------------------------|
| principal | loan_amount / term      |
| interest  | loan_amount * rate / 12 |
| total     | principal + interest    |
```

---

## Edge Cases

- A `DecisionExpression` whose `Outputs` struct has no exported fields is invalid; `NewDecisionExpression` raises `DecisionDefinitionError`.
- The `Entries` key set must be a bijection with the `Outputs` fields' effective names. An `Entries` key matching no field, or a field with no entry, is a `DecisionDefinitionError`.
- An entry source that does not compile via `bl.Expr` is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- An entry referencing another entry's output name declares a cross-entry dependency; cycles are rejected at construction.
- An entry that evaluates to `bl.BlNull` is valid; dependent entries can still reference its output name (it resolves to `bl.BlNull`).
- Entries with no dependencies on other entries may execute in any order relative to each other.
- A single-field outputs struct evaluates to that field's `bl.BlValue` directly; a multi-field struct evaluates to a `bl.BlDictionary`.
- An entry whose runtime value disagrees with its declared output-field type produces a `bl.TypeError` at evaluation time.
