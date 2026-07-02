---
name: DecisionExpression
description: A generic decision construct defining decision logic as named text-expression entries over concrete Go input (I) and output (O) structs. Each entry binds an output field to a bl-expression; entries may reference declared inputs and sibling outputs by name, with automatic by-name dependency sorting. Evaluate(inputs I) returns O, type-checked at Go compile time.
targets:
  - ../../core/decision_expression.go
---

# DecisionExpression

A `DecisionExpression[I, O]` is a [`DecisionNode[I, O]`](decision-node.spec.md) that defines decision logic as a set of named entries over two concrete Go structs: an **input** struct `I` and an **output** struct `O`. Each entry binds one output to a value expression written in the [blkit expression language](../expressions/bl-expr.spec.md) — a raw-string source compiled via `bl.Expr`. Entries may reference the declared inputs, may reference one another by output name, and may call any [user-defined functions](../expressions/udf.spec.md) supplied in `Config.Funcs`; the constructor topologically sorts entries by their inter-entry dependencies.

Because the input and output contracts are concrete Go structs, a caller that passes the wrong input shape or reads a non-existent output gets a **Go compile error**, and `Evaluate(inputs I) (O, error)` carries the result back as a typed struct. Every exported field of `I` and `O` is a variable an entry may reference — by its Go field name, or by the name given in an optional `expr:"name"` tag — and each field must be a `bl.Handle[T]` whose `T` is a `BlValue`, following the family's [reflection contract](decision-node.spec.md#contracts-are-concrete-go-structs). Like every node it exposes `In`/`Out` handle surfaces so it can be wired into a [`DecisionTask`](decision-task.spec.md#wiring); the entry compilation and topological sort below are its kind-specific structure.

It draws on two DMN boxed-expression forms: a single-output node is DMN's **literal expression** (one expression yielding one value), and a multi-output node is DMN's **boxed context** (named entries). `DecisionExpression` unifies them — a single-entry node is just the degenerate boxed context.

```go
type Entries map[string]string // output name -> raw-string source expression

type DecisionExpressionConfig struct {
    Id          string
    Name        string
    Description string

    // Entries maps each output name to its source expression. The key set must
    // be exactly the set of output variable names declared by O.
    Entries Entries

    // Funcs are user-defined functions (see udf.spec.md) that every entry may
    // call by name, with compile-time-checked arguments. Two Funcs sharing a
    // name is a construction error.
    Funcs []UDF
}

// I and O are concrete structs whose exported fields declare the inputs and
// outputs; each field must be a bl.Handle[BlValue] and is exposed under its Go
// field name, or under the name in an optional `expr:"name"` tag.
// NewDecisionExpression accumulates every construction problem and panics once
// with a *DecisionDefinitionError. The node exposes In/Out handle surfaces for
// wiring (see decision-node.spec.md § Port surfaces).
func NewDecisionExpression[I, O any](config DecisionExpressionConfig) *DecisionExpression[I, O]

// DecisionNode[I, O] interface satisfaction.
func (d *DecisionExpression[I, O]) Evaluate(inputs I) (O, error)
func (d *DecisionExpression[I, O]) GetId() string
func (d *DecisionExpression[I, O]) GetName() string
func (d *DecisionExpression[I, O]) GetDescription() string
func (d *DecisionExpression[I, O]) Inputs() []Field  // reflected from I
func (d *DecisionExpression[I, O]) Outputs() []Field // reflected from O

func (d *DecisionExpression[I, O]) Source(output string) (string, bool)
func (d *DecisionExpression[I, O]) ToMarkdown() string

// DecisionDefinitionError reports one or more construction problems.
type DecisionDefinitionError struct {
    Node     string
    Problems []string
}
```

---

## Construction

`NewDecisionExpression` does all of its validation and compilation up front — when you construct the node, not when you later evaluate it. It compiles every entry's raw-string source into an executable program (a compiled `*vm.Program`) — **one `expr.Compile` call per entry**, through the same expression engine the rest of blkit uses (see [bl-expr.spec.md](../expressions/bl-expr.spec.md)) — and assembles the sorted plan that `Evaluate` later runs. It proceeds in four phases.

### 1. Inspect the contracts

Because `NewDecisionExpression[I, O]` is generic, it does not know `I`'s and `O`'s fields when it is written — they depend on the concrete structs each caller supplies. So it *reflects* over them: Go's `reflect` package lets code examine a value's type at runtime, walking each struct's fields to read their names, types, and `expr:"..."` tags. That is how the constructor discovers the declared input and output variables without you listing them separately.

**Every field is a variable.** Each field of `I` and `O` becomes one variable in the environment — there is no opt-out. By default a field's variable name is its Go field name; the `expr:"..."` tag is *optional* and is used only to expose a field under a different name (for example a Go field `PerParcel` mapped to the variable `per_parcel`). So a struct of plainly-named fields needs no tags at all.

It then checks the contracts against the family's [reflection contract](decision-node.spec.md#contracts-are-concrete-go-structs) — `I`/`O` are structs, every field an exported `bl.Handle[BlValue]` with a valid identifier name, each variable read through `Handle.Get()` — plus these expression-specific rules. Each failure is reported as a `DecisionDefinitionError`:

- no input name may collide with an output name — unlike most nodes, an entry's environment combines the inputs and the sibling outputs (see the next phase), so a collision would silently shadow one of them;
- `O` must declare at least one output;
- the `Entries` key set must be exactly the `O` output names — no entry without a matching output, and no output without an entry.

> **Why construction time, not compile time?** Go's generics can constrain a *type parameter*, but there is no constraint that says "a struct all of whose fields are `bl.Handle[BlValue]`" — field types cannot be expressed in the constraint system. So `[I, O any]` is the tightest signature available, and the `Handle` rule is enforced by reflection when `NewDecisionExpression` runs. Because a `DecisionExpression` is typically a package-scope `var`, that check still fails at program (or test-binary) startup — the same load-time fail-fast as every other contract rule here.

### 2. Combine the inputs and outputs into one environment

An *environment* is the set of names an expression is allowed to mention, each bound to a value — the variables in scope. Every entry is compiled and evaluated against a single combined environment built from **every input field plus every output field**, keyed by their declared variable names (each field's Go name or its `expr` rename). Because inputs and sibling outputs live in the same environment, any entry may reference any input or any other output by name.

Concretely, the combined environment is a struct type assembled at runtime with `reflect.StructOf`, joining `I`'s and `O`'s fields. It has to be built dynamically because Go does not allow a generic type parameter like `I` or `O` to be embedded in a struct directly. Each combined field is tagged with its declared variable name, so the expression engine resolves those names to the right fields.

### 3. Compile each entry

Each entry's raw-string source is compiled once, with `expr.Compile`, against the combined environment — and with `Config.Funcs` registered, so an entry may call those user-defined functions by name — producing the `*vm.Program` that `Evaluate` will run. (Two `Funcs` sharing a name is rejected here, before any entry is compiled.)

The environment is *strict*, and blkit adds its own pre-pass that flags any name an expression uses but does not declare. So a name that is neither a declared input nor a sibling output — an *undeclared name*, typically a typo — is rejected at compile time rather than silently resolving to an empty value. It surfaces as a `bl.ParseError`, wrapped in a `DecisionDefinitionError`. Compilation is therefore the point at which name discipline is enforced: `Evaluate` never sees an entry that references an unknown name.

### 4. Order the entries by dependency

An entry may reference this node's own outputs — for example a `total` entry that reads `per_parcel` and `freight`. Those references are the entry's *intra-node dependencies*, discovered by walking the entry's parsed expression (its AST — the tree the parser produces from the source). The entries are then *topologically sorted* by these dependencies: ordered so that every entry runs after the sibling outputs it reads. A self-reference, or a dependency cycle (A needs B while B needs A), cannot be ordered and is reported as a `DecisionDefinitionError`.

### After the four phases

The exported `Entries` map still holds each entry's original raw source (used by `ToMarkdown` and `Source`); the compiled programs and their topological evaluation order are kept internally (unexported).

Every problem found across these phases is **accumulated and raised together** as a single `DecisionDefinitionError`, rather than failing on the first — see [Validation and type safety](#validation-and-type-safety).

---

## Building a DecisionExpression

### Single output

A single-output node is the expression-based way to author a one-value decision.

```go
type QuoteInputs struct {
    Weight bl.Handle[bl.BlNumber] `expr:"weight"`
    Rate   bl.Handle[bl.BlNumber] `expr:"rate"`
}
type QuoteOutputs struct {
    Amount bl.Handle[bl.BlNumber] `expr:"amount"`
}

var freightQuote = bl.NewDecisionExpression[QuoteInputs, QuoteOutputs](bl.DecisionExpressionConfig{
    Id:   "freight_quote",
    Name: "Freight Quote",
    Entries: bl.Entries{
        "amount": `weight * rate / 12`,
    },
})

var weight, _ = bl.Number(200000)
var rate, _   = bl.Number("0.05")
var result, _ = freightQuote.Evaluate(QuoteInputs{
    Weight: bl.NewHandle(weight),
    Rate:   bl.NewHandle(rate),
})
// result.Amount.Get() is a bl.BlNumber
```

### Conditional single output

```go
type ScoreInputs struct {
    Score bl.Handle[bl.BlNumber] `expr:"score"`
}
type StatusOutputs struct {
    Status bl.Handle[bl.BlString] `expr:"status"`
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
type ShipmentInputs struct {
    Weight  bl.Handle[bl.BlNumber] `expr:"weight"`
    Rate    bl.Handle[bl.BlNumber] `expr:"rate"`
    Parcels bl.Handle[bl.BlNumber] `expr:"parcels"`
}
type Breakdown struct {
    PerParcel bl.Handle[bl.BlNumber] `expr:"per_parcel"`
    Freight   bl.Handle[bl.BlNumber] `expr:"freight"`
    Total     bl.Handle[bl.BlNumber] `expr:"total"`
}

var freightBreakdown = bl.NewDecisionExpression[ShipmentInputs, Breakdown](bl.DecisionExpressionConfig{
    Id:   "freight_breakdown",
    Name: "Freight Breakdown",
    Entries: bl.Entries{
        "per_parcel": `weight / parcels`,
        "freight":    `weight * rate / 12`,
        "total":      `per_parcel + freight`,
    },
})

var w, _       = bl.Number(120000)
var r, _       = bl.Number("0.06")
var parcels, _ = bl.Number(12)
var out, _     = freightBreakdown.Evaluate(ShipmentInputs{
    Weight:  bl.NewHandle(w),
    Rate:    bl.NewHandle(r),
    Parcels: bl.NewHandle(parcels),
})
// out.PerParcel.Get()=10000, out.Freight.Get()=600, out.Total.Get()=10600
```

The `total` entry references `per_parcel` and `freight` by name. Those are this node's own outputs — referencing them declares cross-entry dependencies that `NewDecisionExpression` honours when sorting.

### Calling user-defined functions

`Config.Funcs` holds [user-defined functions](../expressions/udf.spec.md) — host-defined, named, typed functions built with `bl.Func`. Every entry may call any of them by name, with arguments checked at construction against the function's declared parameters.

```go
type TaxParams struct {
    Amount bl.BlNumber `expr:"amount"`
}
var addTax, _ = bl.Func[TaxParams, bl.BlNumber]("addTax", `amount * 1.2`)

type PriceInputs struct {
    Base bl.Handle[bl.BlNumber] `expr:"base"`
}
type PriceOutputs struct {
    Gross        bl.Handle[bl.BlNumber] `expr:"gross"`
    WithShipping bl.Handle[bl.BlNumber] `expr:"with_shipping"`
}

var grossPrice = bl.NewDecisionExpression[PriceInputs, PriceOutputs](bl.DecisionExpressionConfig{
    Id:   "gross_price",
    Name: "Gross Price",
    Entries: bl.Entries{
        "gross":         `addTax(base)`, // calls the UDF
        "with_shipping": `gross + 5`,    // references the sibling output
    },
    Funcs: []bl.UDF{addTax},
})

var base, _ = bl.Number(100)
var out, _  = grossPrice.Evaluate(PriceInputs{Base: bl.NewHandle(base)})
// out.Gross.Get()=120, out.WithShipping.Get()=125
```

The functions are registered once, at construction, when each entry is compiled — so a call to a function that is not in `Funcs`, or a call with the wrong argument types, fails as a `DecisionDefinitionError` rather than at evaluation. Two `Funcs` sharing a name is also a `DecisionDefinitionError`.

---

## Validation and type safety

A `DecisionExpression` gets its safety from two layers:

- **Go compile time.** The input and output contracts are concrete structs, so a caller passing the wrong input shape to `Evaluate`, or reading an output field that does not exist, fails `go build`.
- **Construction time.** The entry sources, names, and dependency graph live outside the Go type system (raw strings and `expr` tags), so they are checked when `NewDecisionExpression` runs. Following the decision-family convention, the constructor **accumulates every problem and panics once** with a `*DecisionDefinitionError`. Because a `DecisionExpression` is typically a package-scope `var`, a malformed node aborts the program (or its test binary) at startup — deterministic load-time fail-fast.

What each moment catches, one per phase — compile, runtime init, and runtime:

| Phase | Moment | Trigger | What it catches | Raised as |
|-------|--------|---------|-----------------|-----------|
| **Compile** | Go compilation | `go build` | A caller passing an input value of the wrong type, or reading an undeclared output field. | Go type error |
| **Runtime init** | Node construction | `NewDecisionExpression` | A non-struct `I`/`O`; an unexported field; a field that is not a `bl.Handle[BlValue]`; a variable name that is not a valid expr identifier; a duplicate or colliding name; two `Funcs` sharing a name; an empty `O`; an `Entries` key set that is not exactly the output names; an entry that fails to compile, references an undeclared name, or calls an unregistered function; a dependency cycle. | `DecisionDefinitionError` |
| **Runtime** | Evaluation | `Evaluate` | A produced value whose runtime type disagrees with its declared output field; a runtime operator type error inside an entry's expression. | `bl.TypeError` |

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

`ToMarkdown()` returns a markdown string with two parts:

- the **input variables**, listed (not tabulated);
- a single **Name / Expression** table: first the node's user-defined functions, each shown by its call signature (e.g. `addTax(amount)`) and body, then the entries (each output's name and source expression, in output declaration order, with the source rendered verbatim so a UDF call shows as written).

```go
fmt.Println(grossPrice.ToMarkdown())
```

Output:

```text
### Gross Price

**Inputs:** base

| Name           | Expression   |
|----------------|--------------|
| addTax(amount) | amount * 1.2 |
| gross          | addTax(base) |
| with_shipping  | gross + 5    |
```

`[@test] ../../core/decision_expression_test.go`

---

## Edge Cases

- An `O` with no exported output fields is invalid; `NewDecisionExpression` raises `DecisionDefinitionError`.
- The `Entries` key set must be exactly the output names. An `Entries` key matching no output, or an output with no entry, is a `DecisionDefinitionError`.
- The `expr` tag is optional — an untagged field is exposed under its Go field name; a tag only renames the variable.
- An unexported field of `I` or `O` is a `DecisionDefinitionError` — every field must be an exported `bl.Handle[BlValue]` variable. There is no opt-out; `expr:"-"` is rejected rather than excluding the field.
- A field whose type is not a `bl.Handle[BlValue]` is a `DecisionDefinitionError` at construction.
- A variable name (a field name, or its `expr` tag) that is not a valid expr identifier — a letter or `_` followed by letters, digits, or `_` — is a `DecisionDefinitionError`.
- A duplicate variable name within `I` or `O`, or an input name that collides with an output name, is a `DecisionDefinitionError`.
- An entry source that does not compile is a `DecisionDefinitionError` (wrapping the `bl.ParseError`).
- An entry that references a name that is neither a declared input nor a sibling output is a `DecisionDefinitionError` at construction.
- An entry referencing another entry's output name declares a cross-entry dependency; cycles are rejected at construction.
- An entry may call any function in `Config.Funcs` by name. Two `Funcs` sharing a name, or an entry calling a function not in `Funcs` (or with the wrong argument types), is a `DecisionDefinitionError` at construction.
- An entry that evaluates to `bl.BlNull` is valid, provided the output field's type admits it.
- Entries with no dependencies on other entries may execute in any order relative to each other.
- An entry whose runtime value disagrees with its declared output field type produces a `bl.TypeError` at evaluation time.
