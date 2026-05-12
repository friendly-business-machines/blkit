---
name: LiteralExpression
description: A DecisionNode defined by a single expression body — evaluates the body against declared typed inputs and returns the result
targets:
  - ../decisions/literal_expression.go
---

# LiteralExpression

A `LiteralExpression` is a `DecisionNode` defined by a single `BlExpr` body. When evaluated, the body is computed against the inputs declared on the node, returning a single `BlValue`.

```go
type LiteralExpression struct {
    DecisionNode  // Id, Name, Description, OutputName, plus Require*/Optional* methods

    Body    BlExpr  // the expression to evaluate
    TypeRef *string // expected output type (blkit type name)
}

// Evaluate the body against the input variables
func (l *LiteralExpression) Evaluate(input map[string]any) (BlValue, error)

// Render as a markdown string
func (l *LiteralExpression) ToMarkdown() string
```

`LiteralExpression` is instantiated via direct struct literal — no `New*` factory. Inputs are declared on the node via the inherited `Require*` / `Optional*` methods (see [decision-node.spec.md](decision-node.spec.md)).

---

## Building a node — the constructor-function idiom

Each non-trivial `LiteralExpression` lives in its own dedicated, domain-named Go function. Inputs are declared first; the captured typed refs are used to build the body:

```go
func monthlyPaymentCalc() *LiteralExpression {
    calc := &LiteralExpression{
        Id:   "monthly_payment",
        Name: "Monthly Payment",
    }
    loanAmount := calc.RequireNumber("loan_amount")
    rate       := calc.RequireNumber("rate")
    calc.Body = loanAmount.Multiply(rate).Divide(Bl.Number(12))
    return calc
}

result, err := monthlyPaymentCalc().Evaluate(map[string]any{
    "loan_amount": Bl.Number(200000),
    "rate":        Bl.Number(0.05),
})
// result is a BlNumber
```

A conditional expression follows the same shape:

```go
func applicationStatus() *LiteralExpression {
    status := &LiteralExpression{
        Id:   "status",
        Name: "Application Status",
    }
    score := status.RequireNumber("score")
    status.Body = Bl.If(
        score.GreaterThanOrEqual(Bl.Number(700)),
        Bl.String("approved"),
        Bl.String("review"),
    )
    return status
}
```

The function-scope locals `loanAmount`, `rate`, `score` are independent of any other constructor function in the same file — Go scoping handles disambiguation naturally.

---

## Type Checking

If `TypeRef` is set, the result of evaluating the body is checked against the declared type. A mismatch produces a `BlTypeError`.

```go
func ageCheck() *LiteralExpression {
    check := &LiteralExpression{
        Id:      "age_check",
        TypeRef: stringPtr("boolean"),
    }
    age := check.RequireNumber("age")
    check.Body = age.GreaterThanOrEqual(Bl.Number(18))
    return check
}

result, err := ageCheck().Evaluate(map[string]any{"age": Bl.Number(25)})
// result == BlBoolean.TRUE
```

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing the node name, its declared inputs, and the body rendered via `BlExpr.ToMarkdown()`.

```go
fmt.Println(monthlyPaymentCalc().ToMarkdown())
```

Output:

```text
### Monthly Payment

**Inputs:** loan_amount (number), rate (number)

**Expression:** `loan_amount * rate / 12`
```

---

## Edge Cases

- A `LiteralExpression` with no `Body` set (`nil`) is invalid; `Evaluate()` returns a `DecisionDefinitionError`.
- A `Body` that evaluates to `BlNull` is a valid result, not an error.
- If `TypeRef` is set and the body evaluates to `BlNull`, no type check is performed (`BlNull` is compatible with any type).
- Inputs declared via `Require*` that are not referenced in the body are still recorded as dependencies (and evaluated by the model before this node), even though the body ignores them.
