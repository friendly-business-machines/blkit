---
name: BoxedContext
description: A DecisionNode defined by a set of named context entries — each entry maps a variable name to a boxed expression, automatically sorted by dependencies, with an optional final result expression
targets:
  - ../decisions/boxed_context.go
---

# BoxedContext

A `BoxedContext` is a `DecisionNode` defined by a set of named context entries. Each entry binds a variable name to a value expression. Entries are automatically sorted into an execution order based on their dependencies — if one entry references a variable defined by another entry, the dependency is evaluated first. Each entry's result is available to dependent entries as a typed ref returned from `AddEntry`.

A `BoxedContext` has two forms:

1. **Without a final result** — evaluates to a `BlContext` containing all entries as key-value pairs.
2. **With a final result** — evaluates to the result of the final (unnamed) expression, with all preceding entries available.

```go
type BoxedContext struct {
    DecisionNode  // Id, Name, Description, OutputName, plus Require*/Optional* methods

    Entries []BoxedEntry
    Result  BlExpr // optional final result expression
}

type BoxedEntry struct {
    Name       string
    Expression BlExpr
}

// AddEntry records an entry on the context AND returns a typed ref bound to
// the entry's name. The ref is usable in subsequent entry expressions and in
// the optional final Result.
func (b *BoxedContext) AddNumberEntry(name string, expression BlExpr) BlNumber
func (b *BoxedContext) AddStringEntry(name string, expression BlExpr) BlString
func (b *BoxedContext) AddBooleanEntry(name string, expression BlExpr) BlBoolean
func (b *BoxedContext) AddDateEntry(name string, expression BlExpr) BlDate
func (b *BoxedContext) AddTimeEntry(name string, expression BlExpr) BlTime
func (b *BoxedContext) AddDateTimeEntry(name string, expression BlExpr) BlDateTime
func (b *BoxedContext) AddDaysTimeEntry(name string, expression BlExpr) BlDaysTimeDuration
func (b *BoxedContext) AddYearsMonthsEntry(name string, expression BlExpr) BlYearsMonthsDuration
func (b *BoxedContext) AddListEntry(name string, expression BlExpr) BlList
func (b *BoxedContext) AddContextEntry(name string, expression BlExpr, schema *ContextContract) BlContext

func (b *BoxedContext) SetResult(expression BlExpr) *BoxedContext

// Evaluate the context against the input variables
func (b *BoxedContext) Evaluate(input map[string]any) (BlValue, error)

// Render as a markdown string
func (b *BoxedContext) ToMarkdown() string
```

`BoxedContext` is instantiated via direct struct literal — no `New*` factory.

---

## Building a BoxedContext

The constructor function declares the node's typed inputs first; entry expressions reference those inputs and any earlier entries' refs. The final `SetResult` (if present) likewise uses captured refs.

```go
func monthlyBreakdown() *BoxedContext {
    bc := &BoxedContext{
        Id:   "monthly_breakdown",
        Name: "Monthly Breakdown",
    }
    loanAmount := bc.RequireNumber("loan_amount")
    rate       := bc.RequireNumber("rate")
    term       := bc.RequireNumber("term")

    principal := bc.AddNumberEntry("principal", loanAmount.Divide(term))
    interest  := bc.AddNumberEntry("interest",  loanAmount.Multiply(rate).Divide(Bl.Number(12)))
    bc.AddNumberEntry("total", principal.Add(interest))

    return bc
}

result, err := monthlyBreakdown().Evaluate(map[string]any{
    "loan_amount": Bl.Number(120000),
    "rate":        Bl.Number(0.06),
    "term":        Bl.Number(12),
})
// result is a BlContext: {principal: 10000, interest: 600, total: 10600}
```

With a final result, the `BoxedContext` evaluates to a single value:

```go
func eligibilityChecker() *BoxedContext {
    bc := &BoxedContext{
        Id:   "eligibility",
        Name: "Eligibility",
    }
    age    := bc.RequireNumber("age")
    income := bc.RequireNumber("income")

    ageOk    := bc.AddBooleanEntry("age_ok",    age.GreaterThanOrEqual(Bl.Number(18)))
    incomeOk := bc.AddBooleanEntry("income_ok", income.GreaterThanOrEqual(Bl.Number(30000)))

    bc.SetResult(Bl.If(
        ageOk.And(incomeOk),
        Bl.String("eligible"),
        Bl.String("ineligible"),
    ))
    return bc
}

result, err := eligibilityChecker().Evaluate(map[string]any{
    "age":    Bl.Number(25),
    "income": Bl.Number(50000),
})
// result is BlString("eligible") — not a context, just the final result
```

---

## Evaluation

Whenever entries are added, the `BoxedContext` builds a dependency graph by inspecting which entry refs each expression references. Entries are topologically sorted into an execution order — entries with no dependencies on other entries are evaluated first, and each entry's result is added to the local context before its dependents are evaluated. The declaration order does not affect the execution order.

Circular dependencies between entries produce a `DecisionDefinitionError` at the point the cycle is introduced.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string representing the boxed context. Each entry is rendered as a row showing the variable name and its expression (via `BlExpr.ToMarkdown()`). If a final result expression is set, it is rendered as a separate row.

### Example

```go
fmt.Println(monthlyBreakdown().ToMarkdown())
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

When a final result is set, it appears as a final row:

```text
### Eligibility

| Name      | Expression                                                |
|-----------|-----------------------------------------------------------|
| age_ok    | age >= 18                                                 |
| income_ok | income >= 30000                                           |
| (result)  | if age_ok and income_ok then "eligible" else "ineligible" |
```

---

## Edge Cases

- A `BoxedContext` with no entries and no result evaluates to an empty `BlContext`.
- A `BoxedContext` with no entries but with a result evaluates the result expression against only the input variables.
- If an entry's expression evaluates to `BlNull`, dependent entries can still reference that ref (it resolves to `BlNull`).
- Duplicate entry names are invalid; `Add*Entry` with an existing name produces a `DecisionDefinitionError`.
- Circular dependencies between entries produce a `DecisionDefinitionError` at the point the cycle is introduced.
- Entries with no dependencies on other entries may execute in any order relative to each other.
- The `Result` expression can reference any entry's typed ref. It is always evaluated last, after all entries.
