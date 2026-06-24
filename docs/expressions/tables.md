# Tables

> Tabular data and the expression-language syntax that filters, projects,
> groups, and aggregates it.

A **table** is blkit's relation type: an ordered, immutable list of `dictionary`
rows that all share the same column keys. Think of it as a spreadsheet or a SQL
result set — every row has the same columns, and each column holds one value
type. Because a table *is* a list of uniformly-keyed dictionaries underneath, all
the list machinery (indexing, projection, `for`/`some`/`every`, the list
aggregates) works on it directly, and a set of relational operations —
filtering, projection, sorting, joins, grouping, and aggregation — layers on top.

**The table type and every operation on this page are a blkit-specific value
type for working with tabular data.** Everything here — the constructors, the
bracket selectors, and all the relational verbs (`filter`, `select`,
`withColumn`, `groupBy`, `agg`, `join`, …) — is part of the table type. Where a
table reuses an ordinary list operation, such as `isEmpty` or the list
aggregates, that's noted inline.

This page covers how to build a table in an expression, how to read rows and
columns out of it, the transformation methods that reshape it, and grouping and
aggregation. Tables are immutable: every operation that "changes" a table returns
a fresh one, so the verbs chain.

## Building a table

A table has **no literal form** of its own (there's no `[[...]]` syntax). You
build one with one of two constructors. Both validate their input, enforce the
uniform-keys and per-column-type rules, and yield a value the table methods
accept.

### `table(names, rows…)` — the columnar form

Name the columns once, then list the rows. The first argument is the list of
column names; every argument after it is a value row whose cells bind positionally
to that header. Each column's value type is **inferred from its cells** and must
be uniform down the column. This is the CSV-style form — it rides the ordinary
list-literal grammar, keeps cells natively typed, and never repeats a key.

```
// expression-language
table(
  ["region",   "rate",  "ships_today", "ship_date",        "updated_at"],   // column names
  ["domestic", 5.99,    true,          date("2025-03-28"), datetime("2025-03-28T11:45:30")],
  ["europe",   15.99,   false,         date("2025-04-02"), datetime("2025-04-01T09:15:00")],
)
```

A cell may be any expression (`5.99 * 2`, a variable). `table(names)` with no
value rows is a zero-row table that still carries its column names, and
`table([])` is the truly empty table with no columns at all.

### `tableFromDicts(listOfDictionaries)` — adopt dictionary data

When you already have a list of uniformly-keyed dictionaries, wrap it directly.
The column set and per-column types are inferred from the dictionaries.

```
// expression-language
var myTable = tableFromDicts([
  {region: "domestic", rate: 5.99,  ship_date: date("2025-03-28")},
  {region: "europe",   rate: 15.99, ship_date: date("2025-04-02")},
])
```

`tableFromDicts` validates that every element is a dictionary with identical
keys; a list of dictionaries does **not** become a table automatically, so this
(or the host-side `bl.Table(...)`) is how you opt in to relational behaviour.

## Operators

| Operator | Meaning | Example |
|---|---|---|
| `t[i]` | row by 1-based index — returns a 1-row sub-table (negative counts from the end) | `t[1]`, `t[-1]` |
| `t.col` | column projection — returns a `list` of cell values | `t.region` |
| `t[predicate]` | row filter — columns are in scope by name (`item` is the whole row) | `t[rate > 10]` |
| `=` `!=` | equality (row-wise, order-sensitive; the column set is part of identity) | `t1 = t2` |

Tables have **no arithmetic operators and no ordering operators** (`<` / `<=` /
`>` / `>=`) — they aren't a comparable type. Membership (`in`) is not defined for
tables either; use `listContains(t.toList(), x)` for element membership, or
`t[item = row].nRows > 0` for whole-row membership.

### Row-expression scope

Inside a row expression — the bracket predicate `t[…]`, the `filter` /
`filterOut` predicates, and the `withColumn` expression — a **bare name resolves
to a column** of `t`, bound to that row's cell. So `t[rate > 10]` is exactly
`t[item.rate > 10]`. Bare names do **not** fall through to the enclosing scope: a
bare name that isn't a column of `t` is an error, never an outer binding.

The whole row is also bound to `item` (a `dictionary`), which stays the way to do
whole-row access (`item = otherRow`) and to reach columns whose names aren't valid
identifiers (`item["unit price"]`).

To reference a binding from the **enclosing (host) scope** rather than a column,
use the `env.` pronoun: `env.rate` is the outer variable `rate`, regardless of
whether `t` has a `rate` column (`env["unit price"]` for non-identifier names).
This is the deliberate mirror of `item.` — `item.col` always means a column,
`env.name` always means an outer binding — so a collision between a column and an
outer variable is resolved explicitly, never silently.

## Reading rows and columns

The bracket form `t[…]` is a two-axis selector — a **row selector** plus an
optional **column selector**. **Every bracket form evaluates to a table**, no
matter how many rows or columns it picks: brackets never collapse to a bare cell
or a flat list. To pull out a plain list, a single row dictionary, or a single
cell value, use the [unwrap methods](#unwrapping-a-table).

### Row indexing

The row selector accepts a single index `t[i]`, a range `t[i:j]`, or an explicit
list of indices `t[[i1, i2, …]]`. Indexes are **1-based**; negative indexes count
from the end (`-1` is the last row).

A range `i:j` selects rows `i` through `j` inclusive — it lowers to the sequence
`[i, i+1, …, j]`, so `t[3:10]` is exactly `t[[3, 4, …, 10]]`. A list selector
picks rows in the order given; duplicate indices are silently deduped (first
occurrence wins). A list selector is flattened first, so embedded ranges splice
in: `t[[3, 7:15, 20]]` selects rows 3, 7 through 15, and 20.

| Expression | Result (for a 3-row `t`) |
|---|---|
| `t[1]` | a 1-row sub-table containing the first row |
| `t[-1]` | a 1-row sub-table containing the last row |
| `t[1:2]` | a 2-row sub-table containing rows 1 and 2 |
| `t[[1, 3]]` | a 2-row sub-table containing rows 1 and 3 |
| `t[[3, 1]]` | a 2-row sub-table in the listed order: row 3 then row 1 |
| `t[[1, 1]]` | a 1-row sub-table; the duplicate index is deduped |
| `t[[1, 2:3]]` | a 3-row sub-table (rows 1, 2, 3) — the embedded range is flattened in |

### Column projection

`t.col` projects the column named `col` across every row, returning a **list** of
cell values in row order. This dot form is the column-to-list projection;
**square-bracket indexing is not** — `t[, "col"]` returns a single-column *table*,
not a list. (Because of that, `t.col` is exactly `t[, "col"].toList()`.)

`t.col` requires `col` to be a valid identifier. For names that aren't (spaces,
special characters), select by string with the comma-empty bracket form and unwrap
it: `t[, "unit price"].toList()`.

| Expression | Result (for a 3-row `t`) |
|---|---|
| `t.region` | `["domestic", "europe", "intl"]` (a list of 3 cells) |
| `t[, "region"]` | a single-column table, **not** a list |
| `t[, "unit price"].toList()` | `[5.99, 15.99, 25.99]` — non-identifier column selected by string, then unwrapped |
| `t["region"]` | error — a single-argument string bracket is **not** a column accessor |

The bracket grammar reserves the single-argument shape for row indices (numeric)
and row filters (predicate), so a lone string argument like `t["region"]` is a
type error, not column access.

### Row *and* column together

The comma form selects rows and columns at once. The result **type is always a
table** — the selectors only change its *shape*:

- **Row selector** (first slot): a single index → 1 row; a range or index list →
  that many rows; a predicate → the matching rows; *empty* (`t[, c]`) → all rows.
  A string is **not** a valid row selector — `t["foo"]` is an error.
- **Column selector** (second slot): *absent* (i.e. `t[i]`) → all columns; a
  single name `"rate"` → one column; a name list `["region", "rate"]` → those
  columns, in the listed order.

| Row × Column | Example | Result shape |
|---|---|---|
| single × *absent* | `t[1]` | 1 row, all columns |
| single × single | `t[1, "rate"]` | a 1×1 sub-table |
| single × multi | `t[1, ["region", "rate"]]` | 1 row, listed columns |
| multi × single | `t[1:2, "rate"]` | selected rows, 1 column |
| empty × multi | `t[, ["region", "rate"]]` | all rows, listed columns (≡ `t.select("region", "rate")`) |
| predicate × multi | `t[rate > 10, ["region", "rate"]]` | filtered rows, listed columns |

Positional column indexing by number isn't supported (`t[1, 2]` is not "row 1,
second column") — columns are stored in canonical sorted order, so a numeric
column position would be brittle. If you genuinely need column-by-position, go
through the name: `t[i, t.colNames[k]]`.

The bracket form takes one or two selectors: `t[]` (zero args) and `t[a, b, c]`
(three or more) are parse errors. A column selector that names a column twice
keeps it once; a row-index list with duplicates dedupes.

### Paths to a single cell value

Bracket indexing never yields a bare cell — `t[1, "rate"]` is a 1×1 table, not
`5.99`. To reach the value:

| Path | Example | Result |
|---|---|---|
| `t[i, "col"].toValue()` | `t[1, "rate"].toValue()` | `5.99` |
| `t.col[i]` | `t.rate[1]` | `5.99` (`t.rate` is a list; `[i]` indexes it) |
| inside a `for` body | `for row in t return row.rate` | the cells, per row |

Note `t[i].col` does *not* reach a cell: `t[i]` is a 1-row table, so `.col`
projects over its single row and yields a 1-element list. Use `t.col[i]`.

## Attributes

A table exposes three **attributes** describing its shape, read with bare
dot-path syntax (no parentheses):

| Attribute | Returns |
|---|---|
| `t.nRows` | the row count (`number`) |
| `t.nCols` | the column count (`number`) |
| `t.colNames` | the column names (`list` of `string`), in canonical column order |

```
// expression-language — t is a 3-row, 2-column table (region, rate)
t.nRows            // → 3
t.nCols            // → 2
t.colNames         // → ["rate", "region"]   (canonical sorted order)
```

Like column projection, these resolve by name on the table type, so `nRows`,
`nCols`, and `colNames` are **reserved**: a column literally named `nRows` is
shadowed by the attribute and must be projected with `t[, "nRows"]`.

## Unwrapping a table

Three **methods** collapse a table back to a plain value. Each has a single, fixed
return type, so the result never depends on the table's runtime shape:

| Method | Returns | Behaviour |
|---|---|---|
| `t.toList()` | `list` | a single-column table → its cell values; any wider table → its rows (as dictionaries) |
| `t.toDict()` | `dictionary` | the sole row as a dictionary. **Requires exactly one row.** |
| `t.toValue()` | the cell | the sole cell. **Requires exactly one row and one column.** |

```
// expression-language  — t is a 3-row, 2-column table (region, rate)
t.toList()                 // → [{region:"domestic", rate:5.99}, …]  the rows
t[, "rate"].toList()       // → [5.99, 15.99, 25.99]                 one column → its cells
t[1].toDict()              // → {region:"domestic", rate:5.99}       the one row
t[1, "rate"].toValue()     // → 5.99                                 the one cell
```

The parentheses distinguish a method call from column projection, so a column
literally named `toList` is still reachable by the bare path `t.toList` or by
`t[, "toList"]`.

## Built-in functions

The relational transformation **verbs** (`filter`, `select`, `sort`, …) are
*methods*, not functions, and are covered in the next section; `union` and `join`
carry both surfaces.

| Function | Example | Result |
|---|---|---|
| `table(names, rows…)` | `table(["a"], [1], [2])` | a validated table from a column-name header + value rows (types inferred) |
| `tableFromDicts(listOfDictionaries)` | `tableFromDicts([{a:1},{a:2}])` | a validated table from a list of uniformly-keyed dictionaries |
| `isEmpty(t)` | `isEmpty(tableFromDicts([]))` | `true` (applied to the table's rows) |
| `hasColumn(t, name)` | `hasColumn(t, "rate")` | `true` |
| `union(t, others…[, how])` | `union(q1, q2)` | a new table stacking all rows (UNION ALL); also the method `t.union(...)` — see [Stacking tables](#stacking-tables-union) |
| `join(t, other, on[, how])` | `join(t, orders, "id", "left")` | a relational join; also the method `t.join(...)` — see [Joining tables](#joining-tables-join) |
| `asc(column)` / `desc(column)` | `desc("rate")` | a **sort key** (ascending / descending) for `t.sort(...)` — see [Sort keys](#sort-keys) |
| `inOrder(column, order)` | `inOrder("region", ["europe", "domestic"])` | a **sort key** ranking `column` by an explicit value order — see [Sort keys](#sort-keys) |

There is no `addRow` / `removeRow` / `drop` built-in. Compose instead: **append
rows** by `union`-ing a single-row table, **remove rows** by filtering
(`t[predicate]` / `t.filterOut(p)`) or slicing (`t.slice(2:4)`), and **drop
columns** by selecting the ones to keep (`t.select("region")`).

Because a table is also a list over its rows, the list built-ins apply directly —
`isEmpty`, `sum(t.rate)` to sum a projected column, `for x in t return …`,
`some r in t satisfies r.rate > 10`, `every r in t satisfies r.region != ""`, and
the predicate filter `t[rate > 10]`. See [Lists](lists.md) for the full library.

## Transformation methods

The relational verbs use **method-call** syntax, `t.method(...)`. Every one
returns a fresh table (immutability), so they chain:

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
| `t.filter(predicate)` | `t.filter(rate > 10)` | rows where `predicate` holds; columns are in scope by name (`item` is the whole row). Identical to `t[rate > 10]`. |
| `t.filterOut(predicate)` | `t.filterOut(rate > 10)` | the **complement** — rows where `predicate` does *not* hold. `t.filterOut(p)` ≡ `t.filter(not (p))` ≡ `t[not (p)]`. |
| `t.select(names…)` | `t.select("region", "rate")` | a new table with only the named columns, **in the listed order**. Unknown column → error. |
| `t.rename(from, to)` | `t.rename("rate", "price")` | a new table with column `from` renamed to `to`. Unknown `from`, or `to` colliding with an existing column → error. |
| `t.sort(keys…)` | `t.sort("region", desc("rate"))` | a **stable** multi-column sort by one or more **sort keys**, precedence left→right. See [Sort keys](#sort-keys). |
| `t.slice(rows)` | `t.slice(2:4)` | the rows picked by a single `rows` selector — an index, a list, or a range `i:j`. Exactly the bracket row selector: `t.slice(2:4)` ≡ `t[2:4]`. |
| `t.distinct()` | `t.distinct()` | duplicate rows removed (full-row equality); first occurrence wins, input order preserved. |
| `t.withColumn(name, expr)` | `t.withColumn("with_tax", rate * 1.2)` | a new table with column `name` **added, or replaced in place if it already exists**. `expr` is evaluated per row with the row's columns in scope by name (and `item` bound to the whole row). |
| `t.join(other, on[, how])` | `t.join(orders, "id", "left")` | a relational equi-join — see [Joining tables](#joining-tables-join). |

`filter`, `filterOut`, and `slice` are pure conveniences over bracket forms that
already exist (`t[p]`, `t[not (p)]`, `t[rows]`), so the two spellings are
interchangeable.

### Sort keys

`sort` takes one or more **sort keys** and orders rows with **left→right
precedence** — the first key is primary, later keys break ties — so multi-column
sorting is one call: `t.sort("region", desc("rate"))` sorts by `region` ascending,
then by `rate` descending within each region. At least one key is required
(`t.sort()` → error). A key is one of:

| Key | Meaning |
|---|---|
| `column` (a bare string) | ascending on `column` — sugar for `asc(column)` |
| `asc(column)` | ascending on `column` |
| `desc(column)` | descending on `column` |
| `inOrder(column, order)` | explicit value order — `column`'s cells are ranked by their position in the `order` list |

`asc` / `desc` / `inOrder` are registered functions that build a key, so a column
named `"asc"` is never ambiguous (`desc("asc")` sorts the column `asc`
descending).

An `inOrder(column, order)` key ranks rows by the position of their `column` cell
in `order` (matched by value equality). Values listed earlier come first;
**duplicate entries in `order` are ignored after the first occurrence**. Rows
whose cell value is **not** in `order` are ranked after every listed value, in
ascending fallback order. The whole sort is **stable**: rows tied across all keys
keep their input order.

### Deriving columns with `withColumn`

`t.withColumn(name, expr)` evaluates `expr` once per row with the row's columns in
scope as bare names (and `item` bound to the whole row — the same scope as
`filter`). If `name` already exists, the column is replaced **in place** (keeping
its position); otherwise it's appended.

```
// expression-language
shippingRates
  .withColumn("with_tax", rate * 1.2)    // new column from the rate column
  .withColumn("rate", rate)              // replacing "rate" in place keeps its position
```

As with `filter`, bare names resolve to columns only — reach an enclosing binding
with `env.` (`env.taxRate`). The result is re-validated for uniform keys.

## Stacking tables (union)

`union` stacks two or more tables vertically: the result's rows are every
operand's rows concatenated in argument order. It is **UNION ALL** — duplicate
rows are **retained**; chain `.distinct()` for set-union behaviour. It has both a
function form and a method form, which lower to the same call:

```
// expression-language
union(q1, q2)            // function form
q1.union(q2)             // method form — sugar for union(q1, q2)
union(q1, q2, q3)        // variadic: two or more tables
union(q1, q2, "all")     // reconcile mismatched columns (see how, below)
```

**Column reconciliation (`how`).** An optional trailing string (a string is never
a table operand, so it's unambiguously the mode), defaulting to `"error"`:

| `how` | When operands' column sets differ |
|---|---|
| `"error"` | any mismatch — an extra, missing, or differently-spelled key in any operand — is an error. The strict default. |
| `"all"` | keep the **union** of all columns. Order: the first operand's columns, then each later operand's new columns in first-seen order. |
| `"common"` | keep only the columns present in **every** operand (the intersection); other columns are dropped. Order follows the first operand, restricted to the common set. |

When every operand already shares the same column set, all three modes produce the
same result. Either way the result's column order follows the **first** operand.

| Expression | Result |
|---|---|
| `union(q1, q2)` | rows of `q1` then rows of `q2`, duplicates kept |
| `union(q1)` | `q1` unchanged — a single operand is the identity |
| `union(q1, q2.select("region"))` | error — column sets differ (default `"error"`) |
| `union(q1, q2.select("region"), "all")` | stacked over the union of both column sets |
| `union(q1, q2.select("region"), "common")` | stacked over the shared columns only |
| `union(q1, q2).distinct()` | UNION ALL then dedupe → relational set union |

## Joining tables (join)

`join` combines two tables by matching rows on one or more shared **key columns**
(an equi-join), or — with `how = "cross"` — by pairing every row combination (a
Cartesian product). Like `union`, it has a function and a method form that lower
to the same call:

```
// expression-language
join(rates, orders, "region")          // function form, inner join (default)
rates.join(orders, "region")           // method form — sugar for join(rates, orders, "region")
rates.join(orders, ["region", "tier"]) // composite key
rates.join(orders, "region", "left")   // explicit join type
```

**Keys (`on`).** A single column name or a list of names. Every named key must
exist in **both** tables — otherwise an error. Two rows match when their cells are
equal across *all* key columns. An empty key list `[]` is valid **only** with
`"cross"`.

**Join type (`how`).** An optional string, defaulting to `"inner"`:

| `how` | Rows in the result |
|---|---|
| `"inner"` | only left rows that have a matching right row |
| `"left"` | every left row; right-side cells `null`-filled when there's no match |
| `"right"` | every right row; left-side cells `null`-filled when there's no match |
| `"outer"` | every row from both sides; the absent side `null`-filled |
| `"cross"` | every left row paired with every right row (Cartesian product); no keys |

**Result columns.** The key column(s) **once**, then `t`'s non-key columns, then
`other`'s non-key columns. A non-key column name present in *both* tables is
ambiguous → error; `t.rename(...)` one side first. The uniform-keys invariant
holds on the result.

**Cardinality.** When several rows on one side match a row on the other, the
result emits **one row per matching pair** (SQL semantics), so a join is *not*
guaranteed to preserve row count. A `"cross"` join emits exactly
`t.nRows × other.nRows` rows.

| Expression | Result |
|---|---|
| `rates.join(orders, "region")` | inner equi-join on `region` |
| `rates.join(orders, ["region", "tier"])` | inner join on the composite key |
| `rates.join(orders, "region", "left")` | every `rates` row; `orders` cells `null` where no match |
| `join(rates, orders, "region", "outer")` | full outer join (function form) |
| `rates.join(orders, [], "cross")` | Cartesian product — every `rates` row paired with every `orders` row |
| `rates.join(orders, "missing")` | error — key absent in one side |
| `rates.join(orders, "region", "cross")` | error — `"cross"` takes no keys (pass `[]`) |
| `rates.join(orders, "region", "full")` | error — unknown join type |

## Grouping — groupBy and agg

`groupBy` partitions a table's rows into groups that share the same value(s) in one
or more **key columns**, producing a transient grouped handle. Its sole operation
is `agg`, which collapses each group to a single row, returning a new table with
the key column(s) followed by the named aggregate columns. `groupBy` is a **method
only** and **must** be consumed by `agg`: a bare `t.groupBy(…)` not followed by
`.agg(…)` is a parse error at construction.

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

**Keys.** `groupBy(names…)` takes one or more column names; at least one is
required (`t.groupBy()` → error) and an unknown column is an error. Rows group by
**key-tuple equality** across all key columns.

**Aggregation expressions.** `agg(name, expr, …)` takes alternating string names
and aggregate expressions; an odd argument count, or a non-string in a name
position, is an error. Each `expr` is captured unevaluated and run once per group
with the group's columns in scope **as lists** — a bare column name is the *list*
of that column's cells across the group's rows — and `item` bound to the group's
rows as a sub-table. As in `withColumn`, bare names resolve to columns only; reach
an enclosing binding with `env.`. So the list aggregates apply directly:
`sum(rate)`, `mean(rate)`, `min(rate)`, `max(rate)`, `count(item)` (group size),
and arbitrary expressions like `sum(for r in item return r.rate * r.qty)`.

**Result shape.** The columns are the key column(s) in `groupBy` order, then the
aggregate columns in `agg` order. An aggregate name colliding with a key column or
another aggregate is an error. There is **one row per group**, in **first
appearance order** of each group's key (stable, matching `distinct`).

| Expression | Result |
|---|---|
| `t.groupBy("region").agg("n", count(item))` | `region` + row count per region |
| `t.groupBy("region", "tier").agg("total", sum(rate))` | composite-key grouping; one row per (region, tier) |
| `t.groupBy("region")` (no `.agg`) | parse error — `groupBy` must be consumed by `agg` |
| `t.groupBy()` | error — at least one key column required |
| `t.groupBy("region").agg("region", count(item))` | error — aggregate name collides with the key column |
| `t.groupBy("region").agg("a", sum(rate), "b")` | error — dangling name with no expression |

Tables build on the list and dictionary types, so [Lists](lists.md) and
[Dictionaries](dictionaries.md) are useful companions.

## Tables from Go

Host Go code builds a table with `bl.Table(columns bl.Cols, rows ...bl.Row)` — a
typed `bl.Cols` header of column names and types, then positional `bl.Row` values
— and binds it to a `bl.BlTable` env field for the expression to operate on. See
[Values from Go](values-from-go.md) for the full host-side story.
