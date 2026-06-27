---
name: ReferenceData
description: A static value source generic over T BlValue — carries identity (Id, Name, Description) and a single constant Value, exposed as a wireable .Out handle. Registered in DecisionTaskConfig.ReferenceData and connected into the graph with bl.Edge like any node output; it computes nothing and is never evaluated.
targets:
  - ../../core/reference_data.go
---

# ReferenceData

A `ReferenceData` is a static **value source** within a decision model: a single
constant `BlValue` paired with identity (`Id`, `Name`, `Description`). It is
generic over the value's type `T`.

`ReferenceData` is **not** a [`DecisionNode`](decision-node.spec.md) and has **no
`Evaluate` method** — a constant computes nothing, so it carries its value rather
than deriving one. But it exposes a single typed **output handle**, `.Out`, so it
can be wired into a [`DecisionTask`](decision-task.spec.md) graph exactly like a
node output: `bl.Edge(taxRate.Out, grossPrice.In.TaxRate)`. It is registered in the
task's `Config.ReferenceData` list and wired by its `.Out`; the task binds its
`Value` into the evaluation environment before the consuming node runs. A
`ReferenceData` never appears in a task's `bl.Nodes(...)` list and is never part of
its evaluation order.

Reference data is part of a task's **decision logic**, so a `DecisionTask` clone
shares it **by reference** — the edges that wire it carry over unchanged (see
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

    // Out is the constant exposed as a wireable output handle, stamped with this
    // source's Id. Connect it to a node input with bl.Edge.
    Out Handle[T]
}

// ReferenceDataConfig configures a ReferenceData, matching the family's
// config-struct convention. Value is the constant; Id is mandatory. T is given by
// the config's type argument, so NewReferenceData needs no explicit [T].
type ReferenceDataConfig[T BlValue] struct {
    Id          string
    Name        string
    Description string
    Value       T
}

// NewReferenceData builds a ReferenceData from its config. The Value is wrapped
// into the .Out handle, stamped with the Id. Definition failures (empty Id, nil
// Value) panic with a *DecisionDefinitionError, per the family's
// panicking-constructor convention.
func NewReferenceData[T BlValue](config ReferenceDataConfig[T]) *ReferenceData[T]

func (r *ReferenceData[T]) GetId() string
func (r *ReferenceData[T]) GetName() string
func (r *ReferenceData[T]) GetDescription() string
func (r *ReferenceData[T]) GetValue() BlValue

// Render as a markdown string
func (r *ReferenceData[T]) ToMarkdown() string

// ReferenceValue is the non-generic view of a ReferenceData, so a DecisionTask can
// hold a heterogeneous set of value sources in its DecisionTaskConfig.ReferenceData
// list. Every *ReferenceData[T] satisfies it.
type ReferenceValue interface {
    GetId() string
    GetName() string
    GetDescription() string
    GetValue() BlValue
}
```

A `ReferenceData` has no `Evaluate` method and does not satisfy the `DecisionNode`
interface. `Id` is mandatory; `Name` and `Description` are optional. To be used by a
task it is **registered** in `DecisionTaskConfig.ReferenceData` (as a `ReferenceValue`)
and **wired** via its `.Out` handle (see [§ Wiring and consuming](#wiring-and-consuming)).

---

## Building a ReferenceData

```go
var taxRate = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlNumber]{
    Id:    "tax_rate",
    Name:  "Tax Rate",
    Value: bl.Number(0.2),
})

var baseCurrency = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlString]{
    Id:    "base_currency",
    Name:  "Base Currency",
    Value: bl.String("GBP"),
})
```

The type argument on the config (`[bl.BlNumber]` / `[bl.BlString]`) supplies `T`, so
`NewReferenceData` itself needs no explicit type argument.

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

var shippingRates = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlTable]{
    Id:    "shipping_rates",
    Name:  "Shipping Rates",
    Value: shippingRatesTable,
})
```

`bl.Table` is independently fallible (`(BlTable, error)`), so a malformed table
surfaces at its own declaration before reaching `bl.NewReferenceData`.

---

## Wiring and consuming

A `ReferenceData` is used by a `DecisionTask` in two steps: it is **registered** in
`Config.ReferenceData` (so the task knows the constant and binds its value), and its
`.Out` handle is **wired** to a node input in the `Graph`, exactly like any other
source. An edge from a `.Out` that is not registered is a `DecisionDefinitionError`.

```go
type GrossInputs struct {
    ListPrice bl.Handle[bl.BlNumber] `expr:"list_price"` // fed from the task input
    TaxRate   bl.Handle[bl.BlNumber] `expr:"tax_rate"`   // the reference-data constant
}
type GrossOutputs struct {
    GrossPrice bl.Handle[bl.BlNumber] `expr:"gross_price"`
}

var grossPrice = bl.NewDecisionExpression[GrossInputs, GrossOutputs](bl.DecisionExpressionConfig{
    Id:   "gross_price",
    Name: "Gross Price",
    Entries: bl.Entries{
        "gross_price": `list_price * (1 + tax_rate)`,
    },
})

// The reference-data constant, exposed for wiring as taxRate.Out.
var taxRate = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlNumber]{
    Id:    "tax_rate",
    Name:  "Tax Rate",
    Value: bl.Number(0.2),
})

// The task's external contracts. tax_rate is reference data, so it is NOT part of
// TaskIn — it is wired in from taxRate.Out below.
type PricingInputs struct {
    ListPrice bl.Handle[bl.BlNumber] `expr:"list_price"`
}
type PricingOutputs struct {
    GrossPrice bl.Handle[bl.BlNumber] `expr:"gross_price"`
}

var in  = bl.Inputs[PricingInputs]()
var out = bl.Outputs[PricingOutputs]()

var pricing = bl.NewDecisionTask[PricingInputs, PricingOutputs](bl.DecisionTaskConfig{
    Id:            "pricing",
    Name:          "Pricing",
    Nodes:         bl.Nodes(grossPrice),
    ReferenceData: []bl.ReferenceValue{taxRate}, // register the constant…
    Graph: bl.Edges{
        bl.Edge(in.ListPrice,              grossPrice.In.ListPrice), // task input → node input
        bl.Edge(taxRate.Out,               grossPrice.In.TaxRate),   // reference-data constant
        bl.Edge(grossPrice.Out.GrossPrice, out.GrossPrice),          // node output → task output
    },
})
```

Because the connection is a typed `bl.Edge`, wiring `taxRate.Out` (a
`Handle[BlNumber]`) to a `Handle[BlString]` input would be a **Go compile error**.
A `ReferenceData` need not — and cannot — be listed in `bl.Nodes(...)`.

---

## Resolution

A `ReferenceData` is never evaluated. The `DecisionTask` it is registered with (in
`Config.ReferenceData`) binds its `Value` into the evaluation environment, routed
along the edges from `.Out`, before the consuming nodes run. Because the source is a constant rather than a node in
the evaluation order, it draws no dependency edge of its own — a `ReferenceData`
is always a leaf with no upstream dependencies. Being decision-logic, it is shared
by reference with a task's clones.

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
  `DecisionTask`'s `bl.Nodes(...)` list — it is registered in `Config.ReferenceData`.
- An edge wiring a `ReferenceData`'s `.Out` whose source is not registered in the
  task's `Config.ReferenceData` is a `DecisionDefinitionError`.
- A `ReferenceData` is always a leaf — it references no other node and has no
  upstream dependencies.
- Wiring a `ReferenceData`'s `.Out` handle to an input of a different `BlValue`
  type is a **Go compile error**, caught before construction.
- A `DecisionTask` clone shares its source's reference data and wiring by
  reference; decision logic is never re-supplied to `Clone`.
