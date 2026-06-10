---
name: BlTable
description: The table (relation) type in the blkit expression language — an ordered list of uniformly-keyed dictionaries. Covers row/column access, table built-ins, the transformation methods (filter/filterOut/select/rename/sort/slice/distinct/withColumn/join), the inherited list semantics, and the Go layer (bl.BlTable + expr registrations).
targets:
  - ../../expr_table.go
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
and `join` carry both surfaces. See also [dictionary.spec.md](dictionary.spec.md) for
the row shape.

---

## Literals

A literal is the syntactic form for writing a constant value of a type directly inside
an expression (as `[1, 2, 3]` is for a list). **`table` has no dedicated literal form**
— a table is, structurally, a list of uniformly-keyed dictionaries. Write that list
inline and wrap with the `table(...)` built-in if you need the validated `bl.BlTable`
type (e.g. so the table-specific methods like `t.sort(…)` and attributes like
`t.colNames` accept it):

```
// expression-language
// A bare list of uniformly-keyed dictionaries is the table's data shape:
[{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}]

// table(list) validates uniform keys and yields a bl.BlTable:
table([{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}])

// Row access (1-based; negative from end; out-of-range → empty sub-table):
table([...])[1]                       // → a 1-row sub-table
// Column access is list projection over the rows:
table([...]).region                   // → ["domestic", "europe"]
// Filter is row filtering (columns are in scope by name):
table([...])[rate > 10]               // → sub-table of rows with rate > 10
```

`[@test] ../../expr_table_test.go`

---

## Construction (host-side)

Host Go code constructs a `bl.BlTable` via the variadic `bl.Table(rows ...bl.BlDictionary)
(bl.BlTable, error)` constructor. The first row fixes the column set (in iteration order
of the supplied `map[string]bl.BlValue` — keys are sorted to a canonical order per
[dictionary.spec.md](dictionary.spec.md)); every subsequent row must declare the same
keys. An empty argument list yields an empty table with no columns — its column set is
fixed once it is rebuilt with rows (e.g. via `union` with a non-empty table).

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

// Empty table — no columns until it is rebuilt with rows.
var empty, _ = bl.Table()
```

`bl.Table(...)` returns `(bl.BlTable, error)`. The error path fires when:

- Two rows disagree on their key set (extra key, missing key, or differing key spelling)
  → `bl.TypeError`.
- Any row is `bl.Null()` — null rows aren't a valid table shape; if you want a row that
  acts like "all values null", supply a `bl.BlDictionary` whose values are `bl.Null()`.

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
`t.filter` / `t.filterOut` predicates, and the `t.withColumn` expression — every column
of `t` is in scope as a **bare name** bound to that row's cell, so `t[rate > 10]` is the
same as `t[item.rate > 10]`. The whole row is also bound to `item` (a `bl.BlDictionary`),
which remains the way to do whole-row access (`item = otherRow`) and to reach columns
whose names aren't valid identifiers (`item["unit price"]`). A bare column name shadows
an equally-named outer binding within the expression; write `item.col` to force the
column reading explicitly.

Tables have no arithmetic operators and no ordering operators (`<` / `<=` / `>` / `>=`)
— they're not a comparable type. Membership (`in`) is **not** defined for tables; use
`listContains(t.toList(), x)` if you need element-style membership, or `t[item =
row].nRows > 0` for whole-row membership.

`[@test] ../../expr_table_operators_test.go`

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
the rows at the listed indices, in the order given, with duplicates allowed.

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
| `t[[1, 1]]` | a 2-row sub-table repeating the first row (duplicates allowed) |
| `t[[1, 100]]` | a 1-row sub-table; the out-of-range `100` is skipped |

To extract a single row as a dictionary, use `t[i].toDict()`. To reach a
single cell value, use `t[i, "col"].toValue()` (see
[§ Unwrapping a table](#unwrapping-a-table)).

The filter form `t[predicate]` (see [§ Operators](#operators)) and
`sublist(t.toList(), start, length)` (see [list.spec.md § Built-in functions](list.spec.md#built-in-functions))
are further ways to take a selection of rows.

`[@test] ../../expr_table_indexing_test.go`

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

`[@test] ../../expr_table_indexing_test.go`

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

### Row selector (first slot)

| Form | Selects | Notes |
|---|---|---|
| `i` (a `bl.BlNumber`) | one row by 1-based index (negative from end) | out-of-range → empty sub-table (still an empty sub-table when paired with a column selector) |
| `i:j` | rows `i` through `j` inclusive (1-based) | lowered to `seq(i, j) = [i, i+1, …, j]` by the `:` operator ([list.spec.md § Sequence constructor](list.spec.md#sequence-constructor-seq-and-the--operator)); equivalent to passing the list explicitly. So `t[3:10]` returns the sub-table of rows 3 through 10. |
| `[i1, i2, …]` | the rows at those 1-based indices, in the supplied order (duplicates allowed) | out-of-range indices are silently skipped (the result simply omits them) |
| *predicate* | rows where the boolean expression holds; columns are in scope by name (`item` is the whole row) | the filter form ([§ Operators](#operators)) |
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
- Row selector list with duplicate indices (e.g. `[1, 1, 2]`) — the row is
  included once per occurrence, preserving the supplied order. (This matches
  list-of-indices semantics, not set semantics.)
- Predicate evaluation cost: the predicate runs once per row in the table.
  For large tables this scales linearly. The column selector is applied
  after filtering, so it doesn't multiply the cost.

`[@test] ../../expr_table_indexing_test.go`

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

`[@test] ../../expr_table_attributes_test.go`

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

`[@test] ../../expr_table_unwrap_test.go`

---

## Built-in functions

All blkit extensions (**ext** — no DMN equivalent); DMN treats relations as lists.

| Function | Example | Result |
|---|---|---|
| `table(listOfDictionaries)` | `table([{a:1},{a:2}])` | a validated `bl.BlTable` |
| `isEmpty(t)` | `isEmpty(table([]))` | `true` (inherits list `isEmpty`) |
| `hasColumn(t, name)` | `hasColumn(t, "rate")` | `true` |
| `union(t, others…[, how])` | `union(q1, q2)` | new table stacking all rows of `t` then each other table (UNION ALL — duplicates kept; `how` reconciles mismatched columns, see [§ Stacking tables](#stacking-tables--union)); also the method `t.union(...)` |
| `join(t, other, on[, how])` | `join(t, orders, "id", "left")` | relational join (see [§ Joining tables](#joining-tables--join)); also the method `t.join(...)` |

The relational transformation verbs — `filter`, `filterOut`, `select`, `rename`, `sort`,
`slice`, `distinct`, `withColumn` — are **methods**, not functions; see
[§ Transformation methods](#transformation-methods). `union` and `join` carry both
surfaces.

There is no `addRow` / `removeRow` / `drop` built-in. Compose instead: **append rows**
by `union`-ing a single-row table (`t.union(table([{region: "intl", rate: 25.99}]))`),
**remove rows** by filtering (`t[predicate]` / `t.filterOut(p)`) or slicing
(`t.slice(i:j)`), and **drop columns** by selecting the ones to keep (`t.select("region")`).

A table is also a list (over its rows), so the list built-ins apply directly —
`isEmpty`, `sum(t.rate)` (sum a projected column), `for x in t return …` (per-row
comprehension), `some r in t satisfies r.rate > 10`, `every r in t satisfies r.region !=
""`, predicate filter `t[rate > 10]`. See [list.spec.md](list.spec.md) for the
full list library. For row and column counts use the `t.nRows` / `t.nCols`
attributes (see [§ Attributes](#attributes)).

`[@test] ../../expr_table_functions_test.go`

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
  .sort("with_tax", true)                // stable sort, descending
```

| Method | Example | Result |
|---|---|---|
| `t.filter(predicate)` | `t.filter(rate > 10)` | rows where `predicate` holds; columns are in scope by name (`item` is the whole row — see [§ Operators](#operators)). Identical to the bracket filter `t[rate > 10]`; no match → empty table (columns preserved). |
| `t.filterOut(predicate)` | `t.filterOut(rate > 10)` | the **complement** — rows where `predicate` does *not* hold. `t.filterOut(p)` ≡ `t.filter(not (p))`. |
| `t.select(names…)` | `t.select("region", "rate")` | new table with only the named columns, **in the listed order**. Unknown column → `bl.TypeError`. (The method form of the former `project` built-in.) |
| `t.rename(from, to)` | `t.rename("rate", "price")` | new table with column `from` renamed to `to`. Unknown `from`, or `to` colliding with an existing column → `bl.TypeError`. |
| `t.sort(column[, descending])` | `t.sort("rate", true)` | **stable** sort by `column`'s values; optional `descending` flag (default ascending). Non-comparable cells → `bl.TypeError`. (The method form of the former `sortBy` built-in.) |
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

`sort` inherits the comparability rules of the old `sortBy`: cells comparing as `bl.Null`
(e.g. naive-vs-zoned datetime comparison) sort to the **end** in ascending order, and
**lead** in descending. A column whose cells aren't mutually comparable → `bl.TypeError`
(see [§ Per-column value type](#per-column-value-type)).

`[@test] ../../expr_table_methods_test.go`

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

`[@test] ../../expr_table_join_test.go`

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

`[@test] ../../expr_table_union_test.go`

---

## Semantics & behaviour

### Uniform-keys invariant

Every row in a `bl.BlTable` has the same set of keys. The constructor and every
operation that produces a table enforce this — the expression-language `table(...)`
built-in validates the constraint on the supplied list, and `union` / `withColumn` /
`join` re-validate their results.

### Per-column value type

Every cell in a column shares one value type — if column `region` is `bl.BlString` in
one row it is `bl.BlString` in every row. The constructor and every mutator enforce this
alongside the uniform-keys invariant: a row whose cell type differs from an existing
column's type is a `bl.TypeError`. `bl.Null()` is the sole exception — a null cell is
permitted in any column, standing for an absent value.

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

// Attributes — surfaced as t.nRows / t.nCols / t.colNames.
func (t BlTable) NRows() int                          // row count
func (t BlTable) NCols() int                          // column count
// (t.colNames is backed by Columns() above.)

// Unwrap methods — surfaced as t.toList() / t.toDict() / t.toValue().
func (t BlTable) ToList() BlList                      // 1 column → cell values; else rows (dicts)
func (t BlTable) ToDict() (BlDictionary, error)      // exactly 1 row required; else TypeError
func (t BlTable) ToValue() (BlValue, error)          // exactly 1×1 required; else TypeError
```

### Backing implementations (unexported, suffix `Fn`)

```go
// host-side (Go)
// table(list): validates that every element is a BlDictionary with identical keys.
func tableFn(l BlList) (BlTable, error)

// hasColumn(t, name): schema introspection.
func hasColumnFn(t BlTable, name BlString) BlBoolean

// Method lowerings — schema reshape, return a new table. Surfaced as t.select(...) /
// t.rename(...) / t.distinct(), not registered as functions.
func selectFn(t BlTable, names ...BlString) (BlTable, error)        // unknown column → TypeError; order-preserving
func renameFn(t BlTable, from, to BlString) (BlTable, error)        // unknown from / collision → TypeError
func distinctFn(t BlTable) BlTable                                  // full-row dedup via dictionary equality

// t.sort(column[, descending]): stable sort by the column's values (method lowering).
func sortFn(t BlTable, column BlString, descending ...BlBoolean) (BlTable, error)

// t.withColumn(name, expr): per-row column add-or-replace. `expr` is captured
// unevaluated (like the bracket predicate filter) and run per row with the row's columns
// bound as bare names and `item` bound to the whole row; the patcher lowers it to a
// per-row comprehension that augments each row dictionary and re-tables the result.
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
        expr.Function("table",     typed1(tableFn),     new(func(BlList) BlTable)), // validates uniform keys
        expr.Function("hasColumn", typed2(hasColumnFn), new(func(BlTable, BlString) BlBoolean)),
        // union: variadic over tables, with an optional trailing BlString `how`. unionFn
        // pulls a trailing string off the args (a string is never a table operand).
        expr.Function("union",     variadic(unionFn),   new(func(...BlValue) BlTable)),
        expr.Function("join",      joinFn,
            new(func(BlTable, BlTable, BlString) BlTable),
            new(func(BlTable, BlTable, BlList) BlTable),
            new(func(BlTable, BlTable, BlString, BlString) BlTable),
            new(func(BlTable, BlTable, BlList, BlString) BlTable)),
    }
}
```

Only the function-form built-ins are registered. The transformation methods
(`filter` / `filterOut` / `select` / `rename` / `sort` / `slice` / `distinct` /
`withColumn`), the unwrap methods (`toList` / `toDict` / `toValue`), and the
attributes (`nRows` / `nCols` / `colNames`) are **not** in this list — they
aren't user-callable functions. The patcher recognises the `t.method()` and
`t.attribute` surfaces and lowers them: `select` / `rename` / `sort` /
`distinct` to the matching `…Fn` accessors, `withColumn` to a per-row
comprehension, and `filter` / `filterOut` / `slice` to the bracket forms
`t[P]` / `t[not (P)]` / `t[i:j]` — the same way component access like `x.year`
lowers to `dateYear(x)`.

`union` and `join` are the two methods that are *also* registered functions:
the patcher lowers their method forms `t.union(others…[, how])` and `t.join(other,
on[, how])` to the function calls `union(t, others…[, how])` and `join(t, other, on[,
how])`, so both surfaces share a single registration each.

**Reuse.** Row indexing, column projection, filtering, and list aggregates are inherited
from the list machinery (`bl.BlTable` satisfies `bl.BlList`, so it's accepted wherever a
`bl.BlList` is). Native Go `[]map[string]any` inputs wrap to `bl.BlTable` automatically
via the engine's input bridge when the maps share keys; non-uniform inputs wrap to
`bl.BlList<bl.BlDictionary>` instead, and the caller must invoke `table(...)` explicitly
to validate.

`[@test] ../../expr_table_test.go`

---

## Edge cases

- `table([])` is the empty table with no columns; it gains a column set only when rebuilt
  with rows (e.g. `union`-ed with a non-empty table).
- `bl.Table()` (no rows) is the empty table with no columns. Same shape.
- To remove rows, filter (`t[predicate]` / `t.filterOut(p)`) or slice (`t.slice(i:j)`) —
  there is no positional row-removal built-in.
- `t.select(names…)` referencing an absent column → `bl.TypeError`. To drop columns,
  `select` the ones to keep.
- `t.select(names…)` is **order-preserving**: the resulting column order matches the
  argument order (not the input column order).
- `t.rename(from, to)` where `from` doesn't exist, or where `to` collides with an
  existing column → `bl.TypeError`.
- `t.sort(col)` on a column whose cells aren't mutually comparable → `bl.TypeError`.
  Cells comparing as `bl.Null` (e.g. naive-vs-zoned datetime comparison) are sorted to
  the end in ascending order, leading in descending.
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
  names (and `item` as the whole row), so it can reference other columns (`rate * 1.2`).
  The result is re-validated for uniform keys.
- `t.join(other, on[, how])`: a key column absent from either side → `bl.TypeError`; a
  **non-key** column name shared by both sides → `bl.TypeError` (ambiguous result
  column — `rename` one side first); an unknown `how` → `bl.TypeError`. An empty key list
  is valid only with `how = "cross"` (Cartesian product, `t.nRows × other.nRows` rows);
  it's a `bl.TypeError` for any other `how`, and passing keys with `"cross"` is likewise
  a `bl.TypeError`. Multiple matches emit one row per matching pair, so row count is not
  preserved (except `cross`, which is exactly the product).
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

`[@test] ../../expr_table_edge_cases_test.go`
