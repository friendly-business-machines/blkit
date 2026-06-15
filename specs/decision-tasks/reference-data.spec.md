---
name: ReferenceData
description: A static value source generic over T BlValue — carries identity (Id, Name, Description) and a single constant Value. Registered with a DecisionTask via WithReferenceData and referenced from node expressions by its Id; it computes nothing and is never evaluated.
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
`bl.WithReferenceData(...)`; the task binds its `Value` into the evaluation
context under its `Id`, so decision nodes reference it **by name** in their
expression bodies, exactly as they reference any other in-scope variable. It is
not a node and never appears in a `DecisionTask`'s `[]DecisionNode` or its
evaluation order.

Reference data is part of a task's **decision logic**, so a `DecisionTask` clone
inherits it **by reference** — it is never reset or re-added (see
[decision-task.spec.md § Reuse via Clone](decision-task.spec.md#reuse-via-clone)).

`T` may be any `BlValue` — a scalar (`BlNumber`, `BlString`, `BlDate`,
`BlBoolean`, …) or a composite (`BlTable`, `BlList`, `BlDictionary`). A `BlTable`
constant is the static counterpart to a [`Relation`](relation.spec.md):
`Relation` builds a table from a row struct and per-cell expressions, whereas
`ReferenceData` holds a pre-built table value directly.

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

A composite value — a static lookup table — is held directly, without going
through `Relation`:

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

## Registering and referencing

A `ReferenceData` is registered with the `DecisionTask` that uses it, via
`bl.WithReferenceData`. The task binds its `Value` into the evaluation context
under its **`Id`**, and adds that `Id` and type to the environment node
expressions are compiled against (a **reference scope** distinct from the task's
`InputSchema`). The expression-language variable a node sees is therefore the
reference data's `Id` string — **not** the Go variable it was assigned to. Here
the constant was declared `var taxRate = bl.NewReferenceData(bl.Number(0.2),
bl.WithId("tax_rate"), …)`, so its `Id` is `"tax_rate"` and node bodies reference
it as `tax_rate` (the Go name `taxRate` is irrelevant inside the expression):

```go
type GrossPriceOutputs struct {
    Amount bl.BlNumber
}

var grossPrice = bl.NewLiteralExpression[GrossPriceOutputs](bl.LiteralExpressionOpts{
    Id:   "gross_price",
    Name: "Gross Price",
    // `tax_rate` here is taxRate's Id (bl.WithId("tax_rate")), bound into scope
    // by WithReferenceData below.
    Body: `net_price.amount * (1 + tax_rate)`,
})

// The task's external input contract — what netPrice consumes. (net_price is an
// internal node output; tax_rate is reference data, resolved in the reference
// scope — neither is part of InputSchema.)
var pricingInputs, _ = bl.Schema(
    bl.Field{Name: "list_price", Type: bl.TypeNumber},
)

var pricing = bl.NewDecisionTask(
    bl.WithId("pricing"),
    bl.WithName("Pricing"),
    bl.WithNodes(netPrice, grossPrice),
    bl.WithInputSchema(pricingInputs),
    bl.WithReferenceData(taxRate), // registers the Go value `taxRate`, whose Id is "tax_rate"
)
```

So `tax_rate` resolves because `bl.WithReferenceData(taxRate)` (a) adds the name
`tax_rate` (type `BlNumber`, taxRate's `Id`) to the compile-time environment so
the body type-checks, and (b) binds its value into the evaluation context under
that same `Id` at evaluation time. A `ReferenceData` need not — and cannot — be
listed in `[]bl.DecisionNode`.

---

## Resolution

A `ReferenceData` is never evaluated. The `DecisionTask` it is registered with
binds its `Value` into the evaluation context under its `Id` before nodes run,
and includes its `Id` and type in the compile-time environment so referencing
expressions type-check. Because the source is a constant rather than a node in
the evaluation order, the reference forms no node-to-node dependency edge — a
`ReferenceData` is always a leaf with no upstream dependencies. Being
decision-logic, it is shared by reference with a task's clones and is never reset.

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
- A `ReferenceData` `Id` that collides with a node `Id`, another `ReferenceData`
  `Id`, or an `InputSchema` variable name in the owning task is a
  `DecisionDefinitionError`, raised by `bl.NewDecisionTask`.
- A name referenced by a node but never registered via `bl.WithReferenceData`
  (and not otherwise in scope) is unresolved — `bl.NewDecisionTask` reports it as
  a definition error at construction.
- A `DecisionTask` clone inherits its source's reference data by reference; it is
  not reset, and `bl.WithReferenceData` is not a valid `Clone` option (decision
  logic is shared from the source).
