What this change does: Adds standalone tables with declared row shapes, nullable cells, selection, transformations, joins, sorting, and grouping. It updates parsing, type checking, generated Rust, JSON boundaries, documentation, and tests.

## Review

Reviewed the supplied whole-branch snapshot, approved OpenSpec artifacts, current connected source/tests, README, adjudication, fix reports, scoped reviews, and recorded RED/GREEN evidence. Findings below are deterministic source traces, not newly executed reproductions.

**Correct:**
- Optional construction normalizes top-level nulls centrally (`crates/blkit-core/src/table.rs:378-381`).
- Sorting now propagates comparison errors through stable merging (`crates/blkit-core/src/table.rs:824-941`).
- Generated selector expansion checks cumulative size before allocation (`crates/blkit-transpiler/src/codegen/mod.rs:1518-1558`).
- Recorded B1 regressions compile and execute successfully (`.superpowers/sdd/tasks/task-5.3-b1-green-final.log:52-57`). This does not cover finding 2’s separate projection path.

**Fixed:** None; read-only review.

### Must fix

1. **P1 — Recursive record cells can exhaust the compiler stack** (`crates/blkit-transpiler/src/codegen/mod.rs:1254-1258,1277-1294`)
   - **What this is:** Named record cells are expanded into generated cell-conversion code.
   - **Problem:** `Node = {children: List<Node>}; Row = {node: Node};` with `table(["node"], [node])` passes validation, then conversion generation repeatedly expands `Node → List<Node> → Node`. Unlike F1, this contains no recursive inline Table schema.
   - **Fix:** Emit a callable converter for each named record so supported recursion traverses runtime values instead of endlessly expanding source. Add an isolated-child regression.
   - **If we skip it:** A valid recursive dictionary used in a table constructor can abort transpilation.

2. **P1 — Hintless column projections still generate the wrong Rust type** (`crates/blkit-transpiler/src/codegen/mod.rs:336-337,389-410,1653-1678`)
   - **What this is:** A projected column should retain its inferred element type.
   - **Problem:** With `Rate = {rate: Number}`, `data.rate[1] + 1` infers Number, but binary specialization supplies no hint and projection emission defaults to `serde_json::Value`. Generated numeric arithmetic therefore receives Value instead of Number; nullable arithmetic has the corresponding Option mismatch.
   - **Fix:** Use the known inferred projection type when no expected hint exists, preserving contextual typing for unresolved empty columns. Add generated arithmetic tests for Number and Number?.
   - **If we skip it:** Valid projection compositions fail when the generated application compiles.

3. **P1 — Tables inside input containers lose temporal semantics** (`crates/blkit-transpiler/src/codegen/decision.rs:207-229,267-278`)
   - **What this is:** Bare Table inputs are decoded into declared cell types; nested tables are only validated.
   - **Problem:** For `Event = {when: DateTime}`, input `List<Table<Event>>`, and `for t in data return t.sort(when)`, JSON timestamps remain String cells. `"2026-01-01T09:30:00Z"` sorts before `"2026-01-01T10:00:00+02:00"` lexically, although it is later in time (`crates/blkit-core/src/table.rs:245-251`).
   - **Fix:** Apply the existing declared-table decoder recursively through supported input containers and named records, not only bare ports.
   - **If we skip it:** Valid generated applications can silently sort or join temporal values incorrectly, and miss incompatible-zone errors.

4. **P1 — Composite numeric cells cannot be extracted** (`crates/blkit-transpiler/src/codegen/mod.rs:1662-1678,1881-1915,1995-2002`)
   - **What this is:** Projection, conversion, and row access deserialize table cells into their declared Rust types.
   - **Problem:** With `Row = {xs: List<Number>}`, valid input `{"columns":["xs"],"rows":[{"xs":[1]}]}` makes `data.xs` decode numeric JSON into `Vec<Number>`. Only scalar Number/Number? receives special handling; Decimal’s enabled `serde-str` decoder requires strings (`crates/blkit-core/Cargo.toml:17`), so this extraction fails.
   - **Fix:** Share type-directed cell extraction that handles numeric leaves recursively through lists, typed dictionaries, nullable values, and named records. Preserve ordinary dynamic JSON numbers.
   - **If we skip it:** Valid nested numeric columns construct and validate successfully but fail during projection, iteration, or explicit conversion.

5. **P1 — Positional selector lists and ranges leak an unavailable row binding** (`crates/blkit-transpiler/src/semantic/types.rs:1265-1308`; `crates/blkit-transpiler/src/codegen/mod.rs:1527-1554`)
   - **What this is:** Numeric positions are evaluated outside the per-row predicate loop.
   - **Problem:** With `Rate = {rate: Number}`, `data[[rate]]` and `data[rate:rate]` pass numeric validation using row-column metadata. Their emitted bounds reference `__bl_row`, which does not exist in positional selection.
   - **Fix:** Reject column references in numeric index-list elements and range bounds; retain explicit outer references through `var(name)`.
   - **If we skip it:** Malformed selectors pass transpilation and produce uncompilable Rust instead of a language diagnostic.

6. **P1 — DictionaryEntry bypasses declared-table input validation** (`crates/blkit-transpiler/src/codegen/decision.rs:206-242`)
   - **What this is:** Boundary validation recursively checks containers holding declared tables.
   - **Problem:** `DictionaryEntry` has no traversal arm. A task accepting `DictionaryEntry<Table<Row>>`, where `Row = {id: Number}`, can accept `{"key":"x","value":{"columns":["id"],"rows":[{"id":null}]}}` and successfully evaluate an unrelated constant.
   - **Fix:** Traverse the entry’s `.value` using its declared inner type, including declared-table validation and decoding.
   - **If we skip it:** Invalid table cells cross a typed input boundary without rejection.

7. **P1 — Structural table equality incorrectly requires matching schema names** (`crates/blkit-transpiler/src/semantic/types.rs:1349-1358`; `crates/blkit-transpiler/src/codegen/mod.rs:234`)
   - **What this is:** Approved table equality compares column sets and ordered row values.
   - **Problem:** Declare `A = {x: Number, y: String}` and `B = {y: String, x: Number}`, then compare inputs `left: Table<A>` and `right: Table<B>` using `left = right`. Both operands are tables, but their different declared type names cause rejection; `!=` also lacks general typed-table dispatch.
   - **Fix:** Recognize both operands as tables independently of their row-type names and use the existing structural comparison for both operators.
   - **If we skip it:** Valid comparisons across reordered or separately named row shapes cannot be expressed.

8. **P1 — Valid dynamic positional rows are limited to one column** (`crates/blkit-transpiler/src/semantic/types.rs:2057-2069`)
   - **What this is:** Constructors already accept a typed list as a positional row.
   - **Problem:** For `Pair = {a: Number, b: Number}`, input `values: List<Number>`, and output `Table<Pair>`, `table(["a","b"], values)` is always rejected—even when `values` contains exactly two numbers.
   - **Fix:** Infer each header’s type from the list element type and retain the existing runtime width check.
   - **If we skip it:** Supported matching-width positional lists work for one column but fail for multiple columns.

9. **P1 — Typed dictionary adoption requires the output record’s name** (`crates/blkit-transpiler/src/semantic/types.rs:2024-2027,2093-2097`)
   - **What this is:** `tableFromDicts` should adopt compatible typed dictionaries in canonical key order.
   - **Problem:** With `Input = {z: Number, a: String}` and `Canonical = {a: String, z: Number}`, adopting `List<Input>` into `Table<Canonical>` passes the column-order check but fails because the input is not literally `List<Canonical>`.
   - **Fix:** Compare canonical field names and compatible field types rather than requiring the enclosing record name to match.
   - **If we skip it:** Valid typed dictionary rows with noncanonical declaration order cannot be adopted into the corresponding canonical table output.

10. **P1 — filter accepts a numeric position instead of a predicate** (`crates/blkit-transpiler/src/semantic/types.rs:1360-1385`)
    - **What this is:** `filter` retains rows whose Boolean predicate is true.
    - **Problem:** `data.filter(1)` is translated directly into bracket selection and accepted as row index 1. It silently returns the first row instead of rejecting a non-Boolean predicate; `filterOut(1)` correctly fails through negation.
    - **Fix:** Require Bool or Bool? for filter predicates before sharing selector handling; retain numeric selectors for brackets and slice.
    - **If we skip it:** An invalid filter silently performs a different operation.

### Verification and scope

- The saved pre-fix workspace summary records **949 passed**; it is not verification of the latest fixes.
- Saved post-fix logs confirm the focused successes, including B1’s four generated fixtures and corrected F4/F7 regressions.
- Assumed load: the README’s single-node application model, including configurable concurrent tasks. Checked relational/range resource guards; no benchmark or distributed redesign was proposed.
- Add focused semantic/generated regressions for the cases above, then perform the parent-owned fresh workspace run with the required timeout.

**Merge verdict: BLOCK.**
**Verdict: fix 1–10 first.**

Not checked: No commands, new reproductions, benchmarks, or suite reruns; snapshot SHA256 was supplied, not independently recomputed. Residual risk: these additional source-proven cases remain untested, and fresh whole-workspace verification is pending.