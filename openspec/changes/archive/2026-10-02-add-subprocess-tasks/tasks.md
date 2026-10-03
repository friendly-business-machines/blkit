# Tasks

## 1. Language and Scope Validation

- [x] 1.1 Add failing syntax tests in `tests/graph_syntax.rs` for `subprocess` nodes and `on error|cancel|terminate` links; implement parsing in `src/graph.rs` and verify `cargo test --test graph_syntax` passes.
- [x] 1.2 Add failing graph/type tests in `tests/graph_validation.rs` for normal vs exceptional route availability, one success link, unique handlers, invalid outcome links, unknown/mismatched processes, and direct/indirect recursion; implement semantic validation in `src/semantic.rs` and verify `cargo test --test graph_validation` passes.
- [x] 1.3 Add failing cross-file and cross-scope project tests in `tests/project.rs`; reuse same-scope process merging in project validation and verify `cargo test --test project` passes.

## 2. Generated Graph and Nested Execution

- [x] 2.1 Add a failing generated-process test in `tests/generated.rs` for a typed successful child call and compiled child registration; extend `src/codegen.rs` and named graph definitions, and verify `cargo test --test generated` passes.
- [x] 2.2 Add failing engine tests for a capacity-one nested call, parallel/repeated subprocess activations, all three handled outcomes, unhandled propagation, and child terminal-name retention; implement activation-keyed nested checkpoint scheduling in `src/named_runtime.rs` and `src/runtime.rs`, then verify `cargo test --test engine` passes.
- [x] 2.3 Add failing engine/recovery tests for nested completed-task checkpoints, child waits, retries, child deadline timeout handling, parent retry exhaustion, and parent cancellation/deadline precedence; implement persisted child policy metadata and wake scheduling through the existing checkpoint stores, then verify `cargo test --test engine` passes after local restart.

## 3. Project and Distributed Integration

- [x] 3.1 Add a project fixture calling a process across same-scope `.bl` files in both server and worker targets; ensure parent and child compiled definitions are linked and reject missing child registration; verify `cargo test --test project` passes.
- [x] 3.2 Add distributed worker tests for a handoff during a child wait/retry, shared capacity, and cancellation with active children; use the existing parent claim/generation guarded commit and verify the relevant distributed integration test passes.
- [x] 3.3 Add a minimal subprocess `.bl` example and README usage showing success plus per-outcome routing; verify the documented compile command and generated result with `cargo test --test generated`.

## 4. Integration and Review

- [x] 4.1 Run `cargo fmt --check`, `cargo test`, and `openspec validate add-subprocess-tasks --strict`; verify no unrelated worktree changes are modified and record any remaining known limitations.
- [x] 4.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); have the parent review all three reports and adjudicate every finding, including disagreements, against the code and sources, record which to accept or reject and why, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish.

## Verification and review disposition

- Final verification: `cargo fmt --check`, `git diff --check`, `openspec validate add-subprocess-tasks --strict`, focused engine regressions (49/49), and full serial `cargo test` passed. The first full run timed out during the slow project suite; the longer retry passed, including all 25 project tests and 31 PostgreSQL tests.
- Accepted from all three reviews: running child deadlines were not checked during an in-flight task. A failing regression now covers the timed-out child handler and cancellation hook; the executor races task/permit waits against the earliest nested wake. This is a runtime-contract issue, not a Rust language rule or a requirement to add clock mocks.
- Accepted from Microsoft: public registry constructors checked missing children but not cycles, allowing directly assembled compiled definitions to recurse indefinitely. Both local and distributed registries now share cycle validation. Microsoft's panic guidance is contextual, not the basis for rejecting invalid input.
- Accepted from Google: a failed grandchild bypassed its intermediate parent's retry policy. Nested failures now try each child's policy from innermost to outermost before top-level attempt failure. Google's API-design guidance is contextual; the scoped retry contract and regression test are the evidence.
- Rust-project review did not find an additional Rust language or unsafe-code issue. No review findings were rejected; the sources were checked for applicability rather than treated as normative bug proofs. Ponytail-review of the revised diff found no safe actionable cuts (`Lean already. Ship.`).
- Known limitations: in-flight/uncommitted external effects can repeat after recovery; blocking synchronous tasks must cooperate with their cancellation hook to release a leaf-task permit promptly. These are existing execution constraints, not separate child instances or exactly-once guarantees.
