---
name: BlTable
description: The table (relation) type in the blkit expression language — an ordered list of uniformly-keyed dictionaries. Covers row/column access, table built-ins, the inherited list semantics, and the Go layer (bl.BlTable + expr registrations).
targets:
  - ../../expr_table.go
---

# bl.BlTable — the `table` type

`table` is a DMN **relation**: an ordered, immutable list of `dictionary` rows that all
share the same column keys (the "uniform-keys" invariant). The Go value type backing it
is `bl.BlTable`.

Structurally a `table` is a `list` of uniformly-keyed `dictionary`s, so list literals,
indexing, filtering, projection, and the list built-ins ([list.spec.md](list.spec.md),
[bl-expr.spec.md](bl-expr.spec.md)) all apply. The dedicated `table` built-ins
([§ Built-in functions](#built-in-functions)) layer relational operations on top — column
projection, row mutation by row index, schema reshaping (`project` / `drop` / `rename`),
distinct-rows, sort-by-column. See also [dictionary.spec.md](dictionary.spec.md) for the
row shape.

---

## Literals

A literal is the syntactic form for writing a constant value of a type directly inside
an expression (as `[1, 2, 3]` is for a list). **`table` has no dedicated literal form**
— a table is, structurally, a list of uniformly-keyed dictionaries. Write that list
inline and wrap with the `table(...)` built-in if you need the validated `bl.BlTable`
type (e.g. so the table-specific built-ins like `columns(t)` and `sortBy(t, …)` accept
it):

```
// expression-language
// A bare list of uniformly-keyed dictionaries is the table's data shape:
[{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}]

// table(list) validates uniform keys and yields a bl.BlTable:
table([{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}])

// Row access (1-based; negative from end; out of range → null):
table([...])[1]                       // → the first row dictionary
// Column access is list projection over the rows:
table([...]).region                   // → ["domestic", "europe"]
// Filter is list filtering (item is the row dictionary):
table([...])[item.rate > 10]          // → rows with rate > 10
```

`[@test] ../../expr_table_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlTable` via the variadic `bl.Table(rows ...bl.BlDictionary)
(bl.BlTable, error)` constructor. The first row fixes the column set (in iteration order
of the supplied `map[string]bl.BlValue` — keys are sorted to a canonical order per
[dictionary.spec.md](dictionary.spec.md)); every subsequent row must declare the same
keys. An empty argument list yields an empty table with no columns; columns are fixed by
the first `bl.AddRow` call (see [§ Built-in functions](#built-in-functions) — `addRow`
returns a new table, preserving immutability).

```go
// host-side (Go)
var row1, _ = bl.Dictionary(map[string]bl.BlValue{
    "region":      bl.String("domestic"),
    "rate":        bl.Number(5.99),
    "ships_today": bl.Boolean(true),
})
var row2, _ = bl.Dictionary(map[string]bl.BlValue{
    "region":      bl.String("europe"),
    "rate":        bl.Number(15.99),
    "ships_today": bl.Boolean(false),
})
var shippingRates, _ = bl.Table(row1, row2)

// Empty table — columns are fixed by the first addRow call.
var empty, _ = bl.Table()
```

`bl.Table(...)` returns `(bl.BlTable, error)`. The error path fires when:

- Two rows disagree on their key set (extra key, missing key, or differing key spelling)
  → `bl.TypeError`.
- Any row is `bl.Null()` — null rows aren't a valid table shape; if you want a row that
  acts like "all values null", supply a `bl.BlDictionary` whose values are `bl.Null()`.

Tables are immutable: every host-API mutator (`addRow`, `removeRow`, `project`, `drop`,
`rename`, `sortBy`, `distinct`) returns a fresh `bl.BlTable`.

---

## Operators

| Operator | Meaning | Example |
|---|---|---|
| `t[i]` | row by 1-based index (negative counts from end; out-of-range → `null`) | `t[1]`, `t[-1]` |
| `t.col` | column projection — returns a `list` of cell values | `t.region` |
| `t[predicate]` | row filter — `item` is the row dictionary | `t[item.rate > 10]` |
| `=` `!=` | equality (row-wise, order-sensitive; the column set is part of identity) | `t1 = t2` |

Tables have no arithmetic operators and no ordering operators (`<` / `<=` / `>` / `>=`)
— they're not a comparable type. Membership (`in`) is **not** defined for tables; use
`listContains(asList(t), x)` if you need element-style membership, or `count(t[item =
row]) > 0` for whole-row membership.

`[@test] ../../expr_table_operators_test.go`

---

## Row indexing

`t[i]` accesses a row by its 1-based position. The result is the row as a
`bl.BlDictionary`. Indexes count from 1; negative indexes count from the end
(`-1` is the last row, `-2` second-to-last). Out-of-range indexes return
`bl.Null`, not an error — consistent with list indexing
([list.spec.md](list.spec.md)).

| Expression | Result (for a 3-row `t`) |
|---|---|
| `t[1]` | the first row dictionary |
| `t[3]` | the last row |
| `t[-1]` | the last row (negative shorthand) |
| `t[-3]` | the first row |
| `t[0]` | `null` (indexing is 1-based; no zero index) |
| `t[4]` | `null` (out of range) |
| `t[-4]` | `null` (out of range) |

To access many rows at once, use the filter form `t[predicate]` (see
[§ Operators](#operators)) or `sublist(asList(t), start, length)` (see
[list.spec.md § Built-in functions](list.spec.md#built-in-functions)).

`[@test] ../../expr_table_indexing_test.go`

---

## Column indexing

`t.col` projects the column named `col` across every row, returning a
`bl.BlList` of cell values in row order. For column names that aren't valid
identifiers (spaces, special characters), use the comma-empty bracket form
`t[, "col name"]` — see [§ Row and column indexing](#row-and-column-indexing).
Single-argument string brackets `t["col name"]` are **not** a column-access
form; the bracket grammar reserves the single-argument shape for row indices
(numeric) and row filters (predicate), so a single string argument is a
`bl.TypeError`.

| Expression | Result (for a 3-row `t`) |
|---|---|
| `t.region` | `["domestic", "europe", "intl"]` (a list of 3 cells) |
| `t[, "unit price"]` | `[5.99, 15.99, 25.99]` (comma-empty bracket form for non-identifier names) |
| `t.missing` | `[null, null, null]` — projection of an undeclared key yields a `null` cell per row, never a missing value, because the table's uniform-keys invariant means every row is queried |
| `t.col` on an empty table | `[]` — projection of any column on a zero-row table is the empty list |
| `t["region"]` | `bl.TypeError` — single-arg string bracket is not a column accessor |

Column projection follows the same rule as list-of-dictionaries projection
([list.spec.md § List projection](list.spec.md#list-projection-fieldname)) — a
`bl.BlTable` is a `bl.BlList` of uniformly-keyed dictionaries, and `.col` on
that list extracts the `col` field from each element. The table's uniform-keys
invariant guarantees the result length equals `count(t)`.

`[@test] ../../expr_table_indexing_test.go`

---

## Row and column indexing

The bracket form `t[…]` is a two-axis selector — a **row selector** plus an
optional **column selector** — that subsumes single-cell access, row
indexing, column indexing, slicing, and filtering. Both axes have several
shapes; the result type falls out of whether each axis selects one item or
many.

### Row selector (first slot)

| Form | Selects | Notes |
|---|---|---|
| `i` (a `bl.BlNumber`) | one row by 1-based index (negative from end) | out-of-range → `null` (or omitted from the result when paired with a column selector) |
| `i:j` | rows `i` through `j` inclusive (1-based) | lowered to `seq(i, j) = [i, i+1, …, j]` by the `:` operator ([list.spec.md § Sequence constructor](list.spec.md#sequence-constructor-seq-and-the--operator)); equivalent to passing the list explicitly. So `t[3:10]` returns the sub-table of rows 3 through 10. |
| `[i1, i2, …]` | the rows at those 1-based indices, in the supplied order (duplicates allowed) | each out-of-range index contributes a `null` row to the result |
| *predicate* | rows where the boolean expression holds; `item` is the current row dictionary | the filter form ([§ Operators](#operators)) |
| *empty* (`t[, c]`) | all rows | the comma must be present |

A `bl.BlString` is **not** a valid row selector — `t["foo"]` is a
`bl.TypeError`. Strings appear only in the column selector (single name or
list of names).

### Column selector (second slot)

| Form | Selects | Notes |
|---|---|---|
| *absent* (`t[i]`) | all columns | the no-comma form, equivalent to the original row-indexing behaviour |
| `c` (a `bl.BlString`) | one column by name | unknown column → `null` cells |
| `[c1, c2, …]` | the named columns in the supplied order | order is the column selector's order, not the table's canonical order; unknown column → `null` cells |

### Result-type matrix

Combining a row selector with a column selector, the result type is
determined by whether each axis is single (s) or multi (m):

| Row × Column | Result type | Example | Example result |
|---|---|---|---|
| single × *absent* | `bl.BlDictionary` (the row) | `t[1]` | `{region: "domestic", rate: 5.99}` |
| single × single | `bl.BlValue` (the cell) | `t[1, "rate"]` | `5.99` |
| single × multi | `bl.BlDictionary` (row projected to the columns) | `t[1, ["region", "rate"]]` | `{region: "domestic", rate: 5.99}` |
| multi × *absent* | `bl.BlTable` (sub-table, all columns) | `t[1:2]` | a 2-row sub-table |
| multi × single | `bl.BlList` (column values over the selected rows) | `t[1:2, "rate"]` | `[5.99, 15.99]` |
| multi × multi | `bl.BlTable` (sub-table, listed columns) | `t[1:10, ["region", "rate"]]` | a sub-table |
| empty × single | `bl.BlList` (full column) | `t[, "rate"]` | `[5.99, 15.99, …]` — equivalent to `t.rate` |
| empty × multi | `bl.BlTable` (project to the listed columns) | `t[, ["region", "rate"]]` | equivalent to `project(t, "region", "rate")` |
| predicate × *absent* | `bl.BlTable` (filtered) | `t[item.rate > 10]` | rows where rate > 10 |
| predicate × single | `bl.BlList` (filtered + projected to one column) | `t[item.rate > 10, "rate"]` | rates of qualifying rows |
| predicate × multi | `bl.BlTable` (filtered + projected to listed columns) | `t[item.rate > 10, ["region", "rate"]]` | a sub-table of qualifying rows |

### Two equivalent paths to a single cell

Outside the bracket form, the language's general composition rules give two
other ways to reach a single cell:

| Path | Description | Example |
|---|---|---|
| `t[i].col` | `t[i]` is a `bl.BlDictionary`; path access on it returns the cell | `t[1].rate` → `5.99` |
| `t.col[i]` | `t.col` is a `bl.BlList` (per [§ Column indexing](#column-indexing)); `[i]` indexes it | `t.rate[1]` → `5.99` |

Both fall out of general semantics, not table-specific syntax. Use whichever
reads naturally — `t[i].col` when the row is the natural anchor (e.g. inside
a `for row in t` body), `t.col[i]` when you've already named the projection.
The `t[i, "col"]` form from the matrix above is the only table-specific
single-cell syntax.

### Why no numeric column positions

`c` must be a `bl.BlString` (a column name); positional column indexing by
number (e.g. `t[1, 2]` meaning "row 1, second column") isn't supported.
Columns are stored in canonical sorted order rather than declaration order,
so positional indexing would refer to columns based on alphabetical ordering
of names — surprising and brittle. Use `t[i, columns(t)[k]]` if you
genuinely need column-by-position access (the inner `columns(t)[k]` returns
the column name at position `k`).

### Edge cases for the bracket form

- `t[]` (zero args) — parse error.
- `t[a, b, c, …]` (three or more args) — parse error.
- Out-of-range single row index `t[100]` (with no column slot) → `null`.
- Out-of-range row index inside a list `t[[1, 100], cols]` → that row's slot
  in the result is a `null` row (i.e. the resulting sub-table has a row of
  all-`null` cells at that position, since the table contract requires every
  cell to exist).
- Unknown column name in a column selector → cells for that column are
  `null`. Sub-tables include the column with `null` cells; row dictionaries
  include the key with a `null` value.
- Column selector that names duplicate columns (e.g. `["rate", "rate"]`) —
  the duplicate column appears once in the result. Use `rename` if you need
  it under different names.
- Row selector list with duplicate indices (e.g. `[1, 1, 2]`) — the row is
  included once per occurrence, preserving the supplied order. (This matches
  list-of-indices semantics, not set semantics.)
- Predicate evaluation cost: the predicate runs once per row in the table.
  For large tables this scales linearly. The column selector is applied
  after filtering, so it doesn't multiply the cost.

`[@test] ../../expr_table_indexing_test.go`

---

## Built-in functions

All blkit extensions (**ext** — no DMN equivalent); DMN treats relations as lists.

| Function | Example | Result |
|---|---|---|
| `table(listOfDictionaries)` | `table([{a:1},{a:2}])` | a validated `bl.BlTable` |
| `count(t)` | `count(table([...]))` | row count (inherits list `count`) |
| `isEmpty(t)` | `isEmpty(table([]))` | `true` (inherits list `isEmpty`) |
| `columns(t)` | `columns(table([{a:1,b:2}]))` | `["a", "b"]` (declared / inferred order) |
| `hasColumn(t, name)` | `hasColumn(t, "rate")` | `true` |
| `addRow(t, row)` | `addRow(t, {region:"intl", rate:25.99})` | new table (row keys must match) |
| `removeRow(t, index)` | `removeRow(t, 2)` | new table (1-based index; out-of-range → `bl.TypeError`) |
| `project(t, names…)` | `project(t, "region", "rate")` | new table with only the named columns |
| `drop(t, names…)` | `drop(t, "rate")` | new table without the named columns |
| `rename(t, from, to)` | `rename(t, "rate", "price")` | new table with the column renamed |
| `distinct(t)` | `distinct(t)` | duplicate rows removed (full-row equality) |
| `sortBy(t, column[, descending])` | `sortBy(t, "rate", true)` | stable sort by the column's values |
| `asList(t)` | `asList(t)` | the underlying list of dictionary rows |

A table is also a list (over its rows), so the list built-ins apply directly — `count`,
`isEmpty`, `sum(t.rate)` (sum a projected column), `for x in t return …` (per-row
comprehension), `some r in t satisfies r.rate > 10`, `every r in t satisfies r.region !=
""`, predicate filter `t[item.rate > 10]`. See [list.spec.md](list.spec.md) for the
full list library.

`[@test] ../../expr_table_functions_test.go`

---

## Semantics & behaviour

### Uniform-keys invariant

Every row in a `bl.BlTable` has the same set of keys. The constructor and every mutator
enforce this — `addRow` with a mismatched key set returns `bl.TypeError`, and the
expression-language `table(...)` built-in validates the same constraint on the supplied
list.

### Per-cell types are not enforced

A column can hold values of differing types across rows — `bl.BlTable` does not require
that every cell in column `region` is a `bl.BlString`. Built-ins that need a comparable
column (notably `sortBy`) fail with `bl.TypeError` when the column's cells aren't
mutually comparable; everything else (`project`, `drop`, `rename`, `distinct`, row
projection, list aggregates) is type-agnostic.

### Immutability

Tables are immutable values. Every operation that "modifies" a table returns a fresh
`bl.BlTable` sharing immutable inner values (`bl.BlDictionary` rows are themselves
immutable, so the new table shares the unchanged row dictionaries by reference).

### Equality

`t1 = t2` is true when their column sets are identical (same keys, regardless of order)
**and** their row sequences match position-by-position under `bl.BlDictionary` equality.
Two tables with the same rows in different orders are **not equal** — sort one with
`sortBy(t, col)` first to normalise. Two tables with the same row data but different
column sets are **not equal**.

### Relationship to `bl.BlList`

A `bl.BlTable` **is** a `bl.BlList` of `bl.BlDictionary` values — the engine accepts a
`bl.BlTable` wherever a `bl.BlList` is expected, and the inherited list operations
(indexing, projection, filter, `for … return`, `some` / `every`, aggregates) all work
without conversion. The reverse is not automatic: a `bl.BlList` of dictionaries becomes
a `bl.BlTable` only via the `table(...)` built-in (or the host-side `bl.Table(...)`),
which validates uniform keys.

---

## Go implementation (expr extension)

Lives in `expr_table.go`. Shared mechanics in
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

// Host constructor — validates uniform keys.
func Table(rows ...BlDictionary) (BlTable, error)

// Host accessors.
func (t BlTable) Columns() []string                  // declared column order
func (t BlTable) Rows() []BlDictionary               // defensive copy of the row slice
func (t BlTable) Row(i int) (BlDictionary, error)    // 1-based; out-of-range → error
func (t BlTable) ToRecords() []map[string]BlValue    // copy as a plain Go slice of maps
func (t BlTable) ToMarkdown() string                 // aligned markdown table
```

### Backing implementations (unexported, suffix `Fn`)

```go
// host-side (Go)
// table(list): validates that every element is a BlDictionary with identical keys.
func tableFn(l BlList) (BlTable, error)

// columns(t), hasColumn(t, name): schema introspection.
func columnsFn(t BlTable) BlList
func hasColumnFn(t BlTable, name BlString) BlBoolean

// addRow(t, row), removeRow(t, index): row-level mutation, return a new table.
func addRowFn(t BlTable, row BlDictionary) (BlTable, error)         // key-set mismatch → TypeError
func removeRowFn(t BlTable, index BlNumber) (BlTable, error)        // out-of-range → TypeError

// project / drop / rename: schema reshape, return a new table.
func projectFn(t BlTable, names ...BlString) (BlTable, error)       // unknown column → TypeError
func dropFn(t BlTable, names ...BlString) (BlTable, error)          // unknown column → TypeError
func renameFn(t BlTable, from, to BlString) (BlTable, error)        // unknown from / collision → TypeError

// distinct(t): full-row deduplication using dictionary equality.
func distinctFn(t BlTable) BlTable

// sortBy(t, column[, descending]): stable sort by the column's values.
func sortByFn(t BlTable, column BlString, descending ...BlBoolean) (BlTable, error)

// asList(t): the row list (BlTable IS a BlList of BlDictionary, so this is identity-shaped).
func asListFn(t BlTable) BlList
```

### Registrations (`tableOptions`, unexported)

```go
// host-side (Go)
func tableOptions() []expr.Option {
    return []expr.Option{ // all ext
        expr.Function("table",     typed1(tableFn),     new(func(BlList) BlTable)), // validates uniform keys
        expr.Function("columns",   typed1(columnsFn),   new(func(BlTable) BlList)),
        expr.Function("hasColumn", typed2(hasColumnFn), new(func(BlTable, BlString) BlBoolean)),
        expr.Function("addRow",    typed2(addRowFn),    new(func(BlTable, BlDictionary) BlTable)),
        expr.Function("removeRow", typed2(removeRowFn), new(func(BlTable, BlNumber) BlTable)),
        expr.Function("project",   variadic(projectFn), new(func(BlTable, ...BlString) BlTable)),
        expr.Function("drop",      variadic(dropFn),    new(func(BlTable, ...BlString) BlTable)),
        expr.Function("rename",    typed3(renameFn),    new(func(BlTable, BlString, BlString) BlTable)),
        expr.Function("distinct",  typed1(distinctFn),  new(func(BlTable) BlTable)),
        expr.Function("sortBy",    sortByFn,
            new(func(BlTable, BlString) BlTable),
            new(func(BlTable, BlString, BlBoolean) BlTable)),
        expr.Function("asList",    typed1(asListFn),    new(func(BlTable) BlList)),
    }
}
```

**Reuse.** Row indexing, column projection, filtering, and list aggregates are inherited
from the list machinery (`bl.BlTable` satisfies `bl.BlList`, so it's accepted wherever a
`bl.BlList` is). Native Go `[]map[string]any` inputs wrap to `bl.BlTable` automatically
via the engine's input bridge when the maps share keys; non-uniform inputs wrap to
`bl.BlList<bl.BlDictionary>` instead, and the caller must invoke `table(...)` explicitly
to validate.

`[@test] ../../expr_table_test.go`

---

## Edge cases

- `table([])` is the empty table with no columns. The first `addRow` fixes the column
  set; subsequent rows must match.
- `bl.Table()` (no rows) is the empty table with no columns. Same shape.
- `addRow` with a row whose key set differs from the existing columns (extra key,
  missing key, differing spelling) → `bl.TypeError`.
- `removeRow(t, i)` with `i < 1` or `i > count(t)` → `bl.TypeError`. Use `t[predicate]`
  for value-based removal.
- `project(t, names…)` / `drop(t, names…)` referencing an absent column → `bl.TypeError`.
- `project(t, names…)` is **order-preserving**: the resulting column order matches the
  argument order (not the input column order).
- `rename(t, from, to)` where `from` doesn't exist, or where `to` collides with an
  existing column → `bl.TypeError`.
- `sortBy(t, col)` on a column whose cells aren't mutually comparable → `bl.TypeError`.
  Cells comparing as `bl.Null` (e.g. naive-vs-zoned datetime comparison) are sorted to
  the end in ascending order, leading in descending.
- `distinct(t)` uses row equality, which is order-insensitive over the dictionary's
  keys; row order in the result preserves the first occurrence of each unique row.
- `t[i]` with `i` out of range (including `i == 0` and `|i| > count(t)`) returns
  `bl.Null`, not an error.
- `t.col` where `col` isn't a declared column returns a list of `bl.Null`s of length
  `count(t)` (per the list-projection rule), not a single `bl.Null`. See
  [§ Column indexing](#column-indexing).
- Table equality is `false` when the column sets differ, even if every row's overlapping
  cells match.
- Table equality is `false` when rows match by content but in a different order; use
  `sortBy` to normalise before comparing if order isn't significant to your semantics.

`[@test] ../../expr_table_edge_cases_test.go`
