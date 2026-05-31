---
name: BlTable
description: The table (relation) type in the blkit expression language — an ordered list of uniformly-keyed dictionaries. Covers construction as a list-of-dictionaries, row/column access, the table built-ins, and the Go layer (BlTable + expr registrations).
targets:
  - ../../expr/table.go
---

# BlTable — the `table` type

`table` is a DMN **relation**: an ordered, immutable list of `dictionary` rows that all share the same
column keys (the "uniform-keys" invariant). The Go value type backing it is `BlTable`.

A literal would be the syntactic form for writing a constant value of a type directly inside an
expression (as `[1, 2, 3]` is for a list). **`table` has no dedicated form** of its own — a table
is, structurally, a `list` of uniformly-keyed `dictionary`s, so list literals, indexing, filtering,
and projection ([list.spec.md](list.spec.md), [bl-expr.spec.md](bl-expr.spec.md)) all apply. The
`table(...)` built-in wraps a list of dictionaries as a validated `BlTable` — for example, the
`table([{region: "domestic", rate: 5.99}])` in
`rowCount(table([{region: "domestic", rate: 5.99}]))`. See [dictionary.spec.md](dictionary.spec.md) and
[list.spec.md](list.spec.md).

---

## Construction & access

```
// A list of uniformly-keyed dictionaries is the table's data:
[{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}]

// table(list) validates uniform keys and yields a BlTable:
table([{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}])

// Row access (1-based; negative from end; out of range → null):
table([...])[1]                 // → the first row dictionary
// Column access is list projection over the rows:
table([...]).region             // → ["domestic", "europe"]
// Filter is list filtering (item is the row dictionary):
table([...])[item.rate > 10]    // → rows with rate > 10
```

`[@test] ../../expr/table_test.go`

---

## Built-in functions

All blkit extensions (**ext** — no DMN equivalent); relations in DMN are otherwise handled as lists.

| Function | Example | Result |
|---|---|---|
| `table(listOfDictionaries)` | `table([{a:1},{a:2}])` | a validated `BlTable` |
| `count(t)` | `count(table([...]))` | row count |
| `isEmpty(t)` | `isEmpty(table([]))` | `true` |
| `columns(t)` | `columns(table([{a:1,b:2}]))` | `["a", "b"]` (declared/inferred order) |
| `hasColumn(t, name)` | `hasColumn(t, "rate")` | `true` |
| `addRow(t, row)` | `addRow(t, {region:"intl", rate:25.99})` | new table (row keys must match) |
| `removeRow(t, index)` | `removeRow(t, 2)` | new table (1-based) |
| `project(t, names…)` | `project(t, "region", "rate")` | table with only those columns |
| `drop(t, names…)` | `drop(t, "rate")` | table without those columns |
| `rename(t, from, to)` | `rename(t, "rate", "price")` | table with the column renamed |
| `distinct(t)` | `distinct(t)` | duplicate rows removed (full-row equality) |
| `sortBy(t, column[, descending])` | `sortBy(t, "rate", true)` | stable sort by column |
| `asList(t)` | `asList(t)` | the underlying list of dictionary rows |

A table is also a list, so list aggregates/operations apply over its rows (e.g. `count`, filtering,
`for x in t return …`). `sum(t.rate)` sums a projected column.

`[@test] ../../expr/table_functions_test.go`

---

## Operators

| Operator | Meaning | Example |
|---|---|---|
| `t[i]` | row by index | `t[1]` |
| `t.col` / `t[predicate]` | column projection / row filter | `t.region`, `t[item.rate > 10]` |
| `=` `!=` | equality (row-wise, order-sensitive; shape is part of identity) | `t1 = t2` |

`[@test] ../../expr/table_operators_test.go`

---

## Migration mapping (legacy method-chained → string)

| Legacy | New form |
|---|---|
| `Bl.Table(dictionaries…)` / `Bl.Table(Bl.Columns, Bl.Row…)` | `table([{…}, {…}])` (a list of dictionaries) |
| `columnNames` / `rowCount` / `isEmpty` | `columns(t)` / `count(t)` / `isEmpty(t)` |
| `row(i)` / `rows` / `firstRow` / `lastRow` | `t[i]` / `asList(t)` / `t[1]` / `t[-1]` |
| `column(name)` / `hasColumn(name)` | `t.name` (projection) / `hasColumn(t, name)` |
| `addRow` / `addRows` / `removeRow` | `addRow(t, row)` / repeated `addRow` / `removeRow(t, i)` |
| `project` / `drop` / `rename` / `distinct` | `project(t, …)` / `drop(t, …)` / `rename(t, from, to)` / `distinct(t)` |
| `filter` / `sortBy` | `t[predicate]` / `sortBy(t, column[, descending])` |
| `asList` | `asList(t)` |
| `equals` | `=` / `!=` |
| `toRecords` / `String` / `ToMarkdown` | Go host accessors (below) |
| `BlList.asTable` | `table(list)` |

All are reflected; the table-specific structural ops are blkit extensions, the rest reuse list
semantics.

---

## Go implementation (expr extension)

Lives in `expr/table.go`. Shared mechanics in
[bl-expr.spec.md § Engine internals](bl-expr.spec.md#engine-internals-go).

### Value type & host API (exported)

`BlTable` wraps an ordered column list plus a `[]BlDictionary` of rows enforcing the uniform-keys
invariant. Implements `BlValue` (and is list-compatible).

```go
type BlTable struct{ columns []string; rows []BlDictionary }

func (BlTable) Type() BlType { return BlTypeTable }
func (t BlTable) Equal(other BlValue) BlValue // row-wise, order-sensitive; shape is part of identity
func (t BlTable) ToMarkdown() string          // aligned markdown table
func (BlTable) isBlValue() {}

func Table(rows ...BlDictionary) (BlTable, error) // host constructor (validates uniform keys)
func (t BlTable) ToRecords() []map[string]BlValue
func (t BlTable) String() string      // list-of-dictionaries literal

// ToArrow exports the table as an Apache Arrow record batch. The schema is
// derived from the column order; each column's Arrow type is mapped from its
// Bl* element type (BlNumber → Decimal128, BlString → Utf8, BlBoolean →
// Boolean, BlDate → Date32, BlDateTime → Timestamp, durations → Duration,
// nested BlList/BlDictionary → List/Struct). A BlNull cell becomes a null slot.
// Returns an error if a column holds mixed, non-uniform types.
func (t BlTable) ToArrow() (arrow.Record, error)  // github.com/apache/arrow/go/v17/arrow
```

### Registrations (`tableOptions`, unexported)

```go
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
        expr.Function("sortBy",    sortByFn,            new(func(BlTable, BlString) BlTable), new(func(BlTable, BlString, BlBoolean) BlTable)),
        expr.Function("asList",    typed1(asListFn),    new(func(BlTable) BlList)),
    }
}
```

`table(list)` validates that every element is a `BlDictionary` with identical keys → `BlTypeError` on
mismatch. **Reuse.** Row indexing, projection, filtering, and list aggregates are inherited from the
list machinery (`BlTable` embeds/satisfies `BlList`, so it is accepted wherever a `BlList` is). Native
Go `[]map[string]any` inputs wrap to `BlTable` when uniform.

`[@test] ../../expr/table_test.go`

---

## Edge cases

- `table([])` → empty table, no columns; the first `addRow` fixes the column order.
- `addRow` with mismatched keys → `BlTypeError`.
- `project`/`rename` referencing an absent column, or `rename` colliding with an existing one →
  `BlTypeError`.
- `sortBy` on a mixed-type column → `BlTypeError`.
- `t[i]` / `t.col` out of range / unknown column → `null`.
- `distinct` uses order-insensitive row (dictionary) equality.
- equality is `false` when column sets differ, even with identical row data.
