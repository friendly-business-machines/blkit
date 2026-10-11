# Tasks

## 1. Separate the distributable CLI and transpiler

- [x] 1.1 Establish a failing CLI-isolation check that detects whether the selected CLI package's build script compiles the example graph; verify it fails for today's root `cargo build --bin blkit` (which runs `build.rs`) before migrating, and will pass for a CLI-only build after separation.
- [x] 1.2 Make the root manifest workspace-only with CLI default member; move the compiler/library source into private `crates/blkit-transpiler` and the `blkit` entry point into private `crates/blkit-cli`, preserving the `blkit_transpiler` import and CLI binary name. Restrict their production dependencies to CLI/compiler needs, with no demo build script; verify `cargo build -p blkit-cli --bin blkit` and the isolation check from 1.1 pass.
- [x] 1.3 Move CLI-invocation tests into `crates/blkit-cli/tests`, adjust their project/example paths and test-only dependencies, and update README CLI build/run commands; verify `cargo test -p blkit-cli --test cli` and the documented CLI command run successfully.

## 2. Isolate the development harness

- [x] 2.1 Move the example graph, build script, and `blkit-dev`, `blkit-api`, `blkit-worker` entry points into private `tools/dev-harness`; compile the fixture through the transpiler build dependency instead of `#[path]` imports. Verify `cargo build -p blkit-dev-harness --bins` builds all three and a CLI-only build does not run the harness build script.
- [x] 2.2 Move process-launching PostgreSQL and cross-component HTTP tests into the harness with their binary environment variables, runtime features, and fixtures; update README demo references and runnable `cargo run -p blkit-dev-harness --bin ...` commands. Verify the harness HTTP and PostgreSQL tests and run the documented demo command(s).

## 3. Restore compiler, runtime, and generated-project coverage

- [x] 3.1 Move compiler/validation/codegen tests into `crates/blkit-transpiler/tests` and runtime/temporal/store tests into `crates/blkit-core/tests`; update shared example references and test-only dependencies. Verify `cargo test -p blkit-transpiler` and `cargo test -p blkit-core --features worker,local-persistence,logging -- --test-threads=1` cover the moved suites (the core's default features intentionally omit the feature-gated suites).
- [x] 3.2 Update transpiler project generation's checkout-relative `blkit-core` path and all affected test-generated manifest paths; move CLI-invoking project tests beside the CLI (or split their library-only cases into transpiler tests). Verify checkout-generated projects still pin a matching core version, have a valid local core path, and direct-source consumers depend on core alone by running the relevant project/generated tests.
- [x] 3.3 Verify a CLI build from the workspace default and `cargo build -p blkit-cli --release --bin blkit` produce a working `blkit` without demo artifacts; verify source transpilation, project transpilation, update, help, completions, and an installed/binary-only CLI invocation preserve behavior. Document the distribution build and verify README instructions match the new workspace layout.

## 4. Integration verification and review

- [x] 4.1 Run `cargo fmt --check`, `cargo clippy --all --workspace`, `openspec validate separate-cli-transpiler-and-dev-harness --strict`, and the full suite `cargo test --all --workspace -- --test-threads=1` with a command timeout of at least 5,400 seconds; verify all selected packages and previously covered integration cases pass, and investigate any failures before claiming completion.
- [x] 4.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (reviewers must not see each other's findings); have the parent review the four reports, adjudicate every finding including disagreements against the code and sources, record accept/reject decisions and why, implement accepted changes, then run the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish.

  Review disposition: Rust-project, Microsoft, and Google reviews found no actionable issues. Rust-skills P2 (bare `cargo test -p blkit-core` skips `concurrency`, `engine`, `store`, and `logging`) **accepted**: the fresh bare-package run omitted those suites; the feature-enabled run exercised all four successfully. Updated task 3.1's verification command rather than enabling runtime features by default, which would weaken CLI build isolation. No other findings or disagreements to resolve. Ponytail-review of the revised diff: **Lean already. Ship.** No actionable simplification. Re-verified the feature-enabled core tests, CLI isolation check, formatting, Clippy, strict OpenSpec validation, and full serial workspace suite.
