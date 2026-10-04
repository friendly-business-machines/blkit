# Tasks

## 1. Retire unreachable process-graph compilation

- [x] 1.1 Confirm `compiler::parse` rejects process-level `run`/`return` and existing tests cover current named-graph validation (`cargo test --test language --test graph_validation --test graph_syntax`); add one focused rejection test only if missing and verify it passes first.
- [x] 1.2 Remove `GraphStmt`/branch/`graph::parse`, always-empty `Process.graph`, `semantic::check_graph`, and legacy `codegen` emitter/`has_legacy` branches; keep named graphs and generated `named_graph_definitions()`. Verify the same language/graph tests and `cargo test --test generated --test project` pass; check `cargo fmt --check`.

## 2. Retire the manually built step runtime

- [x] 2.1 Audit step-only tests in `tests/engine.rs`, `tests/http.rs`, and `tests/concurrency.rs` for unique supported cancellation/concurrency coverage; retain or add minimal named-graph equivalents where needed and run those tests before deleting the old cases.
- [x] 2.2 Remove `Definition`/`Step`/`Branch`, standalone evaluation and `execute`, the step registry map and `Engine::start` branch, and their obsolete tests; make `Registry::new`/`get` accept/return compiled graphs in place of `new_named`/`get_named`. Keep callbacks/types needed by named execution. Verify `cargo test --test engine --test concurrency --test http` and `cargo fmt --check`.

## 3. Remove old checkpoint migration and clarify public names

- [x] 3.1 Replace version-zero migration tests with an explicit failure check for old checkpoint execution/resume; retain checks that current checkpoint state round-trips and resumes from committed work. Verify relevant `cargo test --test engine --test store --test postgres` cases (report PostgreSQL environment blockers).
- [x] 3.2 Remove `migrate_checkpoint`, version-zero fallback, and conversion-only fields; guard every execution/resume entry point against unsupported checkpoint versions without destroying stored data. Verify targeted current/old checkpoint tests and `cargo fmt --check`.
- [x] 3.3 Rename `named_runtime` to `compiled_graph` and `runtime::Store` to `runtime::LocalStore` across Rust, generated path strings, project templates, README examples, binaries, and tests, without compatibility aliases; retain `named_graph_definitions()` to avoid a user-task name collision. Verify `rg 'named_runtime|runtime::Store|Registry::new_named|get_named|runtime::Definition' src tests README.md` finds no stale references and run `cargo test --test generated --test project --test cli`.

## 4. Restructure remaining private hotspots

- [x] 4.1 Group decision, named graph/scope, and expression/type validation in private `src/semantic/` children, retaining `validate` and `named_scopes` use; verify `cargo test --test language --test graph_validation --test graph_syntax` and `cargo fmt --check`.
- [x] 4.2 Extract decision and named-graph emission into private `src/codegen/` children; move embedded helper source only if a verbatim extraction clearly helps. Compare representative pre/post emitted Rust allowing only planned module-path substitutions, and run `cargo test --test generated --test project` plus `cargo fmt --check`.
- [x] 4.3 Move compiled-graph scheduling, subprocess, and claimed execution into private `src/runtime/` child code while preserving `runtime::Engine` and worker hook; verify `cargo test --test engine --test concurrency --test http --test postgres` (report PostgreSQL blockers) plus `cargo fmt --check`.

## 5. Integration and review

- [x] 5.1 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets`; build representative generated project consumers and confirm no step-graph symbols or old checkpoint conversion remain. Report blocked external-service checks separately and verify current-version checkpoint results are unchanged.
- [x] 5.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); have the parent review the three reports and adjudicate every finding (including disagreements) against the code and sources, record which to accept or reject and why, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish; verify the review record and final affected-test results exist.
