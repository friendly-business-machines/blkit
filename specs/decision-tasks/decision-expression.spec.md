---
name: DecisionExpression
description: A generic decision construct defining decision logic as named text-expression entries over concrete Go input (I) and output (O) structs. Each entry binds an output field to a bl-expression; entries may reference declared inputs and sibling outputs by name, with automatic by-name dependency sorting. Evaluate(inputs I) returns O, type-checked at Go compile time.
targets:
  - ../../core/decision_expression.go
---

# DecisionExpression

A `DecisionExpression[I, O]` defines decision logic as a set of named entries over two concrete Go structs: an **input** struct `I` and an **output** struct `O`. Each entry binds one output to a value expression written in the [blkit expression language](../expressions/bl-expr.spec.md) — a raw-string source compiled via `bl.Expr`. Entries may reference the declared inputs and may reference one another by output name; the constructor topologically sorts entries by their inter-entry dependencies.

Because the input and output contracts are concrete Go structs, a caller that passes the wrong input shape or reads a non-existent output gets a **Go compile error**, and `Evaluate(inputs I) (O, error)` carries the result back as a typed struct. The exported fields of `I` and `O` are tagged `expr:"name"` so entries reference them by their FEEL names.

> **Note.** Genericising the input/output contracts takes `DecisionExpression` out of the uniform `map[string]BlValue`-based [`DecisionNode`](decision-node.spec.md) interface that a `DecisionTask` uses to hold heterogeneous nodes. Reconciling the decision-node family (uniform interface, task wiring) with the new generic, struct-typed contracts is tracked as follow-up work; this spec describes `DecisionExpression` as a standalone generic construct.

It draws on two DMN boxed-expression forms: a single-output node is DMN's **literal expression** (one expression yielding one value), and a multi-output node is DMN's **boxed context** (named entries). `DecisionExpression` unifies them — a single-entry node is just the degenerate boxed context.

```go
type Entries map[string]string // output name -> raw-string source expression

type DecisionExpressionConfig struct {
    Id          string
    Name        string
    Description string

    // Entries maps each output name to its source expression. The key set must
    // be exactly the set of output names (O's expr-tag field names).
    Entries Entries
}

// I and O are concrete structs whose exported fields, renamed by `expr:"name"`
// tags, declare the inputs and outputs. NewDecisionExpression accumulates every
// construction problem and panics once with a *DecisionDefinitionError.
func NewDecisionExpression[I, O any](config DecisionExpressionConfig) *DecisionExpression[I, O]

func (d *DecisionExpression[I, O]) Evaluate(inputs I) (O, error)
func (d *DecisionExpression[I, O]) Source(output string) (string, bool)
func (d *DecisionExpression[I, O]) ToMarkdown() string
func (d *DecisionExpression[I, O]) GetId() string
func (d *DecisionExpression[I, O]) GetName() string
func (d *DecisionExpression[I, O]) GetDescription() string

// DecisionDefinitionError reports one or more construction problems.
type DecisionDefinitionError struct {
    Node     string
    Problems []string
}
```

`NewDecisionExpression` reflects `I` and `O` to learn the declared variable names (their `expr` tags), joins them into one combined env type (via `reflect.StructOf`, since Go forbids embedding type parameters), compiles each entry's raw-string source (one `expr.Compile` per entry) against that combined env, builds the intra-node dependency graph from sibling-output references, and topologically sorts. A malformed contract, an entry that does not compile, an entry referencing an undeclared name, or a cycle among entries is accumulated and raised as a `DecisionDefinitionError`.

---

## Building a DecisionExpression

### Single output

A single-output node is the expression-based way to author a one-value decision.

```go
type PaymentInputs struct {
    LoanAmount bl.BlNumber `expr:"loan_amount"`
    Rate       bl.BlNumber `expr:"rate"`
}
type PaymentOutputs struct {
    Amount bl.BlNumber `expr:"amount"`
}

var monthlyPayment = bl.NewDecisionExpression[PaymentInputs, PaymentOutputs](bl.DecisionExpressionConfig{
    Id:   "monthly_payment",
    Name: "Monthly Payment",
    Entries: bl.Entries{
        "amount": `loan_amount * rate / 12`,
    },
})

var amount, _ = bl.Number(200000)
var rate, _   = bl.Number("0.05")
var result, _ = monthlyPayment.Evaluate(PaymentInputs{LoanAmount: amount, Rate: rate})
// result is PaymentOutputs{Amount: <bl.BlNumber>}
```

### Conditional single output

```go
type ScoreInputs struct {
    Score bl.BlNumber `expr:"score"`
}
type StatusOutputs struct {
    Status bl.BlString `expr:"status"`
}

var applicationStatus = bl.NewDecisionExpression[ScoreInputs, StatusOutputs](bl.DecisionExpressionConfig{
    Id:   "status",
    Name: "Application Status",
    Entries: bl.Entries{
        "status": `if score >= 700 then "approved" else "review"`,
    },
})
```

### Multiple outputs with cross-entry references

```go
type LoanInputs struct {
    LoanAmount bl.BlNumber `expr:"loan_amount"`
    Rate       bl.BlNumber `expr:"rate"`
    Term       bl.BlNumber `expr:"term"`
}
type Breakdown struct {
    Principal bl.BlNumber `expr:"principal"`
    Interest  bl.BlNumber `expr:"interest"`
    Total     bl.BlNumber `expr:"total"`
}

var monthlyBreakdown = bl.NewDecisionExpression[LoanInputs, Breakdown](bl.DecisionExpressionConfig{
    Id:   "monthly_breakdown",
    Name: "Monthly Breakdown",
    Entries: bl.Entries{
        "principal": `loan_amount / term`,
        "interest":  `loan_amount * rate / 12`,
        "total":     `principal + interest`,
    },
})

var la, _   = bl.Number(120000)
var r, _    = bl.Number("0.06")
var term, _ = bl.Number(12)
var out, _  = monthlyBreakdown.Evaluate(LoanInputs{LoanAmount: la, Rate: r, Term: term})
// out is Breakdown{Principal: 10000, Interest: 600, Total: 10600}
```

The `total` entry references `principal` and `interest` by name. Those are this node's own outputs — referencing them declares cross-entry dependencies that `NewDecisionExpression` honours when sorting.

---

## Compiling entries into the evalPlan

`NewDecisionExpression` validates the contracts, compiles every entry's raw-string source into a `*vm.Program` — **one `expr.Compile` call per entry**, via the same engine path the rest of blkit uses (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)) — and assembles the sorted evaluation plan. It proceeds in four steps:

1. **Validate the contracts.** Each failure is a `DecisionDefinitionError`:
   - `I` and `O` are structs whose exported fields have usable `expr` names;
   - no name is duplicated, and no input name collides with an output name (the combined env would otherwise silently shadow one);
   - `O` declares at least one output;
   - the `Entries` key set is exactly the `O` output names — no entry without a matching output, and no output without an entry.
2. **Build the combined env type.** `I`'s and `O`'s fields are joined into one struct type (via `reflect.StructOf`, since a type parameter cannot be embedded) whose field tags are the declared FEEL names, so every entry may reference any declared input and any sibling output by name.
3. **Compile each entry.** Each entry's source is compiled with `expr.Compile` (once, at construction) against the combined env. Because that env is a strict struct env (plus blkit's own undefined-name pre-pass), a reference to a name that is neither a declared input nor a sibling output is rejected — a `bl.ParseError`, wrapped in a `DecisionDefinitionError`.
4. **Sort by dependency.** Each entry's intra-node dependencies are the sibling output names its source references (discovered by walking the parsed AST); the entries are topologically sorted by these edges. A self-reference or a reference cycle is a `DecisionDefinitionError`.

Compilation is therefore the point at which name discipline is enforced: `Evaluate` never sees an entry that references an unknown name.

The exported `Entries` map retains each entry's original raw source (used by `ToMarkdown` and `Source`). The compiled programs and their topological evaluation order are held unexported.

---

## Validation and type safety

A `DecisionExpression` gets its safety from two layers:

- **Go compile time.** The input and output contracts are concrete structs, so a caller passing the wrong input shape to `Evaluate`, or reading an output field that does not exist, fails `go build`.
- **Construction time.** The entry sources, names, and dependency graph live outside the Go type system (raw strings and `expr` tags), so they are checked when `NewDecisionExpression` runs. Following the decision-family convention, the constructor **accumulates every problem and panics once** with a `*DecisionDefinitionError`. Because a `DecisionExpression` is typically a package-scope `var`, a malformed node aborts the program (or its test binary) at startup — deterministic load-time fail-fast.

What each moment catches:

| Moment | Trigger | What it catches | Raised as |
|--------|---------|-----------------|-----------|
| **Go compilation** | `go build` | A caller passing an input value of the wrong type, or reading an undeclared output field. | Go type error |
| **Node construction** | `NewDecisionExpression` | A non-struct `I`/`O`; a duplicate or colliding `expr` name; an empty `O`; an `Entries` key set that is not exactly the output names; an entry that fails to compile or references an undeclared name; a dependency cycle. | `DecisionDefinitionError` |
| **Evaluation** | `Evaluate` | A produced value whose runtime type disagrees with its declared output field; a runtime operator type error inside an entry's expression. | `bl.TypeError` |

---

## Evaluation

`Evaluate` is stateless: the `DecisionExpression` is immutable after construction, and each call works against its own local combined-env value, so concurrent calls do not interfere.

Each call:

1. Builds a fresh combined-env value and copies the `inputs I` fields into it; the output fields start at their zero values.
2. Walks the topologically-sorted plan in order. Each entry's compiled program is run with `expr.Run` against the current combined env (the inputs plus the outputs of earlier steps), and the result is written into that output's field of the combined env — so later steps that reference it resolve against the value just produced. Writing a result whose runtime type disagrees with the declared output field is a `bl.TypeError`.
3. Copies the output fields out of the combined env into a fresh `O` and returns it.

Because the plan is topologically sorted, every name a step references is already populated by the time that step runs; a not-yet-produced output is never read. Cycles are rejected at construction and never observed during evaluation.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing each entry's name and its source expression, in output declaration order.

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

`[@test] ../../core/decision_expression_test.go`

---

## Edge Cases

- An `O` with no exported output fields is invalid; `NewDecisionExpression` raises `DecisionDefinitionError`.
- The `Entries` key set must be exactly the output names. An `Entries` key matching no output, or an output with no entry, is a `DecisionDefinitionError`.
- A duplicate `expr` name within `I` or `O`, or an input name that collides with an output name, is a `DecisionDefinitionError`.
- An entry source that does not compile is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- An entry that references a name that is neither a declared input nor a sibling output is a `DecisionDefinitionError` at construction.
- An entry referencing another entry's output name declares a cross-entry dependency; cycles are rejected at construction.
- An entry that evaluates to `bl.BlNull` is valid, provided the output field's type admits it.
- Entries with no dependencies on other entries may execute in any order relative to each other.
- An entry whose runtime value disagrees with its declared output field type produces a `bl.TypeError` at evaluation time.
