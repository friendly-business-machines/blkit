# Tasks

## 1. Dictionary literals

- [x] 1.1 Add failing parser-shape and validation tests for ordered dictionary entries, dependent keys, duplicates, and nested dictionaries; verify with `cargo test -p blkit-transpiler --test language`.
- [x] 1.2 Introduce a dictionary-literal `Expr` form in `expr.rs` and update semantic inference/constraints, `decision.rs` and graph walkers, and codegen to retain existing semantics; verify the parser tests and `cargo test -p blkit-transpiler --test generated` pass.
- [x] 1.3 Document the public `Expr` representation change next to its definition, and verify the new node is returned by both `expr::expression` and `parse` in tests.

## 2. Bracket indexing and colon sequences

- [x] 2.1 Add failing parser-shape tests for bracket indexing/filtering and `start:end` with nested operands; verify with `cargo test -p blkit-transpiler --test language`.
- [x] 2.2 Replace parser-generated `__bl_index` and `__bl_sequence` calls with source nodes, update every semantic/graph/decision/codegen consumer, and verify the language and generated-code tests, including dynamic index errors and numeric sequence bounds, pass.

## 3. `some` and named call options

- [x] 3.1 Add failing parser-shape tests for `some` with multiple bindings and ordered named arguments; include rejection tests for duplicate, unknown, and misplaced named arguments; verify with `cargo test -p blkit-transpiler --test language`.
- [x] 3.2 Replace parser-generated `__bl_some` and `__bl_named_*` calls with source-language structure; update inference, constraints, decision/graph walkers, and fallible codegen; verify language and generated-code tests cover short-circuiting, binding scope, and calendar option evaluation.

## 4. Integration verification

- [x] 4.1 Verify direct-source and multi-file project diagnostics and generated graph/decision behavior remain unchanged with `cargo test -p blkit-transpiler` and `cargo test -p blkit-cli --test project`.
- [x] 4.2 Run `cargo fmt`, `cargo clippy --all --workspace`, and `cargo test --all --workspace -- --test-threads=1` (allow at least 5,400 seconds for the full suite); verify they pass and inspect the diff for remaining parser-generated `__bl_dictionary`, `__bl_index`, `__bl_sequence`, `__bl_some`, or `__bl_named_*` encodings.

## 5. Review

- [x] 5.1 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (reviewers must not see each other's findings); have the parent adjudicate every finding, including disagreements, against code and sources, record accept/reject decisions and reasons, implement accepted changes, then run the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Verify all stages are recorded and keep this task unchecked until they finish.
