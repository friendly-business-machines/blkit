---
name: BlTable
description: blkit's table type — an ordered, immutable list of uniformly-keyed contexts; extends BlExpr so all operations are deferred and chainable
targets:
  - ../../expr/table.go
---

# BlTable

`BlTable` is blkit's tabular value type: an ordered list of `BlContext` rows, all sharing the **same set of column keys** (the "uniform-keys" invariant). It is the typed counterpart to a list of records / a relation in DMN. It extends `BlExpr`, so every instance is a literal leaf node and all operations return deferred `BlExpr` nodes.

A `BlTable` is structurally a `BlList[BlContext]` with the additional invariant that every row has the same keys. This makes it a natural fit for the [`TableContract`](../data/data-contract.spec.md#tablecontract) data shape, the output of a [`Relation`](../decision-tasks/relation.spec.md), and any context value that holds tabular data.

```go
type BlTable struct { BlExpr }

// Construction is via Bl.Table(...) — see bl.spec.md.
// Two construction styles are supported:
//   1. From row contexts (keys derived from the first row):
//        Bl.Table(Bl.Context(...), Bl.Context(...))
//   2. With an explicit column ordering (rows then validated against it):
//        Bl.Table(Bl.Columns("region", "rate"), Bl.Row(...), Bl.Row(...))

// Properties — deferred
// ColumnNames BlList            // BlList of BlString in declared/inferred column order
// RowCount    BlNumber

func (t *BlTable) IsEmpty() BlExpr { ... }   // evaluates to BlBoolean (zero rows)

// Row access — deferred; 1-indexed; negative indices count from the end
func (t *BlTable) Row(index BlExpr) BlContext { ... }   // evaluates to BlNull if out of range
func (t *BlTable) Rows() BlList { ... }                  // BlList of BlContext (insertion order)
func (t *BlTable) FirstRow() BlContext { ... }
func (t *BlTable) LastRow() BlContext { ... }

// Column access — deferred; evaluates to BlList of values in row order
func (t *BlTable) Column(name string) BlList { ... }
func (t *BlTable) HasColumn(name string) BlExpr { ... }  // evaluates to BlBoolean

// Immutable structural operations — deferred; evaluate to BlTable
func (t *BlTable) AddRow(row BlExpr) BlTable { ... }                // row's keys must match the table's columns
func (t *BlTable) AddRows(rows ...BlExpr) BlTable { ... }
func (t *BlTable) RemoveRow(index BlExpr) BlTable { ... }           // 1-indexed
func (t *BlTable) Project(columns ...string) BlTable { ... }        // keep only these columns
func (t *BlTable) Drop(columns ...string) BlTable { ... }           // drop these columns
func (t *BlTable) Rename(from string, to string) BlTable { ... }    // rename a column
func (t *BlTable) Distinct() BlTable { ... }                         // remove duplicate rows (full-row equality)

// Filter and sort — deferred; evaluate to BlTable
func (t *BlTable) Filter(predicate func(row BlContext) BlExpr) BlTable { ... }
func (t *BlTable) SortBy(column string, opts ...SortOption) BlTable { ... }
// SortOption is Ascending() (default) or Descending(); the column's values
// must be of a comparable type. Stable sort.

// Conversion — deferred
func (t *BlTable) AsList() BlList { ... }                           // the underlying BlList of BlContext

// Equality — deferred; evaluates to BlBoolean
func (t *BlTable) Equals(other BlExpr) BlExpr { ... }   // row-wise, order-sensitive; row equality is order-insensitive on keys

// Eager host-language utilities — only valid on a concrete BlTable after .Evaluate()
func (t *BlTable) ToRecords() []map[string]BlValue { ... }
func (t *BlTable) String() string { ... }      // Literal notation: a BlList literal of BlContext rows
func (t *BlTable) ToMarkdown() string { ... }   // markdown table with aligned columns
```

## Relation to BlList

A `BlTable` is structurally a `BlList[BlContext]` with the uniform-keys invariant. All `BlList` operations remain available indirectly via `AsList()` — call `t.AsList()` to drop into list-of-context territory when needed. Operations that may break the uniform-keys invariant (e.g. `BlList.Append` with a row that has different keys) are not surfaced on `BlTable` directly; use `AddRow` (which validates) or convert to a list.

Conversely, a `BlList` of `BlContext` may be converted to a `BlTable` via `BlList.AsTable()` (see [list.spec.md](list.spec.md)), which validates uniform keys and raises `BlTypeError` at evaluation time on mismatch.

## Column Order

Column order is preserved insertion-style — the order in which columns appear in `Bl.Columns(...)` (when used) or the order of keys in the first row (when inferred). `Project(...)`, `Rename(...)`, `Drop(...)` preserve relative ordering of the remaining columns. `ColumnNames` and the `ToMarkdown()` rendering follow this order.

## Markdown Rendering

`ToMarkdown()` returns a single markdown table with aligned columns. Headers are the column names; cells render scalar `Bl` values as inline notation (`"Alice"`, `42`, `true`) and nested contexts/lists as compact one-line `Bl` literals. Column widths are computed from the longest cell in each column so the rendered table stays readable in plain text.

```go
fmt.Println(table.ToMarkdown())
```

Output:

```
| region          | standard_rate | express_rate |
|-----------------|---------------|--------------|
| "domestic"      | 5.99          | 12.99        |
| "europe"        | 15.99         | 29.99        |
| "international" | 25.99         | 49.99        |
```

For an empty table, `ToMarkdown()` renders the header row only. For a table with no columns (vacuous — see edge cases), it returns the empty string.

## Construction Examples

### From row contexts

```go
shippingRates := Bl.Table(
    Bl.Context(map[string]BlExpr{
        "region":        Bl.String("domestic"),
        "standard_rate": Bl.Number(5.99),
        "express_rate":  Bl.Number(12.99),
    }),
    Bl.Context(map[string]BlExpr{
        "region":        Bl.String("europe"),
        "standard_rate": Bl.Number(15.99),
        "express_rate":  Bl.Number(29.99),
    }),
)
```

### With explicit column ordering

```go
shippingRates := Bl.Table(
    Bl.Columns("region", "standard_rate", "express_rate"),
    Bl.Row(Bl.String("domestic"),      Bl.Number(5.99),  Bl.Number(12.99)),
    Bl.Row(Bl.String("europe"),        Bl.Number(15.99), Bl.Number(29.99)),
    Bl.Row(Bl.String("international"), Bl.Number(25.99), Bl.Number(49.99)),
)
```

`Bl.Row(...)` is positional; values are matched to columns by index. `Bl.Columns(...)` declares the column order — rows are then validated against it.

## Filter and Project

```go
domestic := shippingRates.Filter(func(row BlContext) BlExpr {
    return row.Get("region").Equals(Bl.String("domestic"))
})

ratesOnly := shippingRates.Project("region", "standard_rate")
```

Both return a new `BlTable` deferred expression node.

## Edge Cases

- `Bl.Table()` with no rows and no `Bl.Columns(...)` produces an empty table with no columns. Adding a row to it sets the column ordering from that row's keys.
- `Bl.Table(Bl.Columns("a", "b"))` with no rows is valid — an empty table with declared columns.
- `AddRow(row)` where `row` has keys differing from the table's columns produces a `BlTypeError` at evaluation time.
- `Project(...)` with a column not present in the table produces a `BlTypeError`.
- `Rename(from, to)` where `from` is not present, or where `to` collides with another existing column, produces a `BlTypeError`.
- `SortBy(col)` on a column whose values are mixed types (no total order) produces a `BlTypeError`.
- `Distinct()` uses `BlContext.Equals` (order-insensitive on keys) for row equality.
- `Equals(other)` returns false if the column sets differ, even if row data is identical (the table shape is part of identity).
- `Row(index)` and `Column(name)` evaluate to `BlNull` for out-of-range indices or unknown column names — non-fatal, matching `BlList.Get` and `BlContext.Get` semantics.
- A `BlList` whose elements are not all `BlContext`, or whose `BlContext` elements have inconsistent keys, produces a `BlTypeError` when converted via `AsTable()`.
