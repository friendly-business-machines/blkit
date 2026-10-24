# Spec Delta

## Purpose

Defines standalone, immutable `.bl` table values and their selection, relational transformation, and aggregation semantics independently of authored decision-table nodes.

## ADDED Requirements

### Requirement: Tables are explicit, immutable, ordered, and column-homogeneous
A table SHALL be an ordered immutable sequence of dictionary rows with a defined column order. All rows SHALL have the same keys; non-null values within each column SHALL share one compatible type. Tables constructed inside `.bl` SHALL infer their column names and non-null cell types from statically determinable headers and values, without requiring column type declarations; unresolved uses that require a specific type SHALL fail during `.bl` transpilation rather than deferring type errors to evaluation. `table(names, rows…)` SHALL accept a statically determinable list of distinct string column names followed by positional value lists of matching width, evaluating cell expressions and rejecting invalid headers, row shapes, or heterogeneous column values. `table(names)` SHALL keep the named columns with zero rows; `table([])` SHALL have zero columns and rows. `tableFromDicts(listOfDictionaries)` SHALL adopt rows only when the dictionary keys and non-null value types are statically determinable from declared row types or literal dictionaries and every row has compatible keys and values; its empty input SHALL yield a zero-column, zero-row table. Untyped dictionaries whose keys or value types are not statically determinable SHALL NOT supply table column references without a declared row shape. A plain list of dictionaries SHALL NOT automatically become a table. Transformations SHALL return new tables without changing their operands, and SHALL support chaining. Table input and output ports SHALL declare a named dictionary row shape as `Table<Row>`; a bare `Table` port SHALL fail transpilation. Declared fields SHALL be present in every row, and the declared field order SHALL be the table's column order. A field declared `T?` SHALL accept either a value of non-null type `T` or null; a plain `T` field SHALL reject null. A `Table<Row>` JSON port SHALL use `{ "columns": ["name", ...], "rows": [{"name": value, ...}, ...] }` to preserve column order and empty schemas; inputs SHALL validate both fields and every declared field type, rejecting malformed data as soon as it arrives and before evaluating expressions. Outputs SHALL use the same shape and match the declared row shape. Table input/output values SHALL preserve table identity at typed boundaries.

#### Scenario: Construct typed values without column declarations
- **WHEN** `.bl` evaluates `table(["region", "rate"], ["domestic", 5.99], ["europe", 15.99])` or `tableFromDicts([{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}])`
- **THEN** each is a two-row `Table` with String and Number columns, and a separate list of those dictionaries remains a list

#### Scenario: Empty shape and invalid rows
- **WHEN** `.bl` evaluates `table(["rate"])` or `table([])`, or tries duplicate headers, a row of wrong width, or mixed String and Number cells in one column
- **THEN** empty tables retain the requested schemas and invalid constructors fail rather than silently changing shape or type

#### Scenario: Declared row shape on table ports
- **WHEN** `Order = {id: Number, price: Number?};` and a task declares `input orders: Table<Order>;` or `output result: Table<Order>;`
- **THEN** both ports have the declared column order and types; a missing `id` or `price`, a null `id`, a wrong cell type, or an unexpected column in submitted JSON fails on input, while a null `price` is allowed
- **WHEN** a table input or output port is declared as bare `Table`, or the program references an undeclared column of `orders`
- **THEN** `.bl` transpilation fails before Rust code is generated

### Requirement: Table selectors have explicit row and column semantics
Every bracket selection on a table SHALL return a table, including one row or one cell. Row selectors SHALL accept 1-based integral indices, negative-from-end indices, inclusive ranges, index lists containing indices and flattened ranges, or Bool row predicates. Lists of row indices SHALL retain input order and the first occurrence of each index; invalid indices, zero, nonintegral positions, and out-of-bounds indices SHALL fail. `t[row, column]` SHALL apply both selectors, allowing omitted rows (`t[, rate]`) to mean all rows or omitted columns (`t[1]`) to mean all columns. A column selector SHALL be an unquoted column reference or a list of such references, preserving listed order and deduplicating first occurrences; unknown columns SHALL fail during `.bl` transpilation when the table's shape is declared or inferred. `t[]`, three or more selectors, numeric column positions, and quoted column selectors such as `t["region"]` SHALL fail. `t.col` SHALL project an identifier-named column to a list of values in row order; `t[1].col` SHALL therefore remain a one-element list. Access to columns with non-identifier names by expression is outside this change.

#### Scenario: Filter, reorder, and explicitly unwrap
- **WHEN** `t` has regions and rates and `.bl` evaluates `t[rate > var(minimum), [region, rate]]`, `t[[3, 1, 3:4], region]`, `t[, rate]`, `t[1, rate].toValue()`, and `t.rate[1]`
- **THEN** the first three values are tables with the requested rows/columns, the index list keeps 3, 1, 4 once each, and the last two values are scalar cells

#### Scenario: Malformed and unsupported selectors
- **WHEN** `.bl` evaluates `t["region"]`, `t[1, 2]`, `t[]`, `t[1, rate, region]`, `t[0]`, or a predicate that is not Bool
- **THEN** validation or evaluation reports an error rather than treating a selector as a dictionary/list index

#### Scenario: Bound table constructors retain statically known columns
- **WHEN** a table constructed with only the `rate` column is assigned to a context or decision binding and that binding is used to select `missing`
- **THEN** `.bl` transpilation rejects the missing column before Rust generation, including through resolvable aliases

### Requirement: Table expressions use scoped column references
Throughout table selectors and the `select`, `rename`, `sort`, `join`, `groupBy`, `withColumn`, and `agg` syntaxes, a column reference SHALL be an unquoted identifier. In bracket, `filter`, `filterOut`, and `withColumn` row expressions, bare identifiers SHALL resolve to the current row's columns; in `agg`, bare column identifiers SHALL resolve to that group's lists of column values. Unknown bare identifiers SHALL fail during `.bl` transpilation instead of falling back to enclosing bindings. `var(name)` SHALL explicitly resolve an enclosing variable (even when a column shares its name); the table operand SHALL still be used directly as `t[...]`. These expressions SHALL NOT bind an implicit whole-row name. Constructor headers and `hasColumn` SHALL continue to accept string column names; mode names and literal cell values SHALL be strings where appropriate.

#### Scenario: Column shadowing and outer variables
- **WHEN** a table has a `rate` column and an enclosing variable also named `rate`, and `.bl` evaluates `t[rate > var(rate)]`
- **THEN** the left `rate` is the current cell and `var(rate)` is the enclosing value; an unknown bare column name fails

### Requirement: Tables expose shape, explicit conversion, and list-style row operations
`t.nRows` and `t.nCols` SHALL return Number counts, and `t.colNames` SHALL return the column names sorted canonically by the existing dictionary-key ordering. `t.toList()` SHALL return ordered cell values for exactly one column and otherwise ordered row dictionaries; nullable cells in typed inputs and join/union-produced missing cells SHALL retain their null values through conversion and row iteration without permitting authored null list literals; `t.toDict()` SHALL require exactly one row and return its dictionary; `t.toValue()` SHALL require exactly one row and one column and return that cell. `Table` equality with `=` or inequality with `!=` SHALL compare column sets and ordered rows structurally, with row dictionary key order irrelevant. Table row iteration (`for`, `some`, `every`), `isEmpty(t)`, projected list operations such as `sum(t.rate)`, and `hasColumn(t, "rate")` SHALL work without unwrapping a table. `hasColumn` SHALL take a string name. Whole-row membership SHALL be expressible as `some r in t satisfies r = row`, with `=` accepting row-dictionary equality only in this table-row context. Ordinary non-table equality SHALL retain `==`/`!=`.

#### Scenario: Counts, conversion, and row traversal
- **WHEN** `t` has two rows and columns `rate`, `region`, and `.bl` evaluates `t.nRows`, `t.colNames`, `sum(t.rate)`, `hasColumn(t, "rate")`, and `for r in t return r.rate`
- **THEN** they yield `2`, `["rate", "region"]`, the numeric sum, `true`, and the row rates in order

#### Scenario: Conversion preconditions and table equality
- **WHEN** a two-row table calls `.toDict()` or `.toValue()`, or two tables have equal column sets and rows in different order
- **THEN** conversion fails and table equality is false; a one-row, one-column selection can convert to a scalar with `.toValue()`

### Requirement: Nullable table cells retain typed, predictable expression semantics
A nullable column SHALL retain its declared or inferred non-null cell type and separately record that its cells may be null. Null from a nullable input field, an outer join, or `union(..., "all")` SHALL propagate through arithmetic and ordinary comparisons without pretending to be a non-null value; negation and `and`/`or` on nullable Booleans SHALL use three-valued logic (for example, `not unknown` is unknown, `false and unknown` is false, and `true or unknown` is true). Table filters SHALL retain only rows with a true predicate, excluding false and unknown rows. `sum` and `mean` SHALL ignore null cells in numeric lists, including projected table columns. `sum` SHALL return zero when no non-null values remain, including an empty numeric list; `mean` SHALL return null when no non-null values remain, including an empty numeric list. `coalesce(value, fallback)` SHALL return `value` if non-null and otherwise a same-type non-null `fallback`, evaluated only when needed. Nullable results SHALL NOT silently satisfy a non-null output type; they SHALL require a nullable declaration or explicit handling such as `coalesce`. Authored `null` and authored `[null]` SHALL remain invalid.

#### Scenario: Null-aware arithmetic, filters, and aggregates
- **WHEN** a table contains a nullable `price` column with rows `20` and null
- **THEN** `sum(t.price)` returns `20`, `mean(t.price)` returns `20`, arithmetic involving the null price produces null, and both `t[price > 10]` and `t[not(price > 10)]` exclude the null row

#### Scenario: All-null and empty numeric columns
- **WHEN** an all-null or empty numeric column is summarized
- **THEN** `sum` returns `0`, `mean` returns null, and `coalesce(mean(t.price), 0)` returns `0`

#### Scenario: Nullable fields and output types
- **WHEN** a nullable field is projected, used in arithmetic, or emitted in a `Table<Row>` output
- **THEN** `.bl` transpilation requires a compatible nullable type or an explicit non-null fallback, and a null cell in a non-nullable output fails before Rust generation if nullability is statically possible

### Requirement: Table row transformations preserve identity and validate output shape
`t.filter(predicate)` SHALL equal `t[predicate]`; `t.filterOut(predicate)` SHALL keep rows for which the predicate is false, equivalent to `t[not(predicate)]`. `t.select(columns…)` SHALL select columns in argument order and fail on unknown names. `t.rename(source -> destination, …)` SHALL accept one or more comma-separated unquoted pairs and reject unknown sources, duplicate outputs, or collisions with unrenamed columns; simultaneous swaps of existing names SHALL be valid if the final schema has unique names. `t.slice(rows)` SHALL have the same row-selector semantics as `t[rows]`. `t.distinct()` SHALL retain the first of duplicate full rows in original order. `t.withColumn(column = expr)` SHALL evaluate once per row in row scope, append a new column or replace one in its original position, and reject incompatible column values. `t.sort(keys…)` SHALL require at least one valid key and stably sort by left-to-right precedence. Bare keys SHALL ascend; `asc(column)` and `desc(column)` SHALL define explicit directions. `inOrder(column, order)` SHALL rank cells by first equal occurrence in the supplied list; unranked values SHALL follow ranked values in ascending order. Unsupported comparisons SHALL report errors rather than use arbitrary ordering.

#### Scenario: Transform and chain without mutation
- **WHEN** `t.select(region, rate).rename(rate -> price).withColumn(with_tax = price * 1.2).sort(region, desc(with_tax))` is evaluated
- **THEN** each step produces a new table, and the result orders its three columns as `region`, `price`, `with_tax` without changing `t`

#### Scenario: Stable sort with explicit ranks
- **WHEN** `t.sort(inOrder(region, ["eu", "eu", "us"]), desc(rate))` is evaluated
- **THEN** eu precedes us, unlisted regions follow in ascending order, rate breaks ties descending, and equal rows under all keys retain their original order

### Requirement: Table union concatenates rows with selectable schema rules
`union(t, others…[, how])` and `t.union(others…[, how])` SHALL have identical table behavior: one operand returns an equal table; multiple table operands concatenate rows in operand order including duplicates. The optional final string mode SHALL default to `"error"` and accept only `"error"`, `"all"`, or `"common"`. `"error"` SHALL reject differing column sets; `"all"` SHALL keep first-table column order followed by newly encountered columns and fill absent cells with join-style null values; `"common"` SHALL keep shared columns in first-table order. With identical column sets, all modes SHALL preserve the first table's column order. Result columns SHALL maintain compatible non-null types; invalid modes or incompatible shared column types SHALL fail. Existing list-valued `union` SHALL retain first-occurrence deduplication.

#### Scenario: UNION ALL and schema modes
- **WHEN** `union(t1, t2)` is evaluated on equal schemas with duplicate rows, or `union(t1, t2, "all")` on disjoint columns
- **THEN** duplicates remain in operand order; `"all"` retains both sets of columns and uses nulls for missing cells, and `.distinct()` can explicitly remove duplicates

### Requirement: Table joins combine matching pairs and retain unmatched rows
`join(t, other, on[, how])` and `t.join(other, on[, how])` SHALL be equivalent. `on` SHALL be one unquoted shared key or a list of such keys, each present on both sides; non-cross joins SHALL require at least one key and match only pairs equal on every key. The optional string mode SHALL default to `"inner"` and accept `"inner"`, `"left"`, `"right"`, `"outer"`, or `"cross"`; `"cross"` SHALL require empty keys (`[]`) and return the Cartesian product. Matches SHALL produce one row per pair. Left, right, and outer joins SHALL retain the appropriate unmatched rows, placing null in missing-side non-key cells while keeping a single key column with the available side's key value. Result columns SHALL be keys in key argument order, then left non-keys, then right non-keys; any duplicate non-key name across inputs SHALL fail unless renamed first. Result rows SHALL have uniform keys and compatible non-null column types. Unknown modes or keys, keyed cross joins, and empty keys on non-cross joins SHALL fail.

#### Scenario: Multi-key duplicates and outer nulls
- **WHEN** two left rows and two right rows share both keys and another left row has no match, and an outer join is evaluated
- **THEN** the result contains four matched pair rows and the unmatched left row with null right-side cells and its original key values

#### Scenario: Cross and overlapping non-keys
- **WHEN** `join(t, other, [], "cross")` has two and three rows respectively, or the inputs both have a non-key `rate` column
- **THEN** the cross join has six rows if names do not collide, and a non-key collision is rejected even for a cross join

### Requirement: Grouped aggregation must consume its grouping
`t.groupBy(columns…).agg(output = expr, …)` SHALL require at least one existing unquoted group key and at least one aggregate assignment. `groupBy` without a following `.agg(…)` SHALL fail validation. Each output SHALL have an unquoted new column name, `=`, and an expression evaluated over the group where each column name denotes that group's ordered list; `var(name)` SHALL resolve outer bindings. Missing or quoted output names, missing `=`, duplicate outputs, and outputs colliding with keys SHALL fail. `count()` SHALL return the Number of rows in the current group; existing list aggregates such as `sum` and `mean` SHALL accept applicable group columns. Result columns SHALL contain keys in argument order followed by outputs in assignment order; result groups SHALL appear in order of first occurrence in the input.

#### Scenario: Grouped sums and counts
- **WHEN** `t.groupBy(region).agg(total = sum(rate), average = mean(rate), n = count())` evaluates rows with repeated regions
- **THEN** it produces one row per region in first-seen order, with columns `region`, `total`, `average`, `n` and each group's correct aggregates

#### Scenario: Grouping errors
- **WHEN** `t.groupBy()`, `t.groupBy(missing)`, `t.groupBy(region)` without `.agg`, or `t.groupBy(region).agg(region = count())` is evaluated
- **THEN** validation fails instead of producing a partial grouping value
