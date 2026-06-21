---
name: DecisionExpression
description: A DecisionNode that defines decision logic as named text-expression entries. Declares its input and output contracts as plain []Field data; each entry binds an output name to a bl-expression. Entries may reference declared inputs and sibling outputs, with automatic by-name dependency sorting. Evaluate takes named BlValues and returns named BlValues.
targets:
  - ../../core/decision_expression.go
---

# DecisionExpression

A `DecisionExpression` is a [`DecisionNode`](decision-node.spec.md) defined by a set of named entries. Each entry binds an output name to a value expression written in the [blkit expression language](../expressions/bl-expr.spec.md) — a raw-string source compiled via `bl.Expr`. Entries may reference the node's declared inputs and may reference one another by output name; the constructor topologically sorts entries by their inter-entry dependencies.

A `DecisionExpression` declares its contracts as plain data (see [decision-node.spec.md § Contracts are plain data](decision-node.spec.md#contracts-are-plain-data-not-go-generics)): `Inputs` is the list of named, typed variables it consumes from outside, and `Outputs` is the list of named, typed values it produces. There is one entry per output. There is no type parameter and no reflected outputs struct.

`DecisionExpression` is the expression-based sibling of [`DecisionTable`](decision-table.spec.md): both author a decision node with an input contract and an output contract, but a `DecisionTable` expresses logic as tabular rules whereas a `DecisionExpression` expresses it as named text expressions.

It draws on two DMN boxed-expression forms: a single-output node is DMN's **literal expression** (one expression yielding one value), and a multi-output node is DMN's **boxed context** (named entries). `DecisionExpression` unifies them — a single-entry node is just the degenerate boxed context.

```go
type Entries map[string]string // output name -> raw-string source expression

type DecisionExpression struct {
    Id          string
    Name        string
    Description string

    Entries Entries // original raw sources, keyed by output name (see § Compiling entries into the evalPlan)

    // inputs / outputs hold the declared contracts; exposed via the
    // DecisionNode interface methods Inputs() / Outputs().
    inputs  []Field
    outputs []Field

    // evalPlan holds each entry's compiled BlExpr paired with the output it
    // binds, in topological evaluation order; built by NewDecisionExpression
    // and walked by Evaluate.
    evalPlan []planStep
}

type planStep struct {
    output string // the output name this entry binds
    expr   BlExpr // the compiled source (bl.Expr result)
}

func NewDecisionExpression(config DecisionExpressionConfig) *DecisionExpression

type DecisionExpressionConfig struct {
    Id          string
    Name        string
    Description string

    // Inputs declares the named, typed variables the entries consume from
    // outside this node (task inputs, upstream node outputs, or reference data).
    Inputs []Field

    // Outputs declares the named, typed values this node produces. The Entries
    // key set must be exactly the set of Output names.
    Outputs []Field

    // Entries maps each output name to its source expression.
    Entries Entries
}

// DecisionNode interface satisfaction.
func (d *DecisionExpression) GetId() string
func (d *DecisionExpression) GetName() string
func (d *DecisionExpression) GetDescription() string
func (d *DecisionExpression) Inputs() []Field
func (d *DecisionExpression) Outputs() []Field

// Evaluate the entries against the input variables, returning a map keyed by
// this node's output names (see decision-node.spec.md).
func (d *DecisionExpression) Evaluate(input map[string]BlValue) (map[string]BlValue, error)

// Render as a markdown string
func (d *DecisionExpression) ToMarkdown() string
```

`NewDecisionExpression` validates the input and output contracts (well-formed names and types, no duplicates), checks that the `Entries` keys are exactly the `Outputs` names, compiles each entry source via `bl.Expr` against a schema built from the declared inputs plus sibling outputs, builds the intra-node dependency graph from sibling-output references, and topologically sorts. A malformed contract, an entry that does not compile, an entry referencing an undeclared name, or a cycle among entries is a `DecisionDefinitionError`.

---

## Building a DecisionExpression

### Single output

A single-output node is the expression-based way to author a one-value decision; `Evaluate` returns a one-entry map.

```go
var monthlyPayment = bl.NewDecisionExpression(bl.DecisionExpressionConfig{
    Id:   "monthly_payment",
    Name: "Monthly Payment",
    Inputs: []bl.Field{
        {Name: "loan_amount", Type: bl.TypeNumber},
        {Name: "rate", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "amount", Type: bl.TypeNumber},
    },
    Entries: bl.Entries{
        "amount": `loan_amount * rate / 12`,
    },
})

var result, err = monthlyPayment.Evaluate(map[string]bl.BlValue{
    "loan_amount": bl.Number(200000),
    "rate":        bl.Number(0.05),
})
// result is map[string]bl.BlValue{"amount": <bl.BlNumber>}
```

`loan_amount` and `rate` are declared inputs, resolved at task construction from the task's inputs or an upstream node's output. `amount` is the produced output; downstream nodes consume it by declaring an input named `amount`.

### Conditional single output

```go
var applicationStatus = bl.NewDecisionExpression(bl.DecisionExpressionConfig{
    Id:   "status",
    Name: "Application Status",
    Inputs: []bl.Field{
        {Name: "score", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "status", Type: bl.TypeString},
    },
    Entries: bl.Entries{
        "status": `if score >= 700 then "approved" else "review"`,
    },
})
```

`score` is a declared input, resolved from an upstream node's output (e.g. a `credit_check` node) or a task input.

### Multiple outputs with cross-entry references

```go
var monthlyBreakdown = bl.NewDecisionExpression(bl.DecisionExpressionConfig{
    Id:   "monthly_breakdown",
    Name: "Monthly Breakdown",
    Inputs: []bl.Field{
        {Name: "loan_amount", Type: bl.TypeNumber},
        {Name: "rate", Type: bl.TypeNumber},
        {Name: "term", Type: bl.TypeNumber},
    },
    Outputs: []bl.Field{
        {Name: "principal", Type: bl.TypeNumber},
        {Name: "interest", Type: bl.TypeNumber},
        {Name: "total", Type: bl.TypeNumber},
    },
    Entries: bl.Entries{
        "principal": `loan_amount / term`,
        "interest":  `loan_amount * rate / 12`,
        "total":     `principal + interest`,
    },
})

var result, err = monthlyBreakdown.Evaluate(map[string]bl.BlValue{
    "loan_amount": bl.Number(120000),
    "rate":        bl.Number(0.06),
    "term":        bl.Number(12),
})
// result is map[string]bl.BlValue{"principal": 10000, "interest": 600, "total": 10600}
```

The `total` entry references `principal` and `interest` by name. Those are this node's own outputs — referencing them declares cross-entry dependencies that `NewDecisionExpression` honours when sorting. `loan_amount`, `rate`, and `term` are declared inputs.

---

## Compiling entries into the evalPlan

`NewDecisionExpression` validates the node's contracts, compiles every entry's raw-string source into a `bl.BlExpr` (via `bl.Expr` — the same compiler the rest of blkit uses, see [bl-expr.spec.md](../expressions/bl-expr.spec.md)), and assembles the sorted `evalPlan`. It proceeds in four steps:

1. **Validate the contracts.** Each failure is a `DecisionDefinitionError`:
   - `Inputs` and `Outputs` are each well-formed: every `Field` has a valid name and type;
   - no name is duplicated within `Inputs`, nor within `Outputs`;
   - `Outputs` is non-empty;
   - the `Entries` key set is exactly the `Outputs` names — no entry without a matching output, and no output without an entry.
2. **Build one node schema.** A single `bl.BlSchema` is built for the whole node from the union of the declared `Inputs` and `Outputs` fields. Every entry compiles against this same schema, so any entry may reference any declared input and any sibling output by name.
3. **Compile each entry.** For each entry, `bl.Expr(source, schema)` is called with the entry's raw source and the node schema. Because the schema is non-nil, this both compiles the source to a `bl.BlExpr` and statically rejects any reference to a name that is neither a declared input nor a sibling output. A source that fails to compile — a syntax error or an undefined name — returns a `bl.ParseError`, which `NewDecisionExpression` wraps in a `DecisionDefinitionError`.
4. **Sort by dependency.** Each entry's intra-node dependencies are the sibling output names its source references; the entries are topologically sorted by these edges. A self-reference or a reference cycle is a `DecisionDefinitionError`.

Compilation is therefore the point at which name discipline is enforced: `Evaluate` never sees an entry that references an unknown name.

The exported `Entries` map retains each entry's original raw source (used by `ToMarkdown` and for inspection). The compiled `bl.BlExpr` values and their topological evaluation order are held in the unexported `evalPlan` field — a slice of `planStep` in evaluation order, populated by `NewDecisionExpression`. (A Go map cannot itself carry the sort order, so the order lives in the slice rather than in `Entries`.)

---

## Validation and type safety

A `DecisionExpression` carries its own contracts as plain data, so safety is contract-matching, not name-inference. Following the family rule (see [decision-node.spec.md § Where type-safety happens](decision-node.spec.md#where-type-safety-happens)), the checks are concentrated at **construction** — the mental model is *if construction does not complain, the node is well-formed* — with only value-versus-declaration correctness left to evaluation.

These are **runtime** checks, not Go compile-time ones. "Construction" means the moment the `NewDecisionExpression` (or `NewDecisionTask`) constructor executes; a `DecisionExpression`'s contracts are `[]Field` data and its entries are raw strings, all outside the Go type system, so a malformed node is rejected when its constructor runs, never by `go build`. Likewise "an entry compiles" means `bl.Expr` parses its expression-language source — also at construction — not Go compilation.

`NewDecisionExpression` does not return an error: following the decision-family convention, it accumulates every construction-time problem and **panics once** with a `*DecisionDefinitionError`. This is what makes the package-scope pattern below work — a fallible constructor would force every declaration into an error-handling function.

Because a `DecisionExpression` is typically declared as a package-scope `var` — including inside a package the application author writes — its construction runs during that package's **initialisation**, when the program (or its test binary) starts, before `main`. A malformed node therefore aborts the program at startup: the panic lists each problem and the stack trace pins the offending declaration. This is not compile-time safety, but it is deterministic **load-time fail-fast** — any run of the program, or any test that merely imports the declaring package, surfaces every construction error, regardless of whether a code path later evaluates the node.

Three moments catch three distinct classes of problem:

| Moment | Trigger | What it catches | Raised as |
|--------|---------|-----------------|-----------|
| **Node construction** | `NewDecisionExpression` | A malformed contract (an ill-formed `Inputs`/`Outputs` name or type, a duplicate name within either list, an empty `Outputs`, or an `Entries` key set that is not exactly the `Outputs` names); an entry that fails to compile; an entry referencing a name that is neither a declared input nor a sibling output; a dependency cycle among entries. | `DecisionDefinitionError` |
| **Task construction** | `NewDecisionTask` | A declared input with no producer of matching name **and** declared type; an output name or `Id` that collides with another node in the task; a cross-node cycle. | `DecisionDefinitionError` |
| **Evaluation** | `Evaluate` | A supplied input value, or a produced output value, whose runtime type disagrees with its declared `Field.Type`; a runtime operator type error inside an entry's expression. | `bl.TypeError` |

**Node construction** is detailed in [§ Compiling entries into the evalPlan](#compiling-entries-into-the-evalplan): it checks the one node in isolation — contract structure, that every entry compiles, that every referenced name is declared, and that the dependency graph is acyclic.

**Task construction** checks the whole graph and is detailed in [decision-task.spec.md § Wiring](decision-task.spec.md#wiring): it matches each declared input to a producer (an upstream node output, a task input, or reference data) by name and declared type, and draws the cross-node edges. A standalone node — evaluated with no containing task — skips this moment entirely; the caller is then responsible for supplying inputs of the right type.

**Evaluation** is the only moment that inspects *values*. The expression engine is runtime-typed — operators dispatch on operand types at evaluation, not compile time — so a value that disagrees with a declared type cannot be caught earlier and surfaces here as a `bl.TypeError` (see [§ Evaluation](#evaluation)). Construction guarantees the declarations are mutually consistent; evaluation guarantees the values honour them.

---

## Evaluation

`Evaluate` is stateless: the `DecisionExpression` is immutable after construction, and each call works against its own local scope, so concurrent calls do not interfere.

Each call builds one fresh scope, `decisionEvalScope`, and mutates it in place as it goes. The scope is a `BlDictionary`: that is the one `BlValue` shape a compiled `bl.BlExpr` spreads into named variables when evaluated (any other shape binds to the unary-test placeholder instead — see [bl-expr.spec.md](../expressions/bl-expr.spec.md)). The `map[string]BlValue` of the `Evaluate` signature is just the API boundary; internally it is carried as this dictionary.

`decisionEvalScope` is seeded with **only** the supplied input variables; output keys are not pre-populated — each is added when its step runs, never as a placeholder `BlNull` beforehand. The `evalPlan` slice is already topologically sorted by `NewDecisionExpression`, so `Evaluate` walks it in order:

1. Each `planStep`'s compiled `expr` is evaluated against the current `decisionEvalScope` (the inputs plus the outputs of earlier steps already added).
2. The result is set into the same `decisionEvalScope` under that step's `output` name — a single in-place write that adds the key, not a copy of the dictionary — so later steps that reference it resolve against the value just produced. A step that produces `BlNull` sets its key to `BlNull` explicitly.
3. Once the walk is complete — after the final step has run — the result is **projected** out of `decisionEvalScope`: a new `map[string]BlValue` is built by iterating the node's declared output names (`d.outputs`), reading each value from the scope, and validating it against that output's declared `Field.Type` — a mismatch returns a `bl.TypeError`. Because the projection iterates output names rather than scope keys, the seeded inputs (and any other scope key) are never copied into the result; only the declared outputs are returned.

Because the plan is topologically sorted, every name a step references is already a key in `decisionEvalScope` by the time that step runs; a not-yet-produced output is simply absent, never a silent `BlNull`. Cycles in the dependency graph are rejected at construction time and never observed during evaluation.

### Standalone vs. within a task

The input map handed to `Evaluate` is the same map of `Inputs()` values regardless of how the node is driven; only its source differs:

- **Standalone** — with no containing task, the caller supplies every value the node's `Inputs()` declare directly to `Evaluate`.
- **Within a `DecisionTask`** — the task resolves each declared input from the producer it was wired to at task construction (an upstream node output, a task input, or reference data) and passes the assembled map to `Evaluate`.

Either way `Evaluate` behaves identically: it seeds `decisionEvalScope` from the supplied inputs and walks the `evalPlan` as above.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing each entry's name and its source expression, in `Outputs` declaration order.

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

- A `DecisionExpression` whose `Outputs` is empty is invalid; `NewDecisionExpression` raises `DecisionDefinitionError`.
- The `Entries` key set must be exactly the `Outputs` names. An `Entries` key matching no output, or an output with no entry, is a `DecisionDefinitionError`.
- A duplicate name within `Inputs` or within `Outputs` is a `DecisionDefinitionError`.
- An entry source that does not compile via `bl.Expr` is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- An entry that references a name that is neither a declared input nor a sibling output is a `DecisionDefinitionError` at construction.
- An entry referencing another entry's output name declares a cross-entry dependency; cycles are rejected at construction.
- An entry that evaluates to `bl.BlNull` is valid; dependent entries can still reference its output name (it resolves to `bl.BlNull`).
- Entries with no dependencies on other entries may execute in any order relative to each other.
- An entry whose runtime value disagrees with its declared output type produces a `bl.TypeError` at evaluation time.
