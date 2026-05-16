---
name: BoxedContext
description: A DecisionNode generic over an outputs struct whose fields are the context entries — named, typed expressions with automatic dependency sorting
targets:
  - ../decisions/boxed_context.go
---

# BoxedContext

A `BoxedContext` is a `DecisionNode` defined by a set of named entries. Each entry binds an output name to a value expression. Entries can reference other entries' typed handles; the constructor topologically sorts entries by their inter-entry dependencies.

`BoxedContext` is generic over an outputs struct (see [decision-node.spec.md](decision-node.spec.md)). Every exported field on the outputs struct is one entry on the context. The node evaluates to a `BlContext` keyed by the entry names.

> A `BoxedContext` is always multi-output. For a single-value computation with named intermediate bindings, use a `LiteralExpression` with intermediate Go vars instead.

```go
type BoxedContext[Outputs any] struct {
    Id          string
    Name        string
    Description *string

    Entries []BoxedEntry // topologically sorted by NewBoxedContext

    Outputs Outputs // typed handles, populated by NewBoxedContext
}

type BoxedEntry struct {
    Output     BlExpr // the typed handle from the outputs struct that this entry sets
    Expression BlExpr // the value expression
}

func NewBoxedContext[Outputs any](opts BoxedContextOpts[Outputs]) *BoxedContext[Outputs]

type BoxedContextOpts[Outputs any] struct {
    Id          string
    Name        string
    Description *string

    // Entries is a builder closure that receives a pre-populated outputs handle
    // and returns the entry expressions. Use the typed Bl* handles on `o` to
    // identify which entry each expression sets and to cross-reference other
    // entries' outputs.
    Entries func(o *Outputs) []BoxedEntry
}

// Helper: pair an outputs-struct handle with the expression that sets it.
func Entry(output BlExpr, expression BlExpr) BoxedEntry

// Evaluate the context against the input variables
func (b *BoxedContext[Outputs]) Evaluate(input map[string]any) (BlValue, error)

// Render as a markdown string
func (b *BoxedContext[Outputs]) ToMarkdown() string
```

`NewBoxedContext` allocates the outputs handle, invokes the `Entries` closure, validates the resulting entry list against the outputs struct's fields (every field must be set by exactly one entry; every `Output` handle must belong to this node's outputs struct), then topologically sorts the entries by their cross-entry dependencies. Cycles produce a `DecisionDefinitionError`.

---

## Building a BoxedContext

```go
type MonthlyBreakdownOutputs struct {
    Principal BlNumber
    Interest  BlNumber
    Total     BlNumber
}

var monthlyBreakdown = NewBoxedContext[MonthlyBreakdownOutputs](BoxedContextOpts[MonthlyBreakdownOutputs]{
    Id:   "monthly_breakdown",
    Name: "Monthly Breakdown",
    Entries: func(o *MonthlyBreakdownOutputs) []BoxedEntry {
        return []BoxedEntry{
            Entry(o.Principal, loanAmount.Divide(term)),
            Entry(o.Interest,  loanAmount.Multiply(rate).Divide(Bl.Number(12))),
            Entry(o.Total,     o.Principal.Add(o.Interest)),
        }
    },
})

result, err := monthlyBreakdown.Evaluate(map[string]any{
    "loan_amount": Bl.Number(120000),
    "rate":        Bl.Number(0.06),
    "term":        Bl.Number(12),
})
// result is BlContext: {principal: 10000, interest: 600, total: 10600}
//
// Access typed values downstream:
// monthlyBreakdown.Outputs.Principal — BlNumber handle
// monthlyBreakdown.Outputs.Total     — BlNumber handle
```

Here `loanAmount`, `rate`, and `term` are typed `BlNumber` handles from upstream nodes' `.Outputs.X` fields (or DecisionTask-level inputs). `o.Principal` and `o.Interest` are this node's own handles — using them inside the closure declares cross-entry dependencies that `NewBoxedContext` honours when sorting.

---

## Evaluation

The entry list is already topologically sorted by `NewBoxedContext`. At evaluation time:

1. Each entry's expression is evaluated against the input variables plus the accumulated entry results.
2. The entry's result is stored in the local context under its name (the lowercased outputs-struct field name, or the `bl:"name"` tag).
3. After all entries are evaluated, the result is a `BlContext` keyed by entry names.

Cycles in the entry graph are rejected at construction time and never observed during evaluation.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing each entry's name and its expression in the topologically sorted execution order.

### Example

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

- A `BoxedContext` whose `Outputs` struct has no exported fields is invalid; `NewBoxedContext` raises `DecisionDefinitionError`.
- A `BoxedContext` whose `Entries` closure returns fewer entries than the outputs struct has fields — or duplicates — is invalid; raises `DecisionDefinitionError`.
- Every entry's `Output` handle must originate from the outputs struct passed to the closure. A foreign handle (e.g., another node's `.Outputs.X`) used in the `Output` position produces `DecisionDefinitionError`.
- An `Output` handle used inside the `Expression` of another entry declares a cross-entry dependency; cycles are rejected at construction.
- If an entry's expression evaluates to `BlNull`, dependent entries can still reference that handle (it resolves to `BlNull`).
- Entries with no dependencies on other entries may execute in any order relative to each other.
