# Tasks

## 0. Temporary build fixture during the breaking migration

- [x] 0.1 Freeze the last generated Rust from `examples/graph.bl` as a temporary fixture and have `build.rs` copy it to `OUT_DIR` so Cargo can build while the parser rejects old syntax; verify `cargo test --test language headers_and_domain_members_require_semicolons_outside_quotes` reaches the test runner. Do not ship the frozen fixture.

## 1. Core syntax and runnable path

- [x] 1.1 Add a focused failing parser test for semicolon-terminated namespace/version (including `"1;0"`) and record/enum members; implement structural statement and brace scanning in `src/compiler.rs`, rejecting unterminated statements and legacy headers, and verify the focused test passes. Keep the already-added full peer/flow tests red until 1.2; do not claim their missing-delimiter cases are proven yet.
- [x] 1.2 Make the already-added `tests/language.rs` peer/semicolon/legacy tests fail specifically for the missing peer, decision-node, and flow/bind behavior, then implement the complete minimal parse path for `start_event`, `end_event`, `decision_task` with one `literal_expression`, and `process` with `flow`/`bind` in `src/compiler.rs`, `src/decision.rs`, and `src/graph.rs`. Verify the positive source parses with its actual peer, decision, and graph AST, malformed braces/semicolons fail for those specific reasons, and old `task`, `decision`, `node =`, and `link` forms are rejected.
- [x] 1.3 Add a failing generated execution test for the same simple process with object-keyed start input and one direct end result, then implement typed input/output ports, `flow`/`bind` validation, decision evaluation, and compiled-graph lowering in `src/semantic/` and `src/codegen/` (runtime adapter changes only if a failing test requires them). Verify the test turns green, mistyped/missing bindings are rejected, and the direct compiler emits runnable Rust.
- [x] 1.4 Document the runnable minimal source, semicolon/braces, `flow` vs `bind`, and JSON start/result formats in README and a small `examples/` source; verify the example compiles and executes as documented.

## 2. Decision graph behavior

- [x] 2.1 Add failing tests for qualified vs sole-output shorthand, implicit acyclic/cyclic dependencies, missing ports, and multiple named task inputs/outputs; implement decision graph inference, port validation, and evaluation; verify the targeted `tests/language.rs` and generated decision tests pass.
- [x] 2.2 Add failing decision-table tests for all existing hit policies, rules/defaults, typed single-/multi-column result ports, and unary tests in the braced syntax; implement table parsing, type checks, and evaluation lowering while reusing existing policy logic; verify migrated table tests pass.
- [x] 2.3 Add failing context and business-knowledge tests for braced typed statements and implicit dependencies; implement parsing, type checks, and code generation; verify migrated context/knowledge tests pass.
- [x] 2.4 Update `examples/pricing.bl` and README decision syntax for inferred dependencies, tables, contexts, and output-port shorthand; verify the documented direct compilation of pricing succeeds.

## 3. Remaining process graph behavior

- [x] 3.1 Add failing flow/bind tests for route-available ports, ambiguous bindings, multiple normal end ports, and branch-specific gateway/join routing; implement type/reachability checks and compiled bindings for these cases; verify language and generated execution tests pass.
- [x] 3.2 Add failing tests for named exceptional terminal events, retries/deadlines, waits, and subprocess outcomes using kind-specific braced peer declarations; implement their parsing/semantic lowering without changing established runtime semantics; verify affected `tests/language.rs`, `tests/generated.rs`, and `tests/engine.rs` cases pass.
- [x] 3.3 Add failing tests for bounded pre/post iteration and sequential/parallel multi-instance `decision_task` execution with typed bindings; implement process-level property syntax and graph lowering; verify invalid bounds and generated execution tests pass.
- [x] 3.4 Migrate graph, iteration, and subprocess examples and their README instructions with the corresponding graph-kind tests; restore `build.rs` to transpile the migrated `examples/graph.bl` and remove the temporary generated-Rust fixture from 0.1; verify each example compiles, its documented routes execute, and Cargo builds without a snapshot.

## 4. Project builds and legacy migration

- [x] 4.1 Add failing cross-file tests for peer resolution, forward references, duplicates, out-of-scope peers, no-Cargo decision-only transpilation, and rejection of legacy crate-backed task calls; update `src/project.rs` and verify targeted project tests pass.
- [x] 4.2 Migrate remaining source fixtures, CLI/direct-source tests, and generated integration tests to the new grammar and object-keyed starts; document rejected generic/extension tasks and migration in README; verify `cargo test` passes without re-enabling legacy syntax.

## 5. Integration and review

- [x] 5.1 Run `cargo fmt --check`, `cargo clippy --all --workspace`, `cargo test --all --workspace`, and `openspec validate revise-business-language-declarations --strict`; fix affected regressions and verify each command passes.
- [x] 5.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); have the parent review the three reports and adjudicate every finding, including disagreements, against code and sources, record accepted/rejected findings and reasons, implement accepted fixes, then use the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish; verify the review record and final checks exist.
