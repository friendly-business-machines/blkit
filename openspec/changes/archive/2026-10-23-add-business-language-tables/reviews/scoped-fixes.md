## Review

- **F1 — ADDRESSED.** `crates/blkit-transpiler/src/semantic/types.rs:39` detects inline Table-schema cycles through wrappers; `semantic/mod.rs:124` rejects them before generation. Tests at `tests/language.rs:3696` cover direct/wrapped/mutual cycles and preserve supported recursion.
- **F2 — ADDRESSED.** `crates/blkit-transpiler/src/codegen/mod.rs:1247` recursively lowers Nullable and declared Table cells; constructors use canonical optional construction at `:1485` and `:1506`. Generated regressions at `tests/generated.rs:4748` and `:4779` cover both constructors, nested values, and authored-null rejection.
- **F3 — ADDRESSED**, with new breakage **B1** below. `crates/blkit-transpiler/src/semantic/types.rs:1584-1591` derives scalar/list types from known fields and retains nullability. `semantic/decision.rs:265-266` checks declared-result compatibility. Negative and positive regressions are at `tests/table_rows.rs:151-215`.
- **F4 — ADDRESSED.** `crates/blkit-transpiler/src/codegen/decision.rs:192-242` separates output validation from input-only dynamic-null rejection; `:380` uses the output validator. Recursive named output helpers at `codegen/mod.rs:4053` retain nested declared-table checks. Standalone/project regressions are at `tests/generated.rs:4817` and `:4853`.
- **F5 — ADDRESSED.** `crates/blkit-transpiler/src/semantic/types.rs:17-33` follows Nullable wrappers for by-value cycles while stopping at actual containers. `tests/nullable.rs:59` covers direct/mutual/mixed cycles and supported list recursion.
- **F6 — ADDRESSED.** `crates/blkit-transpiler/src/decision.rs:126`, `:380`, and `:483`, plus `semantic/decision.rs:112`, traverse join arguments and InOrder expressions. Tests at `tests/table_rows.rs:219-305` and `:355` cover forward references, captures, cycles, column-name exclusion, and binding scope.
- **F7 — ADDRESSED.** `crates/blkit-transpiler/src/codegen/mod.rs:1518-1558` checks cumulative inclusive expansion before extending ranges or pushing indices, without temporary expanded ranges. `tests/generated.rs:4878` covers exact-limit acceptance, all-range/mixed overflow, ordering, and negative ranges.
- **F8 — ADDRESSED.** `crates/blkit-transpiler/src/semantic/types.rs:946` deduplicates selection names by first occurrence; bracket/method schema inference uses it. Join-key parsing remains unchanged. Regressions at `tests/table_rows.rs:308-340` check shape, order, chaining, and scalar conversion.
- **F9 — ADDRESSED.** `crates/blkit-transpiler/src/semantic/types.rs:1041` recursively lowers dictionary values without changing keys. Semantic checks at `tests/table_rows.rs:344` and generated execution at `tests/generated.rs:4906` cover nested dictionaries/lists and outer variables.
- **F10 — ADDRESSED.** `crates/blkit-core/src/table.rs:824-941` bounds cumulative rank work, validates source/rank compatibility, and performs stable fallible merging with propagated comparator errors. Regressions at `:1618-1744` cover incompatible ranks/later keys, stability, null ordering, and larger tables.
- **F11 — ADDRESSED.** `crates/blkit-core/src/table.rs:378-381` normalizes top-level `Some(Cell::Null)` before inference/storage. Tests at `:1138` and `:1162` preserve nested nulls and verify join behavior across serialization.

### New breakage in the fix diff

- **Finding B1 — Important/P1: newly concrete conversion inference is not consumed by hintless emission.**
  **Changed location:** `crates/blkit-transpiler/src/semantic/types.rs:1590-1591`.
  **Connected emission:** `src/codegen/mod.rs:336-337`, `:1872-1885`, `:2041-2042`.

  For `Rate = {rate: Number}`, a Number expression such as `data[1, rate].toValue() + 1` now infers its left operand as Number. This removes the previous Value-to-Number cast inserted by binary specialization. However, binary specialization visits operands without an expected hint (`:336-337`), and `toValue` still emits `to_value::<serde_json::Value>()` without that hint (`:1872-1874`). The resulting numeric arithmetic receives JSON Value instead of Decimal.

  The nullable form similarly infers `Number?`, but nullable binary emission requests hintless operand emission (`:2041-2042`), producing Value where the generated `Some`/`None` match requires Option.

  **Evidence:** deterministic source trace; not an executed reproduction. Saved regressions cover direct conversions and semantic coalesce/sum inference, not generated arithmetic composition.

  **Smallest fix:** when conversion emission has no expected hint, consume its known inferred result type rather than defaulting to JSON Value. Add generated compile/execute coverage for non-null and nullable conversion arithmetic, including null propagation.

### Checks verified

- Read the complete supplied owner-only combined diff, adjudication/mappings, all three fix reports, and connected source seams.
- Confirmed saved RED failures and GREEN results. Final saved logs show:
  - 255 generated tests: `task-5.3-emission-generated-all.log:308`.
  - 96 library, 5 nullable, 21 table-row tests: `task-5.3-emission-affected-tests.log:149,160,187`.
  - 231 language tests: `task-5.3-emission-language.log:284`.
  - 27 core table tests: `task-5.3-emission-core-table.log:80`.
- No tests, shell commands, edits, staging, or branch operations performed.

### Out-of-Scope Observations

None reported.

**Merge verdict: BLOCK.** All eleven accepted findings are addressed, but B1 is new Important breakage in F3’s blast radius. Parent retains adjudication and acceptance.

Skipped whole-branch/Ponytail review and suite reruns, as requested. Risk: conversion composition remains broken; final workspace verification remains parent-owned.

**SCOPED_GATE: BLOCKED**