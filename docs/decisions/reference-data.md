# Reference Data

> Feed decisions the lookup tables and constants they read from — rates, tiers,
> thresholds — as static values wired straight into the graph.

A **reference data** value is a static constant a decision reads: a tax rate, a
minimum income, a shipping-rates table. It computes nothing and never runs — it
just carries its value and exposes it for wiring.

A `bl.ReferenceData[T]` is generic over the value's type and is built from a config
that carries the constant:

```go
var minIncome = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlNumber]{
    Id:    "min_income",
    Name:  "Minimum Income",
    Value: bl.Number(30000),
})

var baseCurrency = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlString]{
    Id:    "base_currency",
    Value: bl.String("GBP"),
})
```

The type argument on the config supplies `T`, so the call needs no explicit
`[bl.BlNumber]`. `Id` is required; `Name` and `Description` are optional.

## Any value, including tables

`T` can be any blkit value — a number or string, or a composite like a
[table](../expressions/tables.md). A table held as reference data is a static
lookup any decision can read:

```go
shippingRates, _ := bl.Table(
    bl.Cols{{Name: "region", Type: bl.TypeString}, {Name: "rate", Type: bl.TypeNumber}},
    bl.Row{"domestic", 5.99},
    bl.Row{"europe", 15.99},
)

var rates = bl.NewReferenceData(bl.ReferenceDataConfig[bl.BlTable]{
    Id:    "shipping_rates",
    Value: shippingRates,
})
```

## Wiring it in

A reference data exposes a single `.Value` handle, so it is wired into a
[decision task](decision-tasks.md) exactly like a node output — connect it to a
node input with `bl.Edge`:

```go
bl.Edge(minIncome.Value, eligibility.In.MinIncome)
```

The task discovers the constant from the edge and supplies it before the consuming
node runs. There's nothing else to register. Because the connection is a typed
`bl.Edge`, wiring a number constant into a string input simply won't compile.
