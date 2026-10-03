# Tasks

## 1. Reconcile the completed CLI uplift

- [x] 1.1 After `uplift-blkit-cli` is implemented and its specs are synced/archived, inspect its actual parser and main `cli-experience` spec; add a matching delta for the changed CLI invocations if that capability exists, and align this proposal/design as needed. Verify `openspec validate transpile-projects-without-building --strict` passes before implementation.

## 2. Separate project generation from compilation

- [x] 2.1 Add failing `tests/project.rs` checks for generated crate/worker/server Cargo sources without a Cargo build, built-in-only transpilation without Cargo, and continued extension metadata/lockfile errors without compilation. Verify targeted tests fail before changing the operation.
- [x] 2.2 Replace `Project::build()` with `Project::transpile()` using the existing generation path; retain `cargo metadata` for referenced external tasks and `Project::update()` for explicit updates. Update tests that need compiled artifacts to run Cargo themselves and preserve cross-file/runtime/lockfile checks; verify `cargo test --test project` passes.

## 3. Expose transpilation through the CLI

- [x] 3.1 Add failing `tests/cli.rs` checks for `blkit transpile [PROJECT_DIR]`, default current directory, rejection of `blkit build`, generated help/completions, and no compiled artifact; verify `cargo test --test cli` exposes the old behavior.
- [x] 3.2 Replace the post-uplift `build` dispatch with `transpile` and preserve `update`, direct source/output, error context, and terminal feedback. Update `README.md` examples to run Cargo separately and explain local/remote builds, extension Cargo requirements, lockfiles, and local path dependencies; verify `cargo test --test cli` and the documented commands work.

## 4. Integration and review

- [x] 4.1 Run `cargo test --test cli --test project`, `cargo test`, and `openspec validate transpile-projects-without-building --strict`; verify generated crate/worker/server projects still compile when Cargo is explicitly run by a user/test, and the CLI itself never builds them.
- [x] 4.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); have the parent review the three reports and adjudicate every finding (including disagreements) against code and sources, record which to accept or reject and why, implement accepted changes, then run the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish; verify review decisions are recorded and affected checks pass.
