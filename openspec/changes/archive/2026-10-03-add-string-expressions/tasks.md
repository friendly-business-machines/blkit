# Tasks

## 1. Expression syntax and typing

- [x] 1.1 Add `+` precedence, signed numeric literals for negative positions, and string-list `in` without changing range `in`, decision-table unary `matches`, or `==`/`!=`; add parser/validation regression tests in `tests/language.rs` and verify with `cargo test --test language`.
- [x] 1.2 Validate all specified builtin function arities, argument/result types, nested `extract` output, and literal regex/flags in `src/semantic.rs`; add valid/invalid task and decision examples in `tests/language.rs` and verify with `cargo test --test language`.
- [x] 1.3 Document new operator precedence, one-based/negative positions, regex literal backslashes, and function signatures in `README.md`; verify examples in the documentation validate with the language tests.

## 2. String evaluation helpers

- [x] 2.1 Implement shared grapheme-based indexing, slicing, reverse, padding, and checked decimal-to-integer/count conversion using `unicode-segmentation`; add focused runtime tests for combining marks, zero/out-of-range indices, and invalid/overflow counts; verify with `cargo test --lib`.
- [x] 2.2 Implement literal string joining/search/splitting and scalar-to-string conversions, including delimiter ordering, empty fields, and Unicode whitespace; add focused runtime tests and verify with `cargo test --lib`.
- [x] 2.3 Implement regex search, all-match replacement, grouped extraction, `i`/`m`/`s` flags, and invalid-pattern reporting using `regex`; add focused runtime tests for optional captures, replacement groups, and bad runtime flags/patterns; verify with `cargo test --lib`.

## 3. Generated Rust and integration

- [x] 3.1 Generate builtins and operators from `src/codegen.rs` using the shared helpers; propagate fallible expressions through task returns/branches and decision knowledge/context/tables without changing infallible task signatures; add generated-Rust tests in `tests/generated.rs` and verify with `cargo test --test generated`.
- [x] 3.2 Propagate fallible task/graph expressions through named/legacy graph nodes, multi-instance/loop/gateway paths, and runtime Result boundaries without panic or swallowed errors; add integration tests with invalid runtime pattern/position and verify with `cargo test --test generated --test language`.
- [x] 3.3 Confirm standalone transpilation and project builds compile/runs with all new helpers through the existing `blkit` dependency; add a project integration test in `tests/project.rs`, document direct Rust `Result` caller migration in `README.md`, and verify with `cargo test --test project --test generated`.

## 4. Integration verification and review

- [x] 4.1 Run `cargo fmt --check`, `cargo test --test language --test generated --test project`, and `openspec validate add-string-expressions --strict`; fix any failures and rerun these checks to verify the whole change.
- [x] 4.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); have the parent review the three reports and adjudicate every finding (including disagreements) against the code and sources, record which to accept or reject and why, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish.
