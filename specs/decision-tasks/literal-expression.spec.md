---
name: LiteralExpression
description: A DecisionNode generic over a single-field outputs struct — evaluates a single BlExpr body whose result is bound to the outputs struct's field
targets:
  - ../decisions/literal_expression.go
---

# LiteralExpression

A `LiteralExpression` is a `DecisionNode` defined by a single `BlExpr` body. When evaluated, the body is computed against the input variables, and the result is the value of the node's single output.

`LiteralExpression` is generic over an outputs struct (see [decision-node.spec.md](decision-node.spec.md)). The outputs struct must have exactly one exported field; the body's runtime type must match that field's static type.

```go
type LiteralExpression[Outputs any] struct {
    Id          string
    Name        string
    Description string

    Body BlExpr

    Outputs Outputs // single-field typed handle, populated by NewLiteralExpression
}

func NewLiteralExpression[Outputs any](opts LiteralExpressionOpts) *LiteralExpression[Outputs]

type LiteralExpressionOpts struct {
    Id          string
    Name        string
    Description string
    Body        BlExpr
}

// Evaluate the body against the input variables
func (l *LiteralExpression[Outputs]) Evaluate(input map[string]any) (BlValue, error)

// Render as a markdown string
func (l *LiteralExpression[Outputs]) ToMarkdown() string
```

`NewLiteralExpression` enforces that `Outputs` has exactly one exported field. The body's declared type (the static type of `opts.Body` as a `BlExpr`) must match that field's `Bl*` type; otherwise `DecisionDefinitionError` is raised. The constructor populates the single field with a typed handle so downstream nodes can reference this node's output as `node.Outputs.FieldName`.

---

## Building a LiteralExpression

```go
type MonthlyPaymentOutputs struct {
    Amount BlNumber
}

var monthlyPayment = NewLiteralExpression[MonthlyPaymentOutputs](LiteralExpressionOpts{
    Id:   "monthly_payment",
    Name: "Monthly Payment",
    Body: loanAmount.Multiply(rate).Divide(Bl.Number(12)),
})

result, err := monthlyPayment.Evaluate(map[string]any{
    "loan_amount": Bl.Number(200000),
    "rate":        Bl.Number(0.05),
})
// result is a BlNumber matching monthlyPayment.Outputs.Amount
```

Here `loanAmount` and `rate` are typed `BlNumber` handles drawn from earlier nodes' `.Outputs.X` fields (or DecisionTask-level inputs). The body composes them with `Bl` operators; the resulting `BlExpr` is typed `BlNumber`, matching the outputs struct's single field.

### Conditional

```go
type ApplicationStatusOutputs struct {
    Status BlString
}

var applicationStatus = NewLiteralExpression[ApplicationStatusOutputs](LiteralExpressionOpts{
    Id:   "status",
    Name: "Application Status",
    Body: Bl.If(
        score.GreaterThanOrEqual(Bl.Number(700)),
        Bl.String("approved"),
        Bl.String("review"),
    ),
})
```

`score` is a typed `BlNumber` handle from an upstream node (e.g., `creditCheck.Outputs.Score`).

### Cross-node reference

```go
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
```

`eligibility.Outputs.Eligibility` is the typed `BlString` handle exposed by the `eligibility` `DecisionTable` node from its `EligibilityOutputs` struct.

---

## Type Checking

The outputs struct's single field declares the expected output type. The body's compile-time type (e.g., `BlNumber`, `BlString`) must match that field. A mismatch is a `DecisionDefinitionError` at construction time. A runtime body value that disagrees with its compile-time type (which should not happen for type-safe `Bl*` expressions, but may for raw values) produces a `BlTypeError` at evaluation time.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing the node name, the referenced input variables, and the body rendered via `BlExpr.ToMarkdown()`.

```go
fmt.Println(monthlyPayment.ToMarkdown())
```

Output:

```text
### Monthly Payment

**Inputs:** loan_amount (number), rate (number)

**Expression:** `loan_amount * rate / 12`
```

The **Inputs** line lists every variable referenced by the body (resolved by walking the expression tree and collecting external references).

---

## Edge Cases

- A `LiteralExpression` whose `Body` is `nil` is invalid; `NewLiteralExpression` raises `DecisionDefinitionError`.
- A `LiteralExpression` whose `Outputs` struct has zero or more than one exported field is invalid; raises `DecisionDefinitionError`.
- A `Body` whose compile-time type disagrees with the outputs-struct field type is invalid; raises `DecisionDefinitionError`.
- A `Body` that evaluates to `BlNull` is a valid result.
- A `Body` that evaluates to `BlNull` is treated as compatible with any output type (consistent with FEEL semantics).
