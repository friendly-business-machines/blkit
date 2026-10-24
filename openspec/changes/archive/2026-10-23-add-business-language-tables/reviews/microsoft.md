## Review

Reviewed the supplied whole-feature snapshot, including new tests, through the Microsoft resilience/correctness lens. No edits applied.

### Correct
- `Table` keeps storage private and checks duplicate columns, row widths, and compatible cell types (`crates/blkit-core/src/table.rs:281–285,364–392`).
- Join growth and row-range expansion have checked limits (`table.rs:451–463,573–580`).
- Generated tests cover required nullable fields, three-valued logic, null-aware aggregates, and checked temporal joins.

### Findings

**P1 — Recursive table schemas exhaust the transpiler stack**
**Location:** `crates/blkit-transpiler/src/codegen/graph.rs:17–21`

A declaration such as `Row = {children: Table<Row>};` passes validation: `resolve` accepts the declared row (`semantic/types.rs:4105–4110`), while `record_cycle` follows only direct `Named` fields (`semantic/types.rs:16–23`). Schema generation then repeatedly expands `Table<Row>` into its fields without a recursion guard.

This is reachable even when the record is unused: `codegen/mod.rs:4055–4066` generates table helpers for every record. Transpiling source containing this declaration therefore recurses until stack exhaustion instead of returning a diagnostic.

**Smallest fix:** detect recursive inline table-schema expansion during validation and reject unsupported cycles before generation. Preserve existing supported `List<Record>` recursion. Add a subprocess-isolated regression test.

**Checked rule:** [M-PANIC-IS-STOP / M-PANIC-ON-BUG](https://microsoft.github.io/rust-guidelines/guidelines/correctness/#M-PANIC-IS-STOP), especially the distinction between inherently fallible source parsing and internal programming errors. Microsoft guidance is advisory here, not a Rust language requirement.

**P1 — Accepted nullable constructor cells generate uncompilable Rust**
**Location:** `crates/blkit-transpiler/src/codegen/mod.rs:1243–1297,1478`

For a task with `input price: Number?`, `Row = {price: Number?}`, and expression `table(["price"], [price])`, semantic checking accepts the matching nullable field (`semantic/types.rs:1996–2029`). However, `emit_table_cell_type` has no `Nullable` branch and emits `Cell::from(price.clone())`. `price` is `Option<Number>`, for which `Cell` has no conversion.

Even adding that conversion alone is insufficient: the constructor uses `Table::new`, which rejects null-containing cells (`table.rs:351–354`). Nullable fields inside adopted named dictionaries encounter the same conversion problem.

**Smallest fix:** lower evaluated nullable values explicitly and route nullable constructor rows through the existing optional-cell construction path. Keep authored `null` rejection in semantic validation. Add generated compile-and-execute coverage for both `Some` and `None`.

**Checked contract/rule:** normative table requirements at `openspec/changes/add-business-language-tables/specs/business-language-tables/spec.md:10,60,71–72`; [M-STRONG-TYPES-GUARD](https://microsoft.github.io/rust-guidelines/guidelines/libs/resilience/#M-STRONG-TYPES-GUARD) supplies the advisory invariant/conversion lens.

**P2 — Optional construction permits two observably different null representations**
**Location:** `crates/blkit-core/src/table.rs:378–386`

`Table::from_optional` accepts `Some(Cell::Null)` without normalizing it to `None`. Consequently, constructing a one-row `id` table this way and joining it to itself matches the null keys: `join_keys_equal` excludes only `None`, then compares `Some(Cell::Null)` structurally (`table.rs:258–274`). The equivalent JSON-deserialized table does not match.

This is reachable through the public Rust API; declared nullable-port decoding also preserves this representation (`compiled_graph.rs:246–254,261`).

**Smallest fix:** normalize top-level `Some(Cell::Null)` to `None` in `from_optional`, while retaining nested `Cell::Null` values. Test join behavior and serialization round-trip equivalence.

**Checked rule:** advisory [M-STRONG-TYPES-GUARD](https://microsoft.github.io/rust-guidelines/guidelines/libs/resilience/#M-STRONG-TYPES-GUARD); existing null-key contract is demonstrated by `table.rs:1385–1409`.

### Merge verdict: BLOCK

Inspected construction/decoding, parser scopes, semantic schema propagation, generated nullable operations, relational helpers, and added tests. Primary Microsoft sections were verified from the fetched artifacts and provenance supplied under `review-sources/`.

Skipped execution, suite reruns, and independent digest computation; reproductions above are source-traced, not executed. The supplied 949-test result was not independently revalidated.