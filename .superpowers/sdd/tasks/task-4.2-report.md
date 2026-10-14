# Task 4.2 — aggregate implementation (base HEAD 0facc3c)

Scope: shared concrete/symbolic aggregate typing, generated specializations, checked core helpers, and scalar-mode test migration. No OpenSpec checkbox, README (task 4.3), dependencies, project builds, or full-workspace test suite changed.

## RED → GREEN

- 4.1 language `aggregate_overloads_validate_result_and_element_types` was RED on base (`mode(input)` produced Number instead of List<Number>); now GREEN. 4.1 generated numeric, duration, comparable, unordered-mode tests now pass. The generated numeric 4.1 error array required splitting `Result<Number>` from the new `Result<List<Number>>` mode error.
- Added core tests for all ties/first-occurrence, empty and incompatible checked comparisons, numeric sample/median identities and overflow, duration kind/even median/overflow and invalid duration aggregate. The new mode/comparable/duration helper tests initially failed to compile because helpers were absent; invalid duration aggregate test failed until the guard was added.
- Added generated DateTime mode regression (initially RED on mixed naive/zoned values silently treated as unequal; now checked comparison errors), empty literal mode, and contextually inferred empty iteration sum/mode (initially RED from unspecialized empty-source binding). Added language empty-iteration aggregate cases and symbolic constraint signature checks. All GREEN.
- Numeric mode now returns a sorted List; other modes retain first-occurrence ties. Duration sum/mean/median use checked decimal operations and preserve duration kind; min/max use checked temporal comparison where required. Type validation still rejects mismatched and unsupported kinds.

## Validation

All Cargo invocations: `CARGO_TARGET_DIR=/tmp/blkit-list-expressions-recovery-target CARGO_BUILD_JOBS=2`.

- `cargo test -p blkit-transpiler --lib --test language --test generated`: 39 unit, 173 language, 184 generated passed (final).
- `cargo test -p blkit-core --lib`: 22 passed (final).
- Targeted RED/GREEN logs: `/tmp/blkit-task-4.2-*.log`; baseline RED `/tmp/blkit-task-4.2-red.log`, core missing helper RED `/tmp/blkit-task-4.2-core-red.log`, temporal-mode RED `/tmp/blkit-task-4.2-temporal-mode-red.log`, empty-iteration RED `/tmp/blkit-task-4.2-generated-iteration-red.log`; corresponding GREEN and final logs in same directory.
- `cargo fmt --all -- --check`, `git diff --check`: passed.
- `cargo clippy --all --workspace`: passed; pre-existing warnings remain (core `nonminimal_bool`, unused/dead-code, complex PostgreSQL type; generated dev-harness warnings). No new transpiler warning in final run.
- Full workspace test suite deliberately not run (task 5.2).

Residual risks: independent review pending; full workspace suite pending per task 5.2. Existing unrelated warnings unchanged. No staged files at handoff.

## Independent review fixes (base HEAD 7c1b209)

- Calendar mode P1: `Calendar` already implements `PartialEq` (entry-set/validity equality); a new language + compiled/generated Calendar mode test passed before implementation edits, so no contract or equality change was needed. Also verified generic `mode(List<Value>)` and nested `List<List<Number>>` compile and execute with first-occurrence tie ordering.
- Duration P1: core boundary test RED on `mean([MAX,MAX])` (`Number + failed`); now duration mean/even median divide inputs into exact quotient/remainder before summing, avoiding unrepresentable intermediate sums and preserving half-unit results at opposite boundaries. `sum` still uses checked Number arithmetic, and unsupported duration aggregate rejection stays intact. Generated mean MAX and median opposite-boundary regression pass.
- Mode P2: core oversized-input test RED on comparison error; `modes_checked` now returns a checked `mode list too large` error above 1024 before comparing. The 1024 boundary and generated oversized mode case pass. This intentionally bounds quadratic work like `sort_checked`.
- Validation (`CARGO_TARGET_DIR=/tmp/blkit-list-expressions-recovery-target CARGO_BUILD_JOBS=2` for every Cargo command): `cargo test -p blkit-core --lib` 24 passed; `cargo test -p blkit-transpiler --lib --test language --test generated` 39/173/186 passed; `cargo clippy --all --workspace` passed with pre-existing core and generated dev-harness warnings. `cargo fmt --all` and `git diff --check` passed. No full workspace test suite per instruction.
