## Review

### Correct
- `Table` keeps its representation private and exposes validated constructors and borrowed accessors (`crates/blkit-core/src/table.rs:275–280,350–394,1103–1111`).
- Closed `Cell`/`CellType` enums and the aggregation callback fit the actual callers without unnecessary trait abstractions.
- Added tests cover declared schemas, temporal/enum identity, nullable aggregates, and three-valued Boolean behavior.

### Findings

**1. P1 — Nullable constructor cells cannot cross the generated Rust boundary.**
**Location:** `crates/blkit-transpiler/src/codegen/mod.rs:1245–1295,1463–1478`.

With `R = {price: Number?}`, an input `price: Number?` and expression `table(["price"], [price])` pass semantic checking (`semantic/types.rs:1988–2025`). Emission nevertheless falls through to `Cell::from(Option<Number>)`; no such conversion exists. `tableFromDicts` on nullable named records reaches the same helper. Separately, declared `Table<Row>` cells fall through here because only **named** `Table` is handled.

**Impact:** Supported typed values produce uncompilable generated applications. Simply adding nullable `From` conversions is insufficient: `Table::new` rejects null-containing cells (`blkit-core/src/table.rs:350–353`).

**Smallest fix:** Explicitly lower nullable cells and `Generic("Table", …)` in this shared bridge, using nullable-aware table construction for validated nullable sources. Add generated compilation tests for both constructors with nullable records and a declared nested table.

**Checked rule:** [Leveraging the Type System](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system.html), advisory domain-invariant guidance; constructor/nullability requirements are normative in the approved table specification.

---

**2. P1 — Explicit conversions discard statically known cell types and nullability.**
**Location:** `crates/blkit-transpiler/src/semantic/types.rs:1530–1575`.

`toValue()` returns the requested output type without comparing it with the known column type. One-column `toList()` likewise accepts an arbitrary expected list element type.

For `R = {price: Number?}`, both:
- `data[1, price].toValue()` declared as `Number`;
- `data[, price].toList()` declared as `List<Number>`

are accepted. Null cells then fail runtime casts instead of being rejected during `.bl` transpilation. Incorrect non-null types are similarly accepted.

**Impact:** Conversions bypass the feature’s nullable-to-non-null guarantee.

**Smallest fix:** Derive conversion results from `table_fields_known`, preserving underlying types and nullability; check the expected result against those types rather than adopting it. Keep row-count failures runtime-checked.

**Checked rule:** [Leveraging the Type System](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system.html), advisory; “Nullable table cells retain typed, predictable expression semantics” supplies the normative contradiction.

---

**3. P1 — Output validation rejects permitted null-preserving dictionary conversions.**
**Location:** `crates/blkit-transpiler/src/codegen/decision.rs:358–360`, through `:212–221`.

A valid two-column nullable table converted with `data.toList()` to `List<Dictionary>`, or `data[1].toDict()` to `Dictionary`, preserves nulls correctly in the runtime helper. The new output validation then invokes `dictionary::reject_null`, which recursively rejects those cells (`crates/blkit-core/src/dictionary.rs:4–14`).

**Impact:** Valid table conversions return `"null is not a dictionary value"` whenever a converted row contains null. Existing generated conversion tests cover non-null dictionaries, not this path.

**Smallest fix:** Separate input-only blanket null rejection from output schema validation. Preserve `Table<Row>` output checks without rejecting nulls produced by checked table conversions. Add nullable dictionary/list conversion execution tests.

**Checked rule:** [Leveraging the Type System](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system.html), advisory; the approved conversion requirement explicitly requires null preservation.

---

**4. P1 — Recursive table row schemas cause unbounded schema generation.**
**Location:** `crates/blkit-transpiler/src/codegen/graph.rs:8–23`.

`Tree = {children: Table<Tree>};` passes `resolve` and the existing record-cycle check. `request_type(Table<Tree>)` then recursively expands the same row fields forever. This is reachable even when the record is unused because validators are generated for every record (`codegen/mod.rs:4055–4066`).

**Impact:** Authored input can exhaust the transpiler stack instead of producing generated code or a diagnostic.

**Smallest fix:** Detect cyclic inline table-schema expansion before emission and return a diagnostic. Supporting recursive schema references would be a larger alternative, not necessary for a safe rejection.

**Checked rule:** [Leveraging the Type System](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system.html), advisory invariant modeling; evidence is the recursive source call chain, not a course prohibition on recursive types.

---

**5. P1 — Nullable wrappers bypass by-value record-cycle rejection.**
**Location:** `crates/blkit-transpiler/src/semantic/types.rs:4112–4118`; connected checker at `:16–24`.

The newly accepted `Node = {next: Node?};` passes validation because `record_cycle` follows only direct `Type::Named` fields. Nullable lowering produces `Option<Node>` (`codegen/mod.rs:3934`), so generation emits an infinitely recursive by-value Rust struct.

**Impact:** An invalid record representation is accepted by `.bl` validation and fails only when compiling generated Rust.

**Smallest fix:** Follow transparent nullable wrappers in the existing by-value cycle check, while continuing to stop at genuinely indirect containers. Add a rejection test for direct and mutual nullable cycles.

**Checked rule:** [Leveraging the Type System](https://google.github.io/comprehensive-rust/idiomatic/leveraging-the-type-system.html), advisory; the existing `"recursive by-value record"` diagnostic establishes the project’s intended constraint.

### Merge verdict: BLOCK

Inspected the whole-feature Rust snapshot, connected semantic/codegen/runtime callers, new tests, README, and approved specifications. Findings are static source traces, not executed repros.

Primary Google type-system, API-design, and polymorphism pages were checked through the supplied HTTPS retrieval artifacts and provenance. Course guidance is advisory throughout.

Not checked: execution, independent snapshot hashing, or the reported 949-test result. No edits, staging, or suite reruns were performed. Residual risk: the identified edge cases need targeted executable regression tests.