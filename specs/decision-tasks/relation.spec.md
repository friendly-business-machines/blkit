---
name: Relation
description: A DecisionNode defined as tabular data — named columns and rows of expressions that evaluate to a BlTable
targets:
  - ../decisions/relation.go
---

# Relation

A `Relation` is a `DecisionNode` that represents tabular data. It has named columns and rows of expressions. Each row evaluates to a `BlContext` (keyed by column names), and the relation as a whole evaluates to a [`BlTable`](../expressions/table.spec.md) — a typed list of uniformly-keyed rows.

Relations are useful for defining static lookup tables, reference data, or any structured data set within a decision model.

```go
type Relation struct {
    DecisionNode  // Id, Name, Description, OutputName, plus Require*/Optional* methods

    Columns []RelationColumn
    Rows    []RelationRow
}

// Per-type column factories — register a column on the relation AND return the
// declared type metadata. Column refs are not used as expressions (rows are
// authored as ordered expression slices), but the typed factories enforce
// per-cell type validation.
func (r *Relation) NumberColumn(name string) RelationColumn
func (r *Relation) StringColumn(name string) RelationColumn
func (r *Relation) BooleanColumn(name string) RelationColumn
func (r *Relation) DateColumn(name string) RelationColumn
// + per-type column factories for the remaining Bl* types

func (r *Relation) AddRow(expressions ...BlExpr) *Relation

// Evaluate the relation — returns a BlTable
func (r *Relation) Evaluate(input map[string]any) (BlValue, error)

// Render as a markdown string
func (r *Relation) ToMarkdown() string


type RelationColumn struct {
    Name    string
    TypeRef string // blkit type name (derived from the factory method)
}

type RelationRow struct {
    Expressions []BlExpr // one expression per column, in column order
}
```

`Relation` is instantiated via direct struct literal — no `New*` factory.

---

## Building a Relation — the constructor-function idiom

A relation declares its inputs via `Require*` (if any row expressions reference input variables); columns are declared via per-type factories; rows are added with one expression per column:

```go
func shippingRates() *Relation {
    rates := &Relation{
        Id:   "shipping_rates",
        Name: "Shipping Rates",
    }
    rates.StringColumn("region")
    rates.NumberColumn("standard_rate")
    rates.NumberColumn("express_rate")

    rates.AddRow(Bl.String("domestic"),      Bl.Number(5.99),  Bl.Number(12.99))
    rates.AddRow(Bl.String("europe"),        Bl.Number(15.99), Bl.Number(29.99))
    rates.AddRow(Bl.String("international"), Bl.Number(25.99), Bl.Number(49.99))

    return rates
}

result, err := shippingRates().Evaluate(map[string]any{})
// result is a BlTable with columns [region, standard_rate, express_rate] and rows:
//   {region: "domestic",      standard_rate: 5.99,  express_rate: 12.99}
//   {region: "europe",        standard_rate: 15.99, express_rate: 29.99}
//   {region: "international", standard_rate: 25.99, express_rate: 49.99}
```

---

## Row Expressions

Each entry in a `RelationRow` is a `BlExpr`, so row cells can reference declared inputs via the captured typed refs:

```go
func riskThresholds() *Relation {
    thresholds := &Relation{
        Id:   "thresholds",
        Name: "Risk Thresholds",
    }
    baseLimit := thresholds.RequireNumber("base_limit")

    thresholds.StringColumn("tier")
    thresholds.NumberColumn("min_score")
    thresholds.NumberColumn("max_amount")

    thresholds.AddRow(Bl.String("gold"),   Bl.Number(750), baseLimit.Multiply(Bl.Number(5)))
    thresholds.AddRow(Bl.String("silver"), Bl.Number(600), baseLimit.Multiply(Bl.Number(3)))
    thresholds.AddRow(Bl.String("bronze"), Bl.Number(0),   baseLimit)

    return thresholds
}
```

---

## Markdown Rendering

`ToMarkdown()` returns a markdown table with column names as headers and each row's expressions as cells.

```go
fmt.Println(shippingRates().ToMarkdown())
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

- A `Relation` with no rows evaluates to an empty `BlTable` (with the declared columns and zero rows).
- A `Relation` with no columns is invalid; `DecisionTask.Validate()` rejects it.
- A `RelationRow` with fewer expressions than columns produces `BlNull` for the missing columns.
- A `RelationRow` with more expressions than columns is invalid; `DecisionTask.Validate()` rejects it.
- Duplicate column names are invalid; `DecisionTask.Validate()` rejects them.
- Each cell's value is validated against the column's declared type (from the `*Column` factory). A mismatch produces a `BlTypeError`.
