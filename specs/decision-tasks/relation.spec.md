---
name: Relation
description: A DecisionNode generic over a row struct — defines tabular data with typed columns and rows of typed cell expressions, evaluating to a bl.BlTable
targets:
  - ../decisions/relation.go
---

# Relation

A `Relation` is a `DecisionNode` that represents tabular data. It is generic over a row struct whose exported fields describe the table's columns. The relation evaluates to a [`bl.BlTable`](../expressions/table.spec.md) — a typed list of uniformly-keyed rows.

Relations are useful for defining static lookup tables, reference data, or any structured data set within a decision model.

Unlike the multi-output `DecisionTable`, a `Relation`'s single output is the whole table. Downstream nodes consume it as a typed `bl.BlTable[Row]` via the node's `Table` field.

```go
type Relation[Row any] struct {
    Id          string
    Name        string
    Description string

    Rows []Row // typed row values, supplied via opts and validated against Row's columns

    Table bl.BlTable[Row] // typed handle to the whole table, populated by NewRelation
}

func NewRelation[Row any](opts RelationOpts[Row]) *Relation[Row]

type RelationOpts[Row any] struct {
    Id          string
    Name        string
    Description string

    // Rows is a builder closure returning the typed row values. Each row's
    // field expressions may reference upstream node outputs (typed Bl* handles)
    // or DecisionTask-level input handles.
    Rows func() []Row
}

// Evaluate the relation — returns a bl.BlTable
func (r *Relation[Row]) Evaluate(input map[string]any) (BlValue, error)

// Render as a markdown string
func (r *Relation[Row]) ToMarkdown() string
```

`NewRelation[Row]` reflects on the `Row` type parameter to derive column metadata:

- **Every exported field is a column.** No filter — the caller put it there, so it is a column.
- The column name defaults to the lowercased field name; a `bl:"name"` struct tag overrides.
- The column's type is the field's static `Bl*` type. A non-`bl.BlValue` field type produces `DecisionDefinitionError`.

The constructor then invokes `opts.Rows` and validates each returned row's cell expressions against the column types.

---

## Building a Relation

```go
type ShippingRatesRow struct {
    Region       BlString
    StandardRate BlNumber
    ExpressRate  BlNumber
}

var shippingRates = NewRelation[ShippingRatesRow](RelationOpts[ShippingRatesRow]{
    Id:   "shipping_rates",
    Name: "Shipping Rates",
    Rows: func() []ShippingRatesRow {
        return []ShippingRatesRow{
            {Region: bl.String("domestic"),      StandardRate: bl.Number(5.99),  ExpressRate: bl.Number(12.99)},
            {Region: bl.String("europe"),        StandardRate: bl.Number(15.99), ExpressRate: bl.Number(29.99)},
            {Region: bl.String("international"), StandardRate: bl.Number(25.99), ExpressRate: bl.Number(49.99)},
        }
    },
})

result, err := shippingRates.Evaluate(map[string]any{})
// result is a bl.BlTable[ShippingRatesRow] with three rows.
//
// Downstream typed access:
// shippingRates.Table — bl.BlTable[ShippingRatesRow]
```

---

## Row Expressions

Each row cell is a typed `Bl*` expression, so cells can reference upstream node outputs or DecisionTask-level inputs:

```go
type RiskThresholdsRow struct {
    Tier      BlString
    MinScore  BlNumber
    MaxAmount BlNumber
}

var riskThresholds = NewRelation[RiskThresholdsRow](RelationOpts[RiskThresholdsRow]{
    Id:   "thresholds",
    Name: "Risk Thresholds",
    Rows: func() []RiskThresholdsRow {
        return []RiskThresholdsRow{
            {Tier: bl.String("gold"),   MinScore: bl.Number(750), MaxAmount: baseLimit.Multiply(bl.Number(5))},
            {Tier: bl.String("silver"), MinScore: bl.Number(600), MaxAmount: baseLimit.Multiply(bl.Number(3))},
            {Tier: bl.String("bronze"), MinScore: bl.Number(0),   MaxAmount: baseLimit},
        }
    },
})
```

`baseLimit` is a typed `bl.BlNumber` handle from an upstream node or a DecisionTask-level input.

---

## Markdown Rendering

`ToMarkdown()` returns a markdown table with column names as headers and each row's expressions as cells.

```go
fmt.Println(shippingRates.ToMarkdown())
```

Output:

```text
### Shipping Rates

| region          | standard_rate | express_rate |
|-----------------|---------------|--------------|
| "domestic"      | 5.99          | 12.99        |
| "europe"        | 15.99         | 29.99        |
| "international" | 25.99         | 49.99        |
```

---

## Edge Cases

- A `Relation[Row]` whose `Row` type has no exported fields is invalid; `NewRelation` raises `DecisionDefinitionError`.
- A `Relation` whose `Rows` closure returns an empty slice evaluates to a `bl.BlTable` with the declared columns and zero rows.
- A `Row` whose field type does not implement `bl.BlValue` is a `DecisionDefinitionError`.
- A cell expression whose runtime type disagrees with its column's declared type produces a `bl.TypeError` at evaluation time.
- Two fields whose `bl:"name"` tags collide is a `DecisionDefinitionError`.
- The `Row` struct's field declaration order is the column order in the output table and in `ToMarkdown`.
