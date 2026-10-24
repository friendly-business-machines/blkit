## Review

Reviewed the supplied whole-feature snapshot and connected source. Findings below are source-derived; no tests were executed.

### Correct
- Table construction validates headers, row widths, and homogeneous cell types; deserialization also validates the envelope (`crates/blkit-core/src/table.rs:301–396`).
- Join growth uses checked arithmetic and explicit errors; temporal comparison failures propagate (`crates/blkit-core/src/table.rs:565–650`).
- Generated tests cover declared table boundaries, nullable aggregates, three-valued Boolean logic, and lazy `coalesce` (`crates/blkit-transpiler/tests/generated.rs:4303–4360, 4620–4642`).

### Findings

**1. P1 — Conversions bypass statically known cell types and nullability**
**Location:** `crates/blkit-transpiler/src/semantic/types.rs:1530–1567`

`toValue()` returns the expected type without checking the selected column; single-column `toList()` similarly accepts the expected list element type. Consequently, a nullable numeric column can satisfy `Number` through `data[1, price].toValue()`, or `List<Number>` through `.toList()`. Null then causes an evaluation error rather than the required transpilation rejection.

**Smallest fix:** Infer conversion types from known fields, retain nullability, and check compatibility with the expected type. Add negative conversion tests.

**Checked contract/rule:** Table spec:60, 71–72 (**normative**); `api-parse-dont-validate` (**advisory**).

**2. P1 — Output validation rejects legitimate null-bearing table conversions**
**Location:** `crates/blkit-transpiler/src/codegen/decision.rs:360`

The new output check reuses `validate_input_nulls`, which calls `reject_null` for dictionaries (`:221`). Returning `data.toList()` as `List<Dictionary>` from a multi-column table containing a nullable cell therefore fails. Returning its row through `toDict()` fails likewise. `reject_null` recursively rejects these values (`crates/blkit-core/src/dictionary.rs:4–14`).

**Smallest fix:** Separate output validation from authored-input null rejection, preserving legitimate table-produced nulls. Test both conversions with null cells.

**Checked contract/rule:** Table spec:49 (**normative**); `type-option-nullable` (**advisory**).

**3. P1 — Nullable and declared-table constructor cells generate invalid Rust**
**Location:** `crates/blkit-transpiler/src/codegen/mod.rs:1245–1295`

`emit_table_cell_type` handles neither `Nullable<T>` nor generic `Table<Row>`; both fall through to `Cell::from(...)`. Valid construction such as `table(["price"], [value])` with `value: Number?` emits an unavailable `From<Option<Number>>` conversion. A typed table used as a cell similarly needs an unavailable `From<Table>` conversion. Nullable fields inside named dictionary cells hit the same omission.

**Smallest fix:** Handle both generic variants explicitly and use null-preserving construction rather than the null-rejecting `Table::new` path (`:1478`).

**Checked contract/rule:** Table spec:10, 60; design decision 1 (**normative**); `pat-exhaustive-enum` (**advisory**).

**4. P1 — Dependency walkers omit global joins and ranked-sort expressions**
**Location:** `crates/blkit-transpiler/src/decision.rs:120–154`

`infer_dependencies` never visits `TableJoin` or `TableSortKey::InOrder`. Neither does `expr_names` (`:423–499`). A global `join(leftNode, rightNode, id)` cannot acquire dependencies on decision-node results; `sort(inOrder(region, var(orderNode)))` misses its ordering-node dependency. Semantic checking exposes only linked node results (`semantic/decision.rs:220–226`), so these valid compositions are rejected.

**Smallest fix:** Traverse join operands and the ranking expression in dependency/name/call walkers. Add node-result and forward-reference regressions.

**Checked contract/rule:** Design decision 4 (**normative**); `pat-exhaustive-enum` (**advisory**).

**5. P1 — Recursive table schemas cause unbounded recursion during transpilation**
**Location:** `crates/blkit-transpiler/src/codegen/graph.rs:8–23`

`request_type` recursively inlines every `Table<Row>` field without cycle detection. The accepted declaration `Row = {children: Table<Row>};` repeatedly expands itself. Table-schema helpers are generated for every record (`codegen/mod.rs:4055–4064`), making this reachable even without a graph.

**Smallest fix:** Detect recursive inline table schemas and return a transpilation diagnostic before emission; do not recurse until stack exhaustion.

**Checked contract/rule:** `err-result-over-panic`, `api-parse-dont-validate` (**advisory**); verified against `resolve` and `record_cycle`.

**6. P1 — Flattened selector ranges bypass the growth bound**
**Location:** `crates/blkit-transpiler/src/codegen/mod.rs:1526–1547`

Each range is individually bounded, but generated list selectors repeatedly call `__bl_indices.extend(...)` without a cumulative limit. Repeating a million-row range 128 times materializes 128 million indices before `Table::select` deduplicates them. A compact selector can therefore allocate roughly 1 GiB of indices despite bounded final output.

**Smallest fix:** Bound cumulative expansion before extending, or expand into a bounded first-occurrence accumulator. Test repeated ranges.

**Checked contract/rule:** Design decision 3 and resource-risk mitigation (**normative**); `num-overflow-explicit` (**advisory**).

**7. P1 — Nullable wrappers bypass existing by-value record-cycle rejection**
**Location:** `crates/blkit-transpiler/src/semantic/types.rs:4111–4116`

New nullable support accepts `Node = {next: Node?};`, but `record_cycle` examines only direct `Type::Named` fields (`:16–25`). Generated Rust contains `next: Option<Node>` (`codegen/mod.rs:3934`), an infinitely sized recursive type.

**Smallest fix:** Traverse nullable wrappers when checking by-value cycles, while retaining collection indirection as a cycle break. Add direct and mutual nullable-cycle rejection tests.

**Checked contract/rule:** Existing record-cycle validation contract; `api-parse-dont-validate`, `type-option-nullable` (**advisory**).

**8. P2 — Static column selection does not deduplicate columns**
**Location:** `crates/blkit-transpiler/src/semantic/types.rs:895–923`

`data[, [rate, rate]]` produces two static fields, whereas runtime selection deduplicates them (`blkit-core/src/table.rs:475–484`). Declaring the correct one-column output therefore causes a false schema mismatch.

**Smallest fix:** Normalize selected columns by first occurrence before schema inference, including method selections.

**Checked contract/rule:** Table spec:27 (**normative**).

**9. P2 — Dictionary-valued row expressions are parsed but not lowered**
**Location:** `crates/blkit-transpiler/src/semantic/types.rs:975–1007`

The parser scopes dictionary values to columns, but `table_row_expr` does not recurse through `Expr::Dictionary`. Thus `data.withColumn(payload = {amount: rate})` retains an unsupported `Column(rate)` during inference and is rejected.

**Smallest fix:** Add dictionary traversal matching `table_group_expr`, plus a generated-code regression.

**Checked contract/rule:** Table spec:42, 75 (**normative**); `pat-exhaustive-enum` (**advisory**).

### Merge verdict: BLOCK

Inspected constructors, typed boundaries, parser/scopes, schema inference, dependency walkers, conversions, relational helpers, nullable emission, and added tests.

Skipped execution, external-source retrieval, and independent SHA256 computation; the reported 949 passing tests are inherited evidence, not independently verified. No external authority is claimed for the local advisory rules.