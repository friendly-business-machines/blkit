# Proposal

## Why

`.bl` has lists and dictionaries but no standalone table value for selecting, joining, grouping, and transforming rows in business expressions. `tables-requirements.md` specifies the missing table behavior. Tables constructed inside `.bl` infer their columns and non-null cell types; input and output ports declare a named row shape so table references can be checked during `.bl` transpilation.

## What Changes

- Add immutable, ordered table values constructed with `table(names, rows…)` or `tableFromDicts(rows)`, with checked column shape and inferred homogeneous non-null column types; plain lists of dictionaries stay lists. Typed table ports use a named row shape such as `Table<Order>`, whose fields may individually permit null.
- Add table-specific bracket selection, row/group expression scopes, projections, shape inspection, conversions, equality, and interoperability with existing list operations.
- Add table methods for filtering, selecting, renaming, slicing, deduplicating, adding/replacing columns, sorting, union, joins, and grouped aggregation; global `union` and `join` forms where specified.
- Allow `=` only for table equality and row membership in table iteration (plus table column assignments), while retaining existing non-table equality rules. Permit join/union-generated null cells and nulls in explicitly nullable input fields, including their projections; define null propagation, null-aware filters and aggregates, and `coalesce(value, fallback)` without permitting authored `null` list literals.

## Capabilities

### New Capabilities
- `business-language-tables`: Standalone table values, column and row selection, conversions, transformations, union, joins, and aggregation.

### Modified Capabilities
- `business-language`: Add a table value type and table-specific overloads/exceptions to existing equality, list iteration/inspection/union, and null/type rules without changing ordinary dictionary, list, or decision-table behavior.

## Impact

- `.bl` expression parsing, type checking and dependency analysis, Rust emission, and runtime value/collection helpers in `crates/blkit-transpiler` and `crates/blkit-core`.
- Language and generated-code tests, README examples, named dictionary fields that permit null, and typed decision-task/process ports carrying tables; bare `Table` ports are replaced by declared `Table<Row>` shapes, and previously completed port/documentation work must be updated. Keep existing `decision_table` nodes in `decision-models` unchanged.
- No database schema or new dependency is proposed.
