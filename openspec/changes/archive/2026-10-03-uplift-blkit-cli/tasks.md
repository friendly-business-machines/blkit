# Tasks

## 1. Parse and expose the existing interface

- [x] 1.1 Extend `tests/cli.rs` with failing checks for `build/update` argument handling, legacy source/output pairs, generated help/version, invalid/mixed arguments, and completion output; verify `cargo test --test cli` exposes the missing behavior before implementation.
- [x] 1.2 Add the six requested CLI dependencies and replace `src/main.rs` argument dispatch with `clap` parsing and `clap_complete` behind `--completions SHELL`, preserving exit behavior and existing invocations; verify `cargo test --test cli` passes for parsing/completion cases.
- [x] 1.3 Document the existing command forms, `--help`, `--version`, and `--completions` in `README.md`; verify sample invocations with `cargo run -- --help` and `cargo run -- --completions bash`.

## 2. Render diagnostic and interactive feedback

- [x] 2.1 Extend `tests/cli.rs` with failing assertions for source/project failure context, stderr/nonzero exits, clean redirected output, and `NO_COLOR` behavior; verify the new tests fail against the current presentation.
- [x] 2.2 Adapt CLI errors using `thiserror` and `miette`, with original compiler/Cargo details intact, and add `console` terminal-only styling plus `indicatif` transient status around `build/update`; verify `cargo test --test cli` passes and manually confirm a terminal run clears the status on success and failure.
- [x] 2.3 Document where interactive feedback and diagnostics appear (stderr) and what changes under redirection/`NO_COLOR`; verify the README examples and `cargo test --test cli` match that behavior.

## 3. Integration and review

- [x] 3.1 Run `cargo test --locked --test cli --test project -- --test-threads=1 --skip project_worker_binary_claims_only_its_compiled_process_version` and `openspec validate uplift-blkit-cli --strict`; verify 12 CLI and 22 project tests pass. Record that the user waived the long full suite and that the excluded worker/PostgreSQL test failed connecting to its container in both the focused and isolated runs; do not claim either passed.
- [x] 3.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); have the parent review the three reports and adjudicate every finding (including disagreements) against code and sources, record accept/reject decisions and reasons, implement accepted changes, then run the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish; verify review decisions are recorded and affected checks pass.
