---
name: BlTable
description: The table (relation) type in the blkit expression language — an ordered list of uniformly-keyed dictionaries. Covers row/column access, table built-ins, the transformation methods (filter/filterOut/select/rename/sort/slice/distinct/withColumn/join), grouping (groupBy/agg), the inherited list semantics, and the Go layer (bl.BlTable + expr registrations).
targets:
  - ../../core/table.go
---

# bl.BlTable — the `table` type

`table` is a DMN **relation**: an ordered, immutable list of `dictionary` rows that all
share the same column keys (the "uniform-keys" invariant). The Go value type backing it
is `bl.BlTable`.

Structurally a `table` is a `list` of uniformly-keyed `dictionary`s, so list literals,
indexing, filtering, projection, and the list built-ins ([list.spec.md](list.spec.md),
[bl-expr.spec.md](bl-expr.spec.md)) all apply. Relational operations layer on top in two
shapes: a few **function** built-ins ([§ Built-in functions](#built-in-functions)) —
`union` for row-stacking and `join` — and the **transformation methods**
([§ Transformation methods](#transformation-methods)) — `filter` / `filterOut`,
`select`, `rename`, `sort`, `slice`, `distinct`, `withColumn`, and `join` — for
filtering, projection, schema reshaping, sorting, derived columns, and joins. `union`
and `join` carry both surfaces. **Grouping** ([§ Grouping](#grouping--groupby-and-agg)) —
`groupBy` / `agg` — partitions rows and reduces each group to an aggregate row. See also
[dictionary.spec.md](dictionary.spec.md) for the row shape.

---

## Literals

A literal is the syntactic form for writing a constant value of a type directly inside
an expression (as `[1, 2, 3]` is for a list). **`table` has no dedicated literal form**
— a table is built with one of two constructors. Both validate their input and yield the
`bl.BlTable` type (so the table-specific methods like `t.sort(…)` and attributes like
`t.colNames` accept the result), and both enforce the
[uniform-keys](#uniform-keys-invariant) and [per-column-type](#per-column-value-type)
invariants.

**`table(names, rows…)` — the columnar constructor.** Name the columns once, then list the
rows. The first argument is the list of column names; every further argument is a value row
whose cells bind positionally to the header. Each column's value type is **inferred from its
cells** (the cells are already natively typed) and must be uniform down the column. This is
the CSV-style form: it rides the ordinary list-literal grammar (the comma is the cell
separator), keeps cells natively typed, and never repeats a key.

```
// expression-language
table(
  ["region",   "rate",  "ships_today", "ship_date",        "updated_at"],   // column names
  ["domestic", 5.99,    true,          date("2025-03-28"), datetime("2025-03-28T11:45:30")],
  ["europe",   15.99,   false,         date("2025-04-02"), datetime("2025-04-01T09:15:00")],
)
```

A cell may be any expression (`5.99 * 2`, a variable); a `null` cell means "null in this
row". `table(names)` with no value rows is a zero-row table that still carries its column
names (their value types are fixed when rows are first added); `table([])` is the truly
empty table. This is the expression-language parallel of the host-side
`bl.Table(bl.Cols{…}, bl.Row{…}…)` (see [§ Construction (host-side)](#construction-host-side)).

**`tableFromDicts(listOfDictionaries)` — adopt dictionary data.** When you already have a
list of uniformly-keyed dictionaries, wrap it directly; the column set and per-column types
are inferred from the dictionaries.

```
// expression-language
// tableFromDicts validates uniform keys and yields a bl.BlTable:
var myTable = tableFromDicts([
  {region: "domestic", rate: 5.99,  ship_date: date("2025-03-28"), updated_at: datetime("2025-03-28T11:45:30")},
  {region: "europe",   rate: 15.99, ship_date: date("2025-04-02"), updated_at: datetime("2025-04-01T09:15:00")},
])

// Row access (1-based; negative from end; out-of-range → empty sub-table):
myTable[1]                             // → a 1-row sub-table
// Column access is list projection over the rows:
myTable.region                         // → ["domestic", "europe"]
// Filter is row filtering (columns are in scope by name):
myTable[rate > 10]                     // → sub-table of rows with rate > 10
```

`[@test] ../../core/table_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlTable` with `bl.Table(columns bl.Cols, rows ...bl.Row)
(bl.BlTable, error)`: a **compact typed header** (`bl.Cols`) declaring each column's name
and data type, followed by **positional value rows** (`bl.Row`). The header's order is the
binding order — value *i* in every row is the cell for column *i* — and cells wrap to the
matching `bl.BlValue` automatically (see [bl-expr.spec.md § Bridging native ↔
Bl\*](bl-expr.spec.md#bridging-native--bl-valuego)), so no per-cell `bl.String` / `bl.Number`
wrapping is needed. The stored column order is the canonical sorted order per
[dictionary.spec.md](dictionary.spec.md); the header only fixes the column set, types, and
the rows' binding order. By convention a comment listing the column names, aligned over
their values, sits above the first `bl.Row`.

```go
// host-side (Go)
var shippingRates, _ = bl.Table(
    bl.Cols{{"region", bl.TypeString}, {"rate", bl.TypeNumber}, {"ships_today", bl.TypeBoolean}},
    //      region      rate    ships_today
    bl.Row{"domestic",  5.99,   true},
    bl.Row{"europe",    15.99,  false},
)

// A null cell is just nil in that position:
var withGap, _ = bl.Table(
    bl.Cols{{"region", bl.TypeString}, {"rate", bl.TypeNumber}, {"ships_today", bl.TypeBoolean}},
    //      region   rate  ships_today
    bl.Row{"intl",   nil,  false},   // rate is null
)

// Zero-row table that still carries its columns.
var noRates, _ = bl.Table(bl.Cols{{"region", bl.TypeString}, {"rate", bl.TypeNumber}})

// Truly empty table — no columns, no rows.
var empty, _ = bl.Table(bl.Cols{})
```

The Go-literal cells above are a convenience — they auto-wrap. A `bl.Row` cell may also be
an **already-constructed `bl.BlValue`**, which is what you want when you're assembling rows
from values you already hold; the two forms mix freely in the same row, and an explicit
`bl.Null()` is the wrapped form of a `nil` cell:

```go
// host-side (Go)
var region        = bl.String("domestic")   // bl.String returns a bl.BlString
var rate, _       = bl.Number(5.99)          // bl.Number / bl.Boolean return (value, error)
var shipsToday, _ = bl.Boolean(true)

var fromValues, _ = bl.Table(
    bl.Cols{{"region", bl.TypeString}, {"rate", bl.TypeNumber}, {"ships_today", bl.TypeBoolean}},
    //     region     rate    ships_today
    bl.Row{region,    rate,   shipsToday},   // explicit bl.BlValue cells
    bl.Row{"europe",  15.99,  bl.Null()},    // literals auto-wrap; bl.Null() is null
)
```

`bl.Table(...)` returns `(bl.BlTable, error)`. The error path fires when:

- A `bl.Row`'s length differs from the column count → `bl.TypeError`.
- A non-`nil` cell wraps to a type other than its column's declared `bl.Type` → `bl.TypeError`
  (the per-column value-type invariant; see
  [§ Per-column value type](#per-column-value-type)). A `nil` cell is always allowed — it
  becomes `bl.Null()`, the way to express "all values null" for a row.
- The header is malformed — a duplicate or empty column name, or a `bl.Type` of
  `bl.TypeNull` or an unknown value → `bl.TypeError` (the same well-formedness rules
  [bl.Schema(...)](schema.spec.md#construction-host-side) applies).

`bl.Table(bl.Cols{})` is the empty table with no columns; `bl.Table(cols)` with no rows is
a zero-row table that still carries its columns.

**Passing a host-built table into an expression.** A `bl.BlTable` is an ordinary
`bl.BlValue`, so it binds to a variable in the evaluation input like any other value (see
[bl-expr.spec.md § Using the engine](bl-expr.spec.md#using-the-engine)). Declare the
variable as a `bl.TypeTable` field whose nested `Fields` describe the columns (see
[schema.spec.md](schema.spec.md#construction-host-side)), then reference it — and call the
[transformation methods](#transformation-methods) on it — from the source text:

```go
// host-side (Go)
// 1. Build the table in Go.
var shippingRates, _ = bl.Table(
    bl.Cols{{"region", bl.TypeString}, {"rate", bl.TypeNumber}, {"ships_today", bl.TypeBoolean}},
    //      region      rate    ships_today
    bl.Row{"domestic",  5.99,   true},
    bl.Row{"europe",    15.99,  false},
    bl.Row{"intl",      24.50,  false},
)

// 2. Declare it as a typed variable the expression may reference.
var rateColumns, _ = bl.Schema(
    bl.Field{Name: "region",      Type: bl.TypeString},
    bl.Field{Name: "rate",        Type: bl.TypeNumber},
    bl.Field{Name: "ships_today", Type: bl.TypeBoolean},
)
var schema, _ = bl.Schema(
    bl.Field{Name: "shipments", Type: bl.TypeTable, Fields: rateColumns},
)

// 3. Compile an expression that chains table methods on the input table.
var pricey, _ = bl.Expr(
    `shipments.filter(rate > 6).sort(desc("rate")).select("region", "rate")`,
    schema,
)

// 4. Bind the host-built table to `shipments` and evaluate.
var inputs, _ = bl.Dictionary(map[string]bl.BlValue{"shipments": shippingRates})
var result, _ = pricey.Evaluate(inputs)
// result is a bl.BlTable:
//   region   rate
//   intl     24.50
//   europe   15.99
```

Tables are immutable: every operation that "modifies" a table — the function built-ins
(`union`, `join`) and the transformation methods (`select`, `filter` / `filterOut`,
`rename`, `sort`, `slice`, `distinct`, `withColumn`, `join`) — returns a fresh
`bl.BlTable`.

---

## Operators

| Operator | Meaning | Example |
|---|---|---|
| `t[i]` | row by 1-based index — returns a 1-row sub-table (negative from end; out-of-range → empty sub-table) | `t[1]`, `t[-1]` |
| `t.col` | column projection — returns a `list` of cell values | `t.region` |
| `t[predicate]` | row filter — columns are in scope by name (`item` is the whole row) | `t[rate > 10]` |
| `=` `!=` | equality (row-wise, order-sensitive; the column set is part of identity) | `t1 = t2` |

**Row-expression scope.** Inside a row expression — the bracket predicate `t[…]`, the
`t.filter` / `t.filterOut` predicates, and the `t.withColumn` expression — a **bare name
resolves to a column** of `t` and to nothing else: it is bound to that row's cell, so
`t[rate > 10]` is the same as `t[item.rate > 10]`. Bare names do **not** fall through to
the enclosing scope; a bare name that is not a column of `t` is an error, never an outer
binding. The whole row is also bound to `item` (a `bl.BlDictionary`), which remains the
way to do whole-row access (`item = otherRow`) and to reach columns whose names aren't
valid identifiers (`item["unit price"]`).

To reference a binding from the **enclosing (host) scope** rather than a column, use the
`env.` pronoun: `env.rate` is the outer variable `rate`, independent of whether `t` has a
`rate` column. `env.` reaches outer values whose names aren't valid identifiers via the
same indexing form (`env["unit price"]`). This is the deliberate mirror of `item.` —
`item.col` always means a column, `env.name` always means an outer binding — so a name
collision between a column and an outer variable is resolved explicitly, never silently.

Tables have no arithmetic operators and no ordering operators (`<` / `<=` / `>` / `>=`)
— they're not a comparable type. Membership (`in`) is **not** defined for tables; use
`listContains(t.toList(), x)` if you need element-style membership, or `t[item =
row].nRows > 0` for whole-row membership.

`[@test] ../../core/table_test.go`

---

## Row indexing

The row selector accesses rows by 1-based position. It accepts three shapes:
a single index `t[i]`, a range `t[i:j]`, or an explicit list of indices
`t[[i1, i2, …]]`. The result is always a `bl.BlTable` — single-row access
doesn't drop a dimension, so every shape returns the same kind of value, just
with a different number of rows. Indexes count from 1; negative indexes count
from the end (`-1` is the last row, `-2` second-to-last).

A range `i:j` selects rows `i` through `j` inclusive. It is lowered to the
list `seq(i, j) = [i, i+1, …, j]` (see [list.spec.md § Sequence constructor](list.spec.md#sequence-constructor-seq-and-the--operator)),
so `t[3:10]` is exactly `t[[3, 4, 5, 6, 7, 8, 9, 10]]`. A list selector picks
the rows at the listed indices, in the order given; duplicate indices are
silently deduped (first occurrence wins, order preserved), so `t[[1, 3, 7, 3]]`
selects rows 1, 3, 7 — the repeated `3` is dropped.

A list selector is `flatten`ed (see [list.spec.md § Built-in functions](list.spec.md#built-in-functions))
before its elements are read as indices, so embedded ranges are spliced in
rather than nested: because `7:15` evaluates to the list `[7, 8, …, 15]`,
`t[[3, 7:15, 20, 25:30]]` is exactly `t[[3, 7, 8, …, 15, 20, 25, 26, …, 30]]` —
rows 3, 7 through 15, 20, and 25 through 30. After flattening every element
must be a `bl.BlNumber`; a non-numeric element is a `bl.TypeError`.

Out-of-range behaviour differs by shape. A single out-of-range index returns
an **empty `bl.BlTable`**, consistent with the filter form `t[predicate]`
returning an empty table when no rows match. Out-of-range indices *inside a
list or range* are silently skipped — the result simply omits them.

| Expression | Result (for a 3-row `t`) |
|---|---|
| `t[1]` | a 1-row sub-table containing the first row |
| `t[3]` | a 1-row sub-table containing the last row |
| `t[-1]` | a 1-row sub-table containing the last row (negative shorthand) |
| `t[-3]` | a 1-row sub-table containing the first row |
| `t[0]` | empty sub-table (indexing is 1-based; no zero index) |
| `t[4]` | empty sub-table (out of range) |
| `t[-4]` | empty sub-table (out of range) |
| `t[1:2]` | a 2-row sub-table containing rows 1 and 2 |
| `t[2:3]` | a 2-row sub-table containing the last two rows |
| `t[1:10]` | a 3-row sub-table (rows past the end are skipped) |
| `t[[1, 3]]` | a 2-row sub-table containing the first and last rows |
| `t[[3, 1]]` | a 2-row sub-table in the listed order: row 3 then row 1 |
| `t[[1, 1]]` | a 1-row sub-table; the duplicate index is deduped (first occurrence wins) |
| `t[[1, 100]]` | a 1-row sub-table; the out-of-range `100` is skipped |
| `t[[1, 2:3]]` | a 3-row sub-table (rows 1, 2, 3) — the embedded range is flattened in |

To extract a single row as a dictionary, use `t[i].toDict()`. To reach a
single cell value, use `t[i, "col"].toValue()` (see
[§ Unwrapping a table](#unwrapping-a-table)).

The filter form `t[predicate]` (see [§ Operators](#operators)) and
`sublist(t.toList(), start, length)` (see [list.spec.md § Built-in functions](list.spec.md#built-in-functions))
are further ways to take a selection of rows.

`[@test] ../../core/table_test.go`

---

## Column indexing

`t.col` projects the column named `col` across every row, returning a
`bl.BlList` of cell values in row order. This dot form is the column-to-list
projection; **square-bracket indexing is not** — `t[, "col"]` returns a
single-column `bl.BlTable`, not a list (every bracket form returns a table;
see [§ Row and column indexing](#row-and-column-indexing)). Because `t[, "col"]`
is a single-column table, `t.toList()` returns its cells as a list — so
`t.col` is exactly `t[, "col"].toList()` (see
[§ Unwrapping a table](#unwrapping-a-table)).

`t.col` requires `col` to be a valid identifier. For column names that aren't
(spaces, special characters), select the column by string with the comma-empty
bracket form and unwrap it — `t[, "unit price"].toList()` — exactly as the
identifier form `t.col` lowers to `t[, "col"].toList()` above.

Single-argument string brackets `t["col name"]` are **not** a column-access
form; the bracket grammar reserves the single-argument shape for row indices
(numeric) and row filters (predicate), so a single string argument is a
`bl.TypeError`.

| Expression | Result (for a 3-row `t`) |
|---|---|
| `t.region` | `["domestic", "europe", "intl"]` (a `bl.BlList` of 3 cells) |
| `t[, "region"]` | a single-column `bl.BlTable`, **not** a list |
| `t[, "unit price"].toList()` | `[5.99, 15.99, 25.99]` — non-identifier column selected by string, then unwrapped to a list |
| `t.missing` | `[null, null, null]` — projection of an undeclared key yields a `null` cell per row, never a missing value, because the table's uniform-keys invariant means every row is queried |
| `t.col` on an empty table | `[]` — projection of any column on a zero-row table is the empty list |
| `t["region"]` | `bl.TypeError` — single-arg string bracket is not a column accessor |

Column projection follows the same rule as list-of-dictionaries projection
([list.spec.md § List projection](list.spec.md#list-projection-fieldname)) — a
`bl.BlTable` is a `bl.BlList` of uniformly-keyed dictionaries, and `.col` on
that list extracts the `col` field from each element. The table's uniform-keys
invariant guarantees the result length equals `t.nRows`.

`[@test] ../../core/table_test.go`

---

## Row and column indexing

The bracket form `t[…]` is a two-axis selector — a **row selector** plus an
optional **column selector** — that covers row indexing, column selection,
slicing, and filtering. **Every bracket form evaluates to a `bl.BlTable`**,
no matter how many rows or columns the selectors pick: brackets never collapse
to a bare cell or a flat list. The row selector sets how many rows the
sub-table has; the column selector sets which columns. To pull a flat
`bl.BlList`, a single row dictionary, or a single cell *value* back out, use
the unwrap methods `t.toList()` / `t.toDict()` / `t.toValue()` — see
[§ Unwrapping a table](#unwrapping-a-table).

**How the comma form is realised.** `expr`'s indexing is a single expression, so the
two-slot forms `t[r, c]` and `t[, c]` are not directly parseable. The `normalise` step
([bl-expr.spec.md § Source normalisation](bl-expr.spec.md#engine-entry-points-enginego))
rewrites them to a backing call **before** parsing — `t[r, c]` → `tableIndex(t, r, c)`, the
empty row slot of `t[, c]` becoming an all-rows marker — distinguishing the comma form from a
list-literal row selector `t[[a, b]]` and skipping strings / nested brackets. The arity errors
below (`t[]`, `t[a, b, c]`) are raised by that rewrite; the no-comma `t[i]` stays as ordinary
single-index access. `tableIndexFn` is documented in [§ Go implementation](#go-implementation-expr-extension).

### Row selector (first slot)

| Form | Selects | Notes |
|---|---|---|
| `i` (a `bl.BlNumber`) | one row by 1-based index (negative from end) | out-of-range → empty sub-table (still an empty sub-table when paired with a column selector) |
| `i:j` | rows `i` through `j` inclusive (1-based) | lowered to `seq(i, j) = [i, i+1, …, j]` by the `:` operator ([list.spec.md § Sequence constructor](list.spec.md#sequence-constructor-seq-and-the--operator)); equivalent to passing the list explicitly. So `t[3:10]` returns the sub-table of rows 3 through 10. |
| `[i1, i2, …]` | the rows at those 1-based indices, in the supplied order (duplicates deduped) | the list is `flatten`ed first, so ranges may be embedded — `t[[3, 7:15, 20]]` selects rows 3, 7–15, 20; duplicate indices are deduped (first occurrence wins); out-of-range indices are silently skipped (the result simply omits them) |
| *predicate* | rows where the boolean expression holds; columns are in scope by name (`item` is the whole row) | the filter form ([§ Operators](#operators)) |
| *empty* (`t[, c]`) | all rows | the comma must be present |

A `bl.BlString` is **not** a valid row selector — `t["foo"]` is a
`bl.TypeError`. Strings appear only in the column selector (single name or
list of names).

### Column selector (second slot)

| Form | Selects | Notes |
|---|---|---|
| *absent* (i.e. `t[i]`) | all columns | the no-comma form, equivalent to the original row-indexing behaviour |
| `"myColName"` | one column by name | unknown column → `null` cells |
| `["col1", "col2", …]` | the named columns in the supplied order | order is the column selector's order, not the table's canonical order; unknown column → `null` cells |

### Result-shape matrix

The result **type** is always `bl.BlTable`. The selectors only change the
sub-table's *shape* — its row count and column set:

- The row selector sets the rows: a single index → 1 row, a range or index
  list → that many rows, a predicate → the matching rows, empty → all rows.
- The column selector sets the columns: absent → all columns, a single name →
  one column, a name list → those columns.

| Row × Column | Result shape | Example | Example result |
|---|---|---|---|
| single × *absent* | 1 row, all columns | `t[1]` | a 1-row sub-table |
| single × single | 1 row, 1 column | `t[1, "rate"]` | a 1×1 sub-table |
| single × multi | 1 row, listed columns | `t[1, ["region", "rate"]]` | a 1-row sub-table |
| multi × *absent* | selected rows, all columns | `t[1:2]` | a 2-row sub-table |
| multi × single | selected rows, 1 column | `t[1:2, "rate"]` | a 2-row single-column sub-table |
| multi × multi | selected rows, listed columns | `t[1:10, ["region", "rate"]]` | a sub-table |
| empty × single | all rows, 1 column | `t[, "rate"]` | a single-column sub-table |
| empty × multi | all rows, listed columns | `t[, ["region", "rate"]]` | equivalent to `t.select("region", "rate")` |
| predicate × *absent* | filtered rows, all columns | `t[rate > 10]` | rows where rate > 10 |
| predicate × single | filtered rows, 1 column | `t[rate > 10, "rate"]` | single-column sub-table of qualifying rows |
| predicate × multi | filtered rows, listed columns | `t[rate > 10, ["region", "rate"]]` | a sub-table of qualifying rows |

Because the type never changes, switching row or column selectors — `t[1]` →
`t[1:3]`, or `t[1, "c"]` → `t[1:3, "c"]` — only changes the sub-table's
dimensions, never what kind of value you get back.

### Paths to a single cell value

Bracket indexing never yields a bare cell — `t[1, "rate"]` is a 1×1
`bl.BlTable`, not `5.99`. To reach a single cell *value*:

| Path | Description | Example |
|---|---|---|
| `t[i, "col"].toValue()` | index to a 1×1 sub-table, then unwrap it (see [§ Unwrapping a table](#unwrapping-a-table)) | `t[1, "rate"].toValue()` → `5.99` |
| `t.col[i]` | `t.col` is a `bl.BlList` (per [§ Column indexing](#column-indexing)); `[i]` indexes it to a cell | `t.rate[1]` → `5.99` |
| inside a `for` body | `row.col` when `row` is the loop variable | `for row in t return row.rate` |

Note that `t[i].col` does **not** reach a cell either — `t[i]` is a 1-row
`bl.BlTable`, so `.col` on it projects the column over its single row and
yields a 1-element `bl.BlList`, not a value. Use `t.col[i]` for the direct
cell value.

### Why no numeric column positions

`c` must be a `bl.BlString` (a column name); positional column indexing by
number (e.g. `t[1, 2]` meaning "row 1, second column") isn't supported.
Columns are stored in canonical sorted order rather than declaration order,
so positional indexing would refer to columns based on alphabetical ordering
of names — surprising and brittle. Use `t[i, t.colNames[k]]` if you
genuinely need column-by-position access (the inner `t.colNames[k]` returns
the column name at position `k`); like any bracket form it yields a 1×1
`bl.BlTable`, so unwrap it if you want the cell value.

### Edge cases for the bracket form

- `t[]` (zero args) — parse error.
- `t[a, b, c, …]` (three or more args) — parse error.
- Out-of-range single row index `t[100]` → empty `bl.BlTable`. A column
  selector doesn't change that: `t[100, "col"]` and `t[100, ["col"]]` both
  return an empty (single- or multi-column) `bl.BlTable`, never `null`.
- Out-of-range row index inside a list `t[[1, 100], cols]` → the
  out-of-range entry is silently skipped; the result has fewer rows than the
  index list's length.
- Unknown column name in a column selector → cells for that column are
  `null`. Sub-tables include the column with `null` cells.
- Column selector that names duplicate columns (e.g. `["rate", "rate"]`) —
  the duplicate column appears once in the result. Use `t.rename(...)` if you
  need it under different names.
- Row selector list with duplicate indices (e.g. `[1, 1, 2]`) — duplicates are
  silently deduped, so the row appears once (first occurrence wins) and the
  result is rows 1, 2 in order. (Set semantics, not list-of-indices semantics.)
- Predicate evaluation cost: the predicate runs once per row in the table.
  For large tables this scales linearly. The column selector is applied
  after filtering, so it doesn't multiply the cost.

`[@test] ../../core/table_test.go`

---

## Attributes

A `bl.BlTable` exposes three **attributes** describing its shape, read with
bare dot-path syntax (no parentheses, unlike the unwrap methods below):

| Attribute | Returns |
|---|---|
| `t.nRows` | `bl.BlNumber` — the row count |
| `t.nCols` | `bl.BlNumber` — the column count |
| `t.colNames` | `bl.BlList<bl.BlString>` — the column names, in the table's canonical column order |

```
// expression-language — t is a 3-row, 2-column table (region, rate)
t.nRows            // → 3
t.nCols            // → 2
t.colNames         // → ["rate", "region"]   (canonical sorted order)
```

Like column projection `t.col`, these are resolved by name on the table type,
so `nRows`, `nCols`, and `colNames` are **reserved** — a column literally
named `nRows` is shadowed by the attribute and must be projected with the
bracket form `t[, "nRows"]`.

`[@test] ../../core/table_test.go`

---

## Unwrapping a table

Every bracket form returns a `bl.BlTable` (see
[§ Row and column indexing](#row-and-column-indexing)). Three **methods**
collapse a table back to a plain value. They use method-call syntax —
`t.toList()`, `t.toDict()`, `t.toValue()` — and each has a single, fixed
return type, so the result type never depends on the table's runtime shape:

| Method | Returns | Behaviour |
|---|---|---|
| `t.toList()` | `bl.BlList` | a single-column table → its cell values (`bl.BlList<bl.BlValue>`); any wider table → its rows (`bl.BlList<bl.BlDictionary>`). Empty table → `[]`. |
| `t.toDict()` | `bl.BlDictionary` | the sole row as a dictionary. **Requires exactly one row** — zero or many rows → `bl.TypeError`. |
| `t.toValue()` | `bl.BlValue` | the sole cell. **Requires exactly one row and one column** — any other shape → `bl.TypeError`. |

```
// expression-language  — t is a 3-row, 2-column table (region, rate)
t.toList()                 // → [{region:"domestic", rate:5.99}, …]  the rows
t[, "rate"].toList()       // → [5.99, 15.99, 25.99]                 one column → its cells
t[1].toDict()              // → {region:"domestic", rate:5.99}       the one row
t[1, "rate"].toValue()     // → 5.99                                 the one cell

t.toDict()                 // → bl.TypeError — 3 rows, not 1
t[, "rate"].toValue()      // → bl.TypeError — 3 rows, not a single cell
t[1].toValue()             // → bl.TypeError — 1 row but 2 columns
```

These three are the **unwrap** methods; the table also has the transformation
methods (`filter`, `filterOut`, `select`, `rename`, `sort`, `slice`, `distinct`,
`withColumn`, `join` — see [§ Transformation methods](#transformation-methods))
and `union` (see [§ Stacking tables — union](#stacking-tables--union)). Together
these are the table's `t.method()` forms. The parentheses distinguish a method
call from column projection, so a column literally named `toList` is still
reachable by the bare path `t.toList` or by `t[, "toList"]`.

`t.col` is exactly `t[, "col"].toList()`: `t[, "col"]` is a single-column
table, and `t.toList()` returns a single-column table's cells as a list (see
[§ Column indexing](#column-indexing)).

`[@test] ../../core/table_test.go`

---

## Built-in functions

All blkit extensions (**ext** — no DMN equivalent); DMN treats relations as lists.

| Function | Example | Result |
|---|---|---|
| `table(names, rows…)` | `table(["a"], [1], [2])` | a validated `bl.BlTable` from a column-name header + value rows; column types inferred from cells (see [§ Literals](#literals)) |
| `tableFromDicts(listOfDictionaries)` | `tableFromDicts([{a:1},{a:2}])` | a validated `bl.BlTable` from a list of uniformly-keyed dictionaries |
| `isEmpty(t)` | `isEmpty(tableFromDicts([]))` | `true` (inherits list `isEmpty`) |
| `hasColumn(t, name)` | `hasColumn(t, "rate")` | `true` |
| `union(t, others…[, how])` | `union(q1, q2)` | new table stacking all rows of `t` then each other table (UNION ALL — duplicates kept; `how` reconciles mismatched columns, see [§ Stacking tables](#stacking-tables--union)); also the method `t.union(...)` |
| `join(t, other, on[, how])` | `join(t, orders, "id", "left")` | relational join (see [§ Joining tables](#joining-tables--join)); also the method `t.join(...)` |
| `asc(column)` / `desc(column)` | `desc("rate")` | a **sort key** (ascending / descending on `column`) for `t.sort(...)` — see [§ Sort keys](#sort-keys) |
| `inOrder(column, order)` | `inOrder("region", ["europe", "domestic"])` | a **sort key** ranking `column` by an explicit value `order`; for `t.sort(...)` — see [§ Sort keys](#sort-keys) |

The relational transformation verbs — `filter`, `filterOut`, `select`, `rename`, `sort`,
`slice`, `distinct`, `withColumn` — are **methods**, not functions; see
[§ Transformation methods](#transformation-methods). `union` and `join` carry both
surfaces. `asc` / `desc` / `inOrder` are functions only — they build the sort keys that
`t.sort(...)` consumes.

There is no `addRow` / `removeRow` / `drop` built-in. Compose instead: **append rows**
by `union`-ing a single-row table (`t.union(tableFromDicts([{region: "intl", rate: 25.99}]))`),
**remove rows** by filtering (`t[predicate]` / `t.filterOut(p)`) or slicing
(`t.slice(i:j)`), and **drop columns** by selecting the ones to keep (`t.select("region")`).

A table is also a list (over its rows), so the list built-ins apply directly —
`isEmpty`, `sum(t.rate)` (sum a projected column), `for x in t return …` (per-row
comprehension), `some r in t satisfies r.rate > 10`, `every r in t satisfies r.region !=
""`, predicate filter `t[rate > 10]`. See [list.spec.md](list.spec.md) for the
full list library. For row and column counts use the `t.nRows` / `t.nCols`
attributes (see [§ Attributes](#attributes)).

`[@test] ../../core/table_test.go`

---

## Transformation methods

The relational transformation verbs use **method-call** syntax — `t.method(...)`. They
are *not* registered as user-callable functions (the two exceptions, `union` and `join`,
also have function forms); the patcher recognises the `t.method()` surface and lowers it
to a backing call, the same way component access `x.year` lowers to `dateYear(x)`. Every
one returns a fresh `bl.BlTable` (immutability), so they chain:

```
// expression-language
shippingRates
  .filter(rate > 5)                      // keep rows where the predicate holds
  .withColumn("with_tax", rate * 1.2)    // derive a column from each row
  .select("region", "with_tax")          // keep just these columns, in this order
  .sort(desc("with_tax"))                // stable sort, descending
```

| Method | Example | Result |
|---|---|---|
| `t.filter(predicate)` | `t.filter(rate > 10)` | rows where `predicate` holds; columns are in scope by name (`item` is the whole row — see [§ Operators](#operators)). Identical to the bracket filter `t[rate > 10]`; no match → empty table (columns preserved). |
| `t.filterOut(predicate)` | `t.filterOut(rate > 10)` | the **complement** — rows where `predicate` does *not* hold. `t.filterOut(p)` ≡ `t.filter(not (p))`. |
| `t.select(names…)` | `t.select("region", "rate")` | new table with only the named columns, **in the listed order**. Unknown column → `bl.TypeError`. (The method form of the former `project` built-in.) |
| `t.rename(from, to)` | `t.rename("rate", "price")` | new table with column `from` renamed to `to`. Unknown `from`, or `to` colliding with an existing column → `bl.TypeError`. |
| `t.sort(keys…)` | `t.sort("region", desc("rate"))` | **stable** multi-column sort by one or more **sort keys**, precedence left→right. A key is a bare column name (ascending), `asc(col)` / `desc(col)`, or `inOrder(col, [values…])` for an explicit value order. Non-comparable cells → `bl.TypeError`. (The method form of the former `sortBy` built-in.) See the [§ Sort keys](#sort-keys) notes. |
| `t.slice(rows)` | `t.slice(2:4)` | the rows picked by the single `rows` selector — a 1-based index, a list of indices, or a range `i:j`. Exactly the bracket row selector `t[rows]`: `t.slice(2:4)` ≡ `t[2:4]`, `t.slice([1, 3])` ≡ `t[[1, 3]]`, `t.slice(2)` ≡ `t[2]`. Same skip-past-end behaviour. |
| `t.distinct()` | `t.distinct()` | duplicate rows removed (full-row equality); first occurrence wins, input order preserved. |
| `t.withColumn(name, expr)` | `t.withColumn("with_tax", rate * 1.2)` | new table with column `name` **added, or replaced in place if it already exists**. `expr` is evaluated per row with the row's columns in scope by name (and `item` bound to the whole row — the same scope as `filter`). |
| `t.join(other, on[, how])` | `t.join(orders, "id", "left")` | relational equi-join — see [§ Joining tables](#joining-tables--join). Also a function: `join(t, other, on[, how])`. |

`filter`, `filterOut` and `slice` are pure conveniences over forms that already exist:
they lower to `t[predicate]`, `t[not (predicate)]`, and `t[rows]` respectively (see
[§ Row and column indexing](#row-and-column-indexing)), so bracket indexing and these
methods are interchangeable. `select` / `rename` / `sort` / `distinct` are the method
spellings of operations the spec previously exposed as the `project` / `rename` /
`sortBy` / `distinct` functions; those function names are **no longer registered**.

### Sort keys

`sort` takes one or more **sort keys** and orders rows by them with **left→right
precedence** — the first key is primary, later keys break ties — so multi-column sorting is
written in one call: `t.sort("region", desc("rate"))` sorts by `region` ascending, then by
`rate` descending within each region. At least one key is required (`t.sort()` →
`bl.TypeError`). A key is one of:

| Key | Meaning |
|---|---|
| `column` (a bare `bl.BlString`) | ascending on `column` — sugar for `asc(column)` |
| `asc(column)` | ascending on `column` |
| `desc(column)` | descending on `column` |
| `inOrder(column, order)` | explicit value order — `column`'s cells are ranked by their position in the `order` list (a `bl.BlList`) |

`asc` / `desc` / `inOrder` are **registered functions** ([§ Built-in functions](#built-in-functions))
that build a sort key; a column named `"asc"`/`"desc"` is therefore never ambiguous
(`desc("asc")` sorts the column `asc` descending). An unknown column in any key →
`bl.TypeError`.

`sort` inherits the comparability rules of the old `sortBy`: for `asc` / `desc` keys, cells
comparing as `bl.Null` (e.g. naive-vs-zoned datetime comparison) sort to the **end** under
`asc`, and **lead** under `desc`. A column whose cells aren't mutually comparable →
`bl.TypeError` (see [§ Per-column value type](#per-column-value-type)).

An `inOrder(column, order)` key ranks rows by the position of their `column` cell in `order`
(matched by `bl.BlValue` equality). Values listed earlier in `order` come first; **duplicate
entries in `order` are ignored after their first occurrence**. Rows whose cell value is
**not** in `order` are ranked after every listed value, in ascending fallback order (the
same `asc` rules, so `bl.Null` cells sort last). Because listed values are matched by
equality, the listed portion needs no ordering relation; only the unlisted fallback requires
the column's cells to be mutually comparable, so an unlisted, non-comparable cell →
`bl.TypeError`. The `order` list may name values not present in the column — they simply
match no rows.

The whole sort is **stable**: rows that compare equal across *all* keys keep their input
order, as do rows tied within an `inOrder` key's trailing unlisted group.

`[@test] ../../core/table_test.go`

---

## Joining tables — join

`join` combines two tables by matching rows on one or more shared **key columns** (an
equi-join), or — with `how = "cross"` — by pairing every row combination (a Cartesian
product). Like `union` it has both a function form and a method form, which lower to
the same call:

```
// expression-language
join(rates, orders, "region")          // function form, inner join (default)
rates.join(orders, "region")           // method form — sugar for join(rates, orders, "region")
rates.join(orders, ["region", "tier"]) // composite key
rates.join(orders, "region", "left")   // explicit join type
```

**Keys (`on`).** A single column name (`bl.BlString`) or a list of names
(`["region", "tier"]`). Every named key must exist in **both** tables — otherwise
`bl.TypeError`. Two rows match when their cells are equal across *all* key columns (under
`bl.BlValue` equality). An empty key list `[]` is a `bl.TypeError` for every join type
**except** `"cross"`, which takes no keys (and rejects a non-empty `on`).

**Join type (`how`).** An optional `bl.BlString`, defaulting to `"inner"`:

| `how` | Rows in the result |
|---|---|
| `"inner"` | only left rows that have a matching right row |
| `"left"` | every left row; right-side cells `null`-filled when there's no match |
| `"right"` | every right row; left-side cells `null`-filled when there's no match |
| `"outer"` | every row from both sides; the absent side `null`-filled |
| `"cross"` | every left row paired with every right row (Cartesian product); no keys |

Any other string → `bl.TypeError`. `"cross"` requires an empty key list `[]` — passing
keys with `"cross"`, or `[]` with any other `how`, is a `bl.TypeError`.

**Result columns.** The key column(s) **once**, then the non-key columns of `t`, then the
non-key columns of `other`, in that order. A `"cross"` join has no keys, so it is simply
every column of `t` then every column of `other`. Either way, a non-key column name
present in *both* tables is ambiguous → `bl.TypeError`; `t.rename(...)` one side first so
the result's columns stay unique. The uniform-keys invariant holds on the result.

**Cardinality & order.** When several rows on one side match a row on the other, the
result emits **one row per matching pair** (SQL join semantics), so a join is *not*
guaranteed to preserve row count. `inner`/`left` results follow `t`'s row order, and
within each left row its matching `other` rows in `other`'s order; `right` follows
`other`'s order; `outer` emits the left-ordered rows (matched and unmatched) followed by
the unmatched `other` rows in `other`'s order. A `"cross"` join emits
`t.nRows × other.nRows` rows, each left row paired with every `other` row in order
(left-major).

| Expression | Result |
|---|---|
| `rates.join(orders, "region")` | inner equi-join on `region` |
| `rates.join(orders, ["region", "tier"])` | inner join on the composite key |
| `rates.join(orders, "region", "left")` | every `rates` row; `orders` cells `null` where no match |
| `join(rates, orders, "region", "outer")` | full outer join (function form) |
| `rates.join(orders, [], "cross")` | Cartesian product — every `rates` row paired with every `orders` row |
| `rates.join(orders, "missing")` | `bl.TypeError` — key absent in one side |
| `rates.join(orders, "region", "cross")` | `bl.TypeError` — `"cross"` takes no keys (pass `[]`) |
| `rates.join(orders, "region", "full")` | `bl.TypeError` — unknown join type |
| `rates.join(orders, [])` | `bl.TypeError` — empty key requires `how = "cross"` |

`[@test] ../../core/table_test.go`

---

## Stacking tables — union

`union` stacks two or more tables vertically: it returns a new `bl.BlTable` whose
rows are every operand's rows concatenated in argument order. It is **UNION ALL**
semantics — duplicate rows are **retained**, not collapsed; chain `.distinct()`
when you want set-union behaviour (`union(a, b).distinct()`).

It has both a function form and a method form, which lower to the same call:

```
// expression-language
union(q1, q2)            // function form
q1.union(q2)             // method form — sugar for union(q1, q2)
union(q1, q2, q3)        // variadic: two or more tables
q1.union(q2, q3)         // method form, variadic
union(q1, q2, "all")     // reconcile mismatched columns (see how, below)
```

**Column reconciliation (`how`).** An optional trailing `bl.BlString` (a string is never
a table operand, so it's unambiguously the mode), defaulting to `"error"`:

| `how` | When operands' column sets differ |
|---|---|
| `"error"` | any mismatch — an extra, missing, or differently-spelled key in any operand — is a `bl.TypeError`. The strict default. |
| `"all"` | keep the **union** of all columns; a row missing a column gets a `null` cell. Column order: the first operand's columns, then each later operand's new columns in first-seen order. |
| `"common"` | keep only the columns present in **every** operand (the intersection); other columns are dropped. Column order follows the first operand, restricted to the common set. |

When every operand already shares the same column set (the same keys, order-independent —
the same rule as table equality, see [§ Equality](#equality)), all three modes produce
the same result. With `"all"` / `"common"` each output row is rebuilt against the
reconciled column set, so the uniform-keys invariant still holds; `"common"` with no
column shared by all operands yields a zero-column table. Either way the result's column
order follows the **first** operand (extended or restricted per `how`).

| Expression | Result |
|---|---|
| `union(q1, q2)` | rows of `q1` followed by rows of `q2`, duplicates kept |
| `q1.union(q2)` | identical — the method form |
| `union(q1, q2, q3)` | all three stacked, in argument order |
| `union(q1)` | `q1` unchanged — a single operand is the identity |
| `union(q1, q2.select("region"))` | `bl.TypeError` — column sets differ (default `"error"`) |
| `union(q1, q2.select("region"), "all")` | stacked; `q1`'s extra columns `null`-filled for `q2`'s rows |
| `union(q1, q2.select("region"), "common")` | stacked over the shared columns only |
| `union(q1, q2).distinct()` | UNION ALL then dedupe → relational set union |

Because a zero-row table still carries its columns, `union(t, empty)` where
`empty` has `t`'s columns but no rows returns `t`'s rows unchanged. A no-column
`table([])` unions cleanly under `"all"` (it contributes no columns and no rows); under
the default `"error"` it only matches another no-column table.

`[@test] ../../core/table_test.go`

---

## Grouping — groupBy and agg

`groupBy` partitions a table's rows into groups that share the same value(s) in one or more
**key columns**, producing a transient `bl.BlGroupedTable`. Its sole operation is `agg`,
which collapses each group to a single row, returning a new `bl.BlTable` with the key
column(s) followed by the named aggregate columns. `groupBy` is a **method only** (no
function form) and **must** be consumed by `agg`: a bare `t.groupBy(…)` not followed by
`.agg(…)` is a `bl.ParseError` at construction.

```
// expression-language
shippingRates
  .groupBy("region")
  .agg(
    "total",   sum(rate),       // sum of the group's rate column
    "avgRate", mean(rate),
    "n",       count(item),     // group size — item is the group's rows
  )
// → one row per region; columns: region, total, avgRate, n
```

**Keys.** `groupBy(names…)` takes one or more column names (`bl.BlString`); at least one is
required (`t.groupBy()` → `bl.TypeError`) and an unknown column → `bl.TypeError`. Rows are
grouped by **key-tuple equality** under `bl.BlValue` equality across all key columns — two
rows share a group when every key cell is equal. `bl.Null` key cells group together (all
rows whose key is null form one group).

**Aggregation expressions.** `agg(name, expr, …)` takes alternating `bl.BlString` names and
aggregate expressions; an odd argument count, or a non-string in a name position, →
`bl.TypeError`. Each `expr` is **captured unevaluated** (the same mechanism as
[`withColumn`](#transformation-methods)) and run once per group with the group's columns in
scope **as lists** — a bare column name is the `bl.BlList` of that column's cells across the
group's rows — and `item` bound to the group's rows as a sub-`bl.BlTable`. As in
`withColumn`, bare names resolve to columns only; reach an enclosing binding with the
`env.` pronoun (`env.taxRate`). So the list aggregates
([list.spec.md § Built-in functions](list.spec.md#built-in-functions)) apply directly:
`sum(rate)`, `mean(rate)`, `min(rate)`, `max(rate)`, `count(item)` (group size), and
arbitrary expressions like `sum(for r in item return r.rate * r.qty)`.

**Result shape.** The result's columns are the key column(s) in `groupBy` order, then the
aggregate columns in `agg` order. An aggregate name colliding with a key column or with
another aggregate name → `bl.TypeError`. There is **one row per group**, in the **first
appearance order** of each group's key (stable, matching `distinct`). The uniform-keys and
per-column value-type invariants hold on the result (an aggregate whose value type differs
across groups → `bl.TypeError`).

| Expression | Result |
|---|---|
| `t.groupBy("region").agg("n", count(item))` | `region` + row count per region |
| `t.groupBy("region", "tier").agg("total", sum(rate))` | composite-key grouping; one row per (region, tier) |
| `t.groupBy("region")` (no `.agg`) | `bl.ParseError` — `groupBy` must be consumed by `agg` |
| `t.groupBy()` | `bl.TypeError` — at least one key column required |
| `t.groupBy("region").agg("region", count(item))` | `bl.TypeError` — aggregate name collides with the key column |
| `t.groupBy("region").agg("a", sum(rate), "b")` | `bl.TypeError` — dangling name with no expression |

Grouping over a zero-row table has no groups, so `.agg(…)` yields a zero-row table that
still carries the key and aggregate columns.

`[@test] ../../core/table_test.go`

---

## Semantics & behaviour

### Uniform-keys invariant

Every row in a `bl.BlTable` has the same set of keys. The constructors and every
operation that produces a table enforce this — the expression-language `table(...)` and
`tableFromDicts(...)` built-ins validate the constraint on their input, and `union` /
`withColumn` / `join` re-validate their results.

### Per-column value type

Every cell in a column shares one value type — if column `region` is `bl.BlString` in
one row it is `bl.BlString` in every row. Both constructors and every mutator enforce this
alongside the uniform-keys invariant: a row whose cell type differs from the column's type
is a `bl.TypeError`. Both `table(...)` and `tableFromDicts(...)` infer each column's type
from its cells. `bl.Null()` is the sole exception — a null cell is permitted in any column,
standing for an absent value.

A single value type does not imply comparability: a uniformly-typed column may still be
non-orderable (e.g. `bl.BlDictionary` cells), so `t.sort(...)` fails with `bl.TypeError`
when asked to order a column whose cells aren't mutually comparable.

### Immutability

Tables are immutable values. Every operation that "modifies" a table returns a fresh
`bl.BlTable` sharing immutable inner values (`bl.BlDictionary` rows are themselves
immutable, so the new table shares the unchanged row dictionaries by reference).

### Equality

`t1 = t2` is true when their column sets are identical (same keys, regardless of order)
**and** their row sequences match position-by-position under `bl.BlDictionary` equality.
Two tables with the same rows in different orders are **not equal** — sort one with
`t.sort(col)` first to normalise. Two tables with the same row data but different
column sets are **not equal**.

### Relationship to `bl.BlList`

A `bl.BlTable` **is** a `bl.BlList` of `bl.BlDictionary` values — the engine accepts a
`bl.BlTable` wherever a `bl.BlList` is expected, and the inherited list operations
(indexing, projection, filter, `for … return`, `some` / `every`, aggregates) all work
without conversion. The reverse is not automatic: a `bl.BlList` of dictionaries becomes
a `bl.BlTable` only via the `tableFromDicts(...)` built-in (or the host-side `bl.Table(...)`),
which validates uniform keys.

---

## Go implementation (expr extension)

Lives in `table.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`bl.BlTable` wraps an ordered column list plus a `[]bl.BlDictionary` of rows enforcing the
uniform-keys invariant. It implements `bl.BlValue` and is list-compatible.

```go
// host-side (Go)
type BlTable struct {
    columns []string
    rows    []BlDictionary
}

// bl.BlValue interface — required by all Bl* value types.
func (BlTable) Type() Type { return TypeTable }
func (t BlTable) Equal(other BlValue) BlValue   // row-wise, order-sensitive; column set is part of identity
func (t BlTable) String() string                // canonical list-of-dictionaries literal
func (BlTable) isBlValue() {}

// Host constructor — a typed header plus positional value rows. Validates the header's
// well-formedness, each row's length against the column count, and each non-nil cell's
// type against its column. See § Construction (host-side).
type Col  struct{ Name string; Type Type }   // one column's name + declared dtype
type Cols []Col                               // ordered, typed header
type Row  []any                               // a row's cell values, positional
func Table(columns Cols, rows ...Row) (BlTable, error)

// Host accessors.
func (t BlTable) Columns() []string                  // declared column order
func (t BlTable) Rows() []BlDictionary               // defensive copy of the row slice
func (t BlTable) Row(i int) (BlDictionary, error)    // 1-based; out-of-range → error
func (t BlTable) ToRecords() []map[string]BlValue    // copy as a plain Go slice of maps
func (t BlTable) ToMarkdown() string                 // aligned markdown table

// Attributes — surfaced as t.nRows / t.nCols / t.colNames.
func (t BlTable) NRows() int                          // row count
func (t BlTable) NCols() int                          // column count
// (t.colNames is backed by Columns() above.)

// Unwrap methods — surfaced as t.toList() / t.toDict() / t.toValue().
func (t BlTable) ToList() BlList                      // 1 column → cell values; else rows (dicts)
func (t BlTable) ToDict() (BlDictionary, error)      // exactly 1 row required; else TypeError
func (t BlTable) ToValue() (BlValue, error)          // exactly 1×1 required; else TypeError

// Transient grouping handle produced by t.groupBy(...); its only consumer is .agg(...).
// Holds the key columns and one sub-table per group, in first-appearance order.
type BlGroupedTable struct {
    keys   []string
    groups []BlTable
}
func (BlGroupedTable) Type() Type { return TypeGroupedTable }
func (BlGroupedTable) isBlValue() {}

// Transient sort key produced by asc(col) / desc(col) / inOrder(col, order); consumed by
// t.sort(keys…). `order` is non-nil only for the inOrder form.
type BlSortKey struct {
    column     string
    descending bool
    order      BlList   // explicit value order (inOrder); nil for asc/desc
}
func (BlSortKey) Type() Type { return TypeSortKey }
func (BlSortKey) isBlValue() {}
```

### Backing implementations (unexported, suffix `Fn`)

```go
// host-side (Go)
// table(names, rows…): columnar constructor. args[0] is the BlList of column-name
// BlStrings; args[1:] are the value rows (each a BlList whose cells bind positionally to the
// header). Each column's value type is inferred from its cells and must be uniform down the
// column. Validates each row's length == column count, a uniform non-null type per column,
// and a well-formed header (no duplicate/empty name); else TypeError. A nil/null cell becomes
// BlNull(). Mirrors the host-side bl.Table(bl.Cols{…}, bl.Row{…}…).
func tableFn(args ...BlValue) (BlTable, error)

// tableFromDicts(list): validates that every element is a BlDictionary with identical keys.
func tableFromDictsFn(l BlList) (BlTable, error)

// tableIndex(t, rowSel, colSel): the backing call that `normalise` emits for the comma form
// t[rowSel, colSel] / t[, colSel] (expr can't parse comma/leading-comma brackets — see
// bl-expr.spec.md § Source normalisation). rowSel is an index/range/list/predicate or the
// all-rows marker (from the empty slot); colSel is a column name, list of names, or absent.
// Returns the selected sub-table per § Result-shape matrix; unknown column → null cells.
func tableIndexFn(args ...BlValue) (BlTable, error)

// hasColumn(t, name): schema introspection.
func hasColumnFn(t BlTable, name BlString) BlBoolean

// Method lowerings — schema reshape, return a new table. Surfaced as t.select(...) /
// t.rename(...) / t.distinct(), not registered as functions.
func selectFn(t BlTable, names ...BlString) (BlTable, error)        // unknown column → TypeError; order-preserving
func renameFn(t BlTable, from, to BlString) (BlTable, error)        // unknown from / collision → TypeError
func distinctFn(t BlTable) BlTable                                  // full-row dedup via dictionary equality

// t.sort(keys…): stable multi-column sort (method lowering). Each key is a BlString (a bare
// column name → ascending) or a BlSortKey from asc/desc/inOrder. Keys apply left→right
// (first key primary, later keys break ties). No keys / unknown column → TypeError; a
// non-comparable cell under an asc/desc key (or an inOrder key's unlisted fallback) → TypeError.
func sortFn(t BlTable, keys ...BlValue) (BlTable, error)

// asc / desc / inOrder: registered functions that build a sort key for t.sort(...).
// inOrder's `order` ranks the column's cells by list position; unlisted cells fall back to
// ascending. Unknown column is validated by sortFn (the key only carries the column name).
func ascFn(column BlString) BlSortKey                          // descending = false
func descFn(column BlString) BlSortKey                         // descending = true
func inOrderFn(column BlString, order BlList) BlSortKey        // explicit value order

// t.groupBy(columns…): partition rows into groups sharing the key columns; returns the
// transient grouping handle consumed by agg. No columns / unknown column → TypeError.
// Method lowering; the patcher fuses the groupBy(...).agg(...) chain (a bare groupBy not
// followed by agg is a ParseError at construction).
func groupByFn(t BlTable, columns ...BlString) (BlGroupedTable, error)

// g.agg(name, expr, …): collapse each group to one row → new table (the key columns then
// the named aggregates). Each expr is captured unevaluated and run per group with the
// group's columns bound as lists and `item` bound to the group's rows (a sub-table) — the
// same capture mechanism as withColumn. Odd arg count, non-string name, or a name that
// collides with a key/another aggregate → TypeError.
func aggFn(g BlGroupedTable, pairs ...any) (BlTable, error)   // (BlString name, group-expression)…

// t.withColumn(name, expr): per-row column add-or-replace. `expr` is captured
// unevaluated (like the bracket predicate filter) and run per row with the row's columns
// bound as bare names, `item` bound to the whole row, and `env.` reaching the enclosing
// scope; bare names resolve to columns only and never fall through to `env`. The patcher
// lowers it to a per-row comprehension that augments each row dictionary and re-tables it.
// Replaces the column in place if name exists.
func withColumnFn(t BlTable, name BlString, expr <row-expression>) (BlTable, error)

// union(args…): stack rows of every operand (UNION ALL), return a new table. The
// operands are the leading BlTable args; an optional trailing BlString is `how` ∈
// "error" (default) / "all" / "common", controlling column-set reconciliation.
// "error" + mismatch → TypeError; "all" null-fills missing columns; "common" keeps the
// intersection. A non-string, non-table arg → TypeError.
func unionFn(args ...BlValue) (BlTable, error)

// join(t, other, on[, how]): equi-join, or Cartesian product when how == "cross".
// `on` is a column name or list of names; `how` ∈ "inner" (default) / "left" / "right" /
// "outer" / "cross". Absent key, ambiguous non-key column collision, unknown `how`, an
// empty key list with a non-cross `how`, or a non-empty key with "cross" → TypeError.
func joinFn(t, other BlTable, on BlValue, how ...BlString) (BlTable, error)

// filter / filterOut / slice carry no dedicated Fn: the patcher lowers them to the
// existing bracket forms — t.filter(P) → t[P], t.filterOut(P) → t[not (P)],
// t.slice(rows) → t[rows] — reusing the inherited list filter / index / range machinery.

// t.nRows / t.nCols / t.colNames: attribute accessors (bare dot-path, no parens).
func tableNRowsFn(t BlTable) BlNumber
func tableNColsFn(t BlTable) BlNumber
func tableColNamesFn(t BlTable) BlList                 // column names, canonical order

// t.toList() / t.toDict() / t.toValue(): method-call unwraps. The patcher lowers the
// t.method() and t.attribute surfaces to these accessor calls (cf. component access
// x.year → dateYear(x)); none are registered as user-callable functions.
func tableToListFn(t BlTable) BlList                  // 1 column → values; else rows (dicts)
func tableToDictFn(t BlTable) (BlDictionary, error)   // != 1 row → TypeError
func tableToValueFn(t BlTable) (BlValue, error)       // != 1×1  → TypeError
```

### Registrations (`tableOptions`, unexported)

```go
// host-side (Go)
func tableOptions() []expr.Option {
    return []expr.Option{ // all ext
        // table: variadic columnar constructor — names list, then value rows.
        // tableFn pulls args[0] as the header and args[1:] as rows (column types inferred).
        expr.Function("table",          variadic(tableFn),          new(func(...BlValue) BlTable)),
        // tableFromDicts: validates a list of uniformly-keyed dictionaries.
        expr.Function("tableFromDicts", typed1(tableFromDictsFn),   new(func(BlList) BlTable)),
        // tableIndex: normalise's lowering target for the comma bracket t[r, c] / t[, c].
        expr.Function("tableIndex",     variadic(tableIndexFn),     new(func(...BlValue) BlTable)),
        expr.Function("hasColumn", typed2(hasColumnFn), new(func(BlTable, BlString) BlBoolean)),
        // union: variadic over tables, with an optional trailing BlString `how`. unionFn
        // pulls a trailing string off the args (a string is never a table operand).
        expr.Function("union",     variadic(unionFn),   new(func(...BlValue) BlTable)),
        expr.Function("join",      joinFn,
            new(func(BlTable, BlTable, BlString) BlTable),
            new(func(BlTable, BlTable, BlList) BlTable),
            new(func(BlTable, BlTable, BlString, BlString) BlTable),
            new(func(BlTable, BlTable, BlList, BlString) BlTable)),
        // Sort-key constructors consumed by t.sort(...).
        expr.Function("asc",     typed1(ascFn),     new(func(BlString) BlSortKey)),
        expr.Function("desc",    typed1(descFn),    new(func(BlString) BlSortKey)),
        expr.Function("inOrder", typed2(inOrderFn), new(func(BlString, BlList) BlSortKey)),
    }
}
```

Only the function-form built-ins are registered. The transformation methods
(`filter` / `filterOut` / `select` / `rename` / `sort` / `slice` / `distinct` /
`withColumn`), the grouping methods (`groupBy` / `agg`), the unwrap methods
(`toList` / `toDict` / `toValue`), and the attributes (`nRows` / `nCols` /
`colNames`) are **not** in this list — they aren't user-callable functions. The
patcher recognises the `t.method()` and `t.attribute` surfaces and lowers them:
`select` / `rename` / `sort` / `distinct` to the matching `…Fn` accessors,
`withColumn` to a per-row comprehension, and `filter` / `filterOut` / `slice` to
the bracket forms `t[P]` / `t[not (P)]` / `t[i:j]` — the same way component access
like `x.year` lowers to `dateYear(x)`. `t.sort(keys…)` lowers to `sortFn(t, keys…)`; the
`keys` are ordinary values — bare column strings, or the `bl.BlSortKey`s returned by the
registered `asc` / `desc` / `inOrder` functions, which (unlike the methods) **are** in the
list above.

The grouping chain `t.groupBy(cols…).agg(name, expr, …)` is lowered as a fused
unit: `groupByFn` partitions the rows into a `bl.BlGroupedTable` and `aggFn` runs
each captured aggregate expression per group (columns bound as lists, `item` as the
group's rows), the same capture mechanism as `withColumn`. Because the grouped
handle is transient, `groupBy` must be consumed by `agg`; a bare `t.groupBy(…)` is
a `bl.ParseError` at construction.

`union` and `join` are the two methods that are *also* registered functions:
the patcher lowers their method forms `t.union(others…[, how])` and `t.join(other,
on[, how])` to the function calls `union(t, others…[, how])` and `join(t, other, on[,
how])`, so both surfaces share a single registration each.

**Reuse.** Row indexing, column projection, filtering, and list aggregates are inherited
from the list machinery (`bl.BlTable` satisfies `bl.BlList`, so it's accepted wherever a
`bl.BlList` is). Native Go `[]map[string]any` inputs wrap to `bl.BlTable` automatically
via the engine's input bridge when the maps share keys; non-uniform inputs wrap to
`bl.BlList<bl.BlDictionary>` instead, and the caller must invoke `tableFromDicts(...)`
explicitly to validate.

`[@test] ../../core/table_test.go`

---

## Edge cases

- `table([])` is the empty table with no columns; it gains a column set only when rebuilt
  with rows (e.g. `union`-ed with a non-empty table).
- `bl.Table(bl.Cols{})` is the empty table with no columns. `bl.Table(cols)` with a
  non-empty header but no rows is a zero-row table that still carries those columns.
- To remove rows, filter (`t[predicate]` / `t.filterOut(p)`) or slice (`t.slice(i:j)`) —
  there is no positional row-removal built-in.
- `t.select(names…)` referencing an absent column → `bl.TypeError`. To drop columns,
  `select` the ones to keep.
- `t.select(names…)` is **order-preserving**: the resulting column order matches the
  argument order (not the input column order).
- `t.rename(from, to)` where `from` doesn't exist, or where `to` collides with an
  existing column → `bl.TypeError`.
- `t.sort(keys…)` takes one or more **sort keys** — a bare column name (ascending),
  `asc(col)` / `desc(col)`, or `inOrder(col, order)` — applied with left→right precedence
  (first key primary, later keys break ties). No keys → `bl.TypeError`; an unknown column in
  any key → `bl.TypeError`. The sort is stable across the full key tuple. A bare boolean is
  **not** a direction (use `asc` / `desc`).
- For an `asc` / `desc` key on a column whose cells aren't mutually comparable →
  `bl.TypeError`. Cells comparing as `bl.Null` (e.g. naive-vs-zoned datetime comparison)
  sort to the end under `asc`, leading under `desc`.
- `inOrder(col, order)` ranks rows by their cell's position in the `order` list (equality
  match); rows whose value isn't listed are ranked after the listed values in ascending
  fallback order, stably. Duplicate `order` entries collapse to their first occurrence;
  `order` values absent from the column match no rows. A non-comparable cell among the
  **unlisted** rows → `bl.TypeError` (the listed portion only needs equality, so it imposes
  no ordering relation).
- `t.distinct()` uses row equality, which is order-insensitive over the dictionary's
  keys; row order in the result preserves the first occurrence of each unique row.
- `t.filter(p)` and `t.filterOut(p)` are exact complements over rows where `p` is a
  boolean: every row goes to exactly one of them. They are equivalent to `t[p]` and
  `t[not (p)]`; an empty result still carries `t`'s columns.
- `t.slice(rows)` is the bracket row selector `t[rows]` — `rows` is a single index, a
  list, or a range `i:j`. Indices past the end are skipped (the result is shorter), never
  an error; an empty or fully out-of-range selector yields an empty table.
- `t.withColumn(name, expr)` naming an **existing** column replaces it in place (keeping
  its position); a new name is appended. `expr` sees the row's columns bound as bare
  names (and `item` as the whole row), so it can reference other columns (`rate * 1.2`);
  bare names resolve to columns only — reach an enclosing binding with `env.` (`env.rate`).
  The result is re-validated for uniform keys.
- `t.join(other, on[, how])`: a key column absent from either side → `bl.TypeError`; a
  **non-key** column name shared by both sides → `bl.TypeError` (ambiguous result
  column — `rename` one side first); an unknown `how` → `bl.TypeError`. An empty key list
  is valid only with `how = "cross"` (Cartesian product, `t.nRows × other.nRows` rows);
  it's a `bl.TypeError` for any other `how`, and passing keys with `"cross"` is likewise
  a `bl.TypeError`. Multiple matches emit one row per matching pair, so row count is not
  preserved (except `cross`, which is exactly the product).
- `t.groupBy(cols…).agg(name, expr, …)`: `groupBy` needs at least one column and must be
  consumed by `agg` (a bare `t.groupBy(…)` → `bl.ParseError`); an unknown key column, an
  odd `agg` argument count, a non-string aggregate name, or an aggregate name colliding
  with a key or another aggregate → `bl.TypeError`. Groups follow first-appearance key
  order; `bl.Null` keys group together; aggregate expressions see columns as lists and
  `item` as the group's rows. A zero-row table groups to no rows but keeps the key +
  aggregate columns. See [§ Grouping](#grouping--groupby-and-agg).
- `t[i]` with `i` out of range (including `i == 0` and `|i| > t.nRows`) returns
  an empty `bl.BlTable`, not `bl.Null` and not an error — the result type is
  always a sub-table regardless of which row indices the selector hits.
- `t.col` where `col` isn't a declared column returns a list of `bl.Null`s of length
  `t.nRows` (per the list-projection rule), not a single `bl.Null`. See
  [§ Column indexing](#column-indexing).
- `t.toDict()` on a table that doesn't have exactly one row → `bl.TypeError`
  (zero rows and multi-row tables both fail). `t.toValue()` on a table that
  isn't exactly 1×1 → `bl.TypeError`. `t.toList()` never errors on shape — an
  empty table yields `[]`. See [§ Unwrapping a table](#unwrapping-a-table).
- Table equality is `false` when the column sets differ, even if every row's overlapping
  cells match.
- Table equality is `false` when rows match by content but in a different order; use
  `t.sort(col)` to normalise before comparing if order isn't significant to your semantics.
- `union(...)` where operands disagree on their column set → `bl.TypeError` under the
  default `how = "error"`. Pass `"all"` to keep every column (missing cells `null`-filled)
  or `"common"` to keep the intersection. Duplicate rows are **retained** (UNION ALL);
  chain `.distinct()` for relational set union. The result's column order follows the
  first operand.
- `union(t)` with a single operand returns `t` unchanged. Stacking a no-column
  `table([])` onto a table with columns is a mismatch under `"error"` (`bl.TypeError`);
  under `"all"` it contributes nothing and the result is the other operand's rows.

`[@test] ../../core/table_test.go`
