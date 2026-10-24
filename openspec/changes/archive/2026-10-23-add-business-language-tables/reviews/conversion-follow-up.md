## Review

- **B1 — ADDRESSED.** `crates/blkit-transpiler/src/codegen/mod.rs:1766-1775` supplies existing semantic inference when `toValue`/`toList` has no expected hint and exactly one known column. Consequently, arithmetic receives Number or Option<Number>, and indexed one-column lists agree with inference. Explicit hints retain precedence; unknown-shape conversions remain unchanged.
- **Correct:** existing numeric/null decoding is reused at `codegen/mod.rs:1882-1915`. Generated regressions at `tests/generated.rs:4918`, `:4934`, `:4953`, and `:4983` cover scalar arithmetic, Some/None propagation, indexed lists, coalesce, aggregates, cardinality errors, and dynamic fallback.

### New breakage in the fix diff

No issues found.

### Recorded validation checked

- Corrected RED: three expected generated-compilation failures and one passing dynamic control (`task-5.3-b1-red-corrected.log:119-155,238-250,321-367`).
- GREEN: all four new fixtures pass, including final rerun (`task-5.3-b1-green-final.log:52-57`).
- **158 selected tests confirmed:** 27 generated table + 9 generated nullable + 96 library + 5 nullable + 21 table-row tests (`task-5.3-b1-generated-table.log:79`, `task-5.3-b1-generated-nullable.log:61`, `task-5.3-b1-affected-tests.log:149,160,187`).
- `tests/generated.rs:16-39` confirms the fixture helper compiles and executes generated Rust.

### Out-of-Scope Observations

None.

**Fix round:** All findings addressed, no new Critical/Important breakage.
**Merge verdict: OK** for this B1-only patch; parent retains acceptance.

Skipped F1–F11 re-review, suite reruns, shell commands, and repository mutations. Residual risk: final whole-branch/workspace verification remains parent-owned.

**SCOPED_GATE: PASS**