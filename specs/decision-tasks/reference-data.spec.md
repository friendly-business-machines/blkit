---
name: ReferenceData
description: A static value source generic over T BlValue — carries identity (Id, Name, Description) and a single constant Value. Registered with a DecisionTask via WithReferenceData and consumed by nodes that declare an input whose name is the reference data's Id; it computes nothing and is never evaluated.
targets:
  - ../../core/reference_data.go
---

# ReferenceData

A `ReferenceData` is a static **value source** within a decision model: a single
constant `BlValue` paired with identity (`Id`, `Name`, `Description`). It is
generic over the value's type `T`.

`ReferenceData` is **not** a [`DecisionNode`](decision-node.spec.md) and has **no
`Evaluate` method** — a constant computes nothing, so it carries its value rather
than deriving one. It is registered with a `DecisionTask` at construction via
`bl.WithReferenceData(...)`. A node consumes it by declaring an input whose
**name is the reference data's `Id`**; the task matches that input to the
reference value during wiring and binds the value into the evaluation context
under the same `Id` before nodes run. A `ReferenceData` never appears in a
`DecisionTask`'s `[]DecisionNode` or its evaluation order.

Reference data is part of a task's **decision logic**, so a `DecisionTask` clone
inherits it **by reference** — it is never reset or re-added (see
[decision-task.spec.md § Reuse via Clone](decision-task.spec.md#reuse-via-clone)).

`T` may be any `BlValue` — a scalar (`BlNumber`, `BlString`, `BlDate`,
`BlBoolean`, …) or a composite (`BlTable`, `BlList`, `BlDictionary`). A `BlTable`
`ReferenceData` is conceptually DMN's *relation* — a list of uniformly-keyed
rows held as inline reference data within the decision model. It holds a
**static**, pre-built table; for a table whose cells are *computed* from task
inputs or upstream outputs, use a [`DecisionExpression`](decision-expression.spec.md)
with a single `BlTable` output built via the `table(...)` constructor (see
[table.spec.md](../expressions/table.spec.md)).

```go
type ReferenceData[T BlValue] struct {
    Id          string
    Name        string
    Description string

    Value T // the static constant
}

// ReferenceValue is the non-generic view of a ReferenceData, so a DecisionTask
// can hold a heterogeneous set of value sources and bind each into the
// evaluation context by Id. Every ReferenceData[T] satisfies it.
type ReferenceValue interface {
    GetId() string
    GetName() string
    GetDescription() string
    GetValue() BlValue
}

// referenceDataConfig is the unexported staging struct the options write into.
// It embeds the package-shared identity (see decision-task.spec.md § Shared
// identity options), so the shared WithId / WithName / WithDescription options
// configure it.
type referenceDataConfig struct{ identity }

type ReferenceDataOption = func(*referenceDataConfig)

// NewReferenceData builds a ReferenceData from the mandatory constant value plus
// identity options. value is positional — it is the entire purpose of a
// ReferenceData — which also lets type inference supply T, so the call site needs
// no explicit [T]. Definition failures (empty Id, nil Value) panic with a
// *DecisionDefinitionError, per the family's panicking-constructor convention.
func NewReferenceData[T BlValue](value T, opts ...ReferenceDataOption) *ReferenceData[T]

// Render as a markdown string
func (r *ReferenceData[T]) ToMarkdown() string
```

`ReferenceData` has no `Evaluate` method and does not satisfy the `DecisionNode`
interface. Its only options are the shared identity options
(`bl.WithId`/`bl.WithName`/`bl.WithDescription`); it has no type-specific options.

---

## Building a ReferenceData

```go
var taxRate = bl.NewReferenceData(bl.Number(0.2),
    bl.WithId("tax_rate"),
    bl.WithName("Tax Rate"),
)

var baseCurrency = bl.NewReferenceData(bl.String("GBP"),
    bl.WithId("base_currency"),
    bl.WithName("Base Currency"),
)
```

The positional `value` lets type inference supply `T`, so neither call needs an
explicit `[BlNumber]` / `[BlString]`.

A composite value — a static lookup table — is held directly:

```go
var shippingRatesTable, _ = bl.Table(
    bl.Cols{
        {Name: "region", Type: bl.TypeString},
        {Name: "rate", Type: bl.TypeNumber},
    },
    bl.Row{"domestic", 5.99},
    bl.Row{"europe", 15.99},
    bl.Row{"international", 25.99},
)

var shippingRates = bl.NewReferenceData(shippingRatesTable,
    bl.WithId("shipping_rates"),
    bl.WithName("Shipping Rates"),
)
```

`bl.Table` is independently fallible (`(BlTable, error)`), so a malformed table
surfaces at its own declaration before reaching `bl.NewReferenceData`.

---

## Registering and consuming

A `ReferenceData` is registered with the `DecisionTask` that uses it, via
`bl.WithReferenceData`. A node that needs the constant declares an **input whose
name is the reference data's `Id`** and whose type is the value's type. During
wiring `NewDecisionTask` matches that declared input to the registered reference
value (by name and type), and at evaluation it binds the `Value` into the
context under the same `Id`. The name a node uses is therefore the reference
data's `Id` string — **not** the Go variable it was assigned to. Here the
constant was declared `var taxRate = bl.NewReferenceData(bl.Number(0.2),
bl.WithId("tax_rate"), …)`, so its `Id` is `"tax_rate"`, the consuming node
declares an input `tax_rate` of `TypeNumber`, and its expression references
`tax_rate`:

```go
var grossPrice = bl.NewDecisionExpression(bl.DecisionExpressionConfig{
    Id:   "gross_price",
    Name: "Gross Price",
    Inputs: []bl.Field{
        {Name: "net_price", Type: bl.TypeNumber}, // an upstream node's output
        {Name: "tax_rate", Type: bl.TypeNumber},  // reference data, by its Id
    },
    Outputs: []bl.Field{
        {Name: "gross_price", Type: bl.TypeNumber},
    },
    Entries: bl.Entries{
        "gross_price": `net_price * (1 + tax_rate)`,
    },
})

// The task's external input contract — what the graph consumes from callers.
// (net_price is an internal node output; tax_rate is reference data — neither is
// part of InputSchema.)
var pricingInputs, _ = bl.Schema(
    bl.Field{Name: "list_price", Type: bl.TypeNumber},
)

var pricing = bl.NewDecisionTask(
    bl.WithId("pricing"),
    bl.WithName("Pricing"),
    bl.WithNodes(netPrice, grossPrice),
    bl.WithInputSchema(pricingInputs),
    bl.WithReferenceData(taxRate), // registers the Go value taxRate, whose Id is "tax_rate"
)
```

So the `tax_rate` input on `grossPrice` resolves because `bl.WithReferenceData(taxRate)`
makes a value source named `tax_rate` (type `BlNumber`) available to the wiring
matcher, and binds its value into the evaluation context under that `Id`. A
`ReferenceData` need not — and cannot — be listed in `[]bl.DecisionNode`.

---

## Resolution

A `ReferenceData` is never evaluated. The `DecisionTask` it is registered with
binds its `Value` into the evaluation context under its `Id` before nodes run.
Because the source is a constant rather than a node in the evaluation order,
consuming it forms no node-to-node dependency edge — a `ReferenceData` is always
a leaf with no upstream dependencies. Being decision-logic, it is shared by
reference with a task's clones and is never reset.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown string showing the name (or `Id`), the optional
description, and the value rendered via `BlValue.String()`.

```go
fmt.Println(taxRate.ToMarkdown())
```

Output:

```text
### Tax Rate

**Value:** `0.2`
```

Rendering a value source is useful for documenting a task's reference data.
Integration into `DecisionTask.ToMarkdown()` is out of scope for this spec.

---

## Edge Cases

- A `ReferenceData` whose `Id` is an empty string is invalid; `bl.NewReferenceData`
  raises `DecisionDefinitionError`.
- A `ReferenceData` whose `Value` is nil is invalid; `bl.NewReferenceData` raises
  `DecisionDefinitionError`.
- A `Value` of `bl.BlNull` is a valid constant.
- A `ReferenceData` has no `Evaluate` method and cannot be placed in a
  `DecisionTask`'s `[]bl.DecisionNode`.
- A `ReferenceData` is always a leaf — it references no other node and has no
  upstream dependencies.
- A `ReferenceData` `Id` that collides with a node output name, another
  `ReferenceData` `Id`, or an `InputSchema` variable name in the owning task is a
  `DecisionDefinitionError`, raised by `bl.NewDecisionTask` (the three together
  form one shared, name-keyed value namespace).
- A node input whose name matches no registered reference data, task input, or
  upstream output is unresolved — `bl.NewDecisionTask` reports it as a definition
  error at construction.
- A node input that matches a reference data by name but whose declared type
  differs from the value's type is a `DecisionDefinitionError`.
- A `DecisionTask` clone inherits its source's reference data by reference; it is
  not reset, and `bl.WithReferenceData` is not a valid `Clone` option (decision
  logic is shared from the source).
