# Tasks

## 1. Split compiler from runtime

- [x] 1.1 Reconcile active `add-temporal-expressions` work before moving overlapping sources; record isolated generated crate/worker/server dependency trees and run current CLI/generated tests as a baseline.
- [x] 1.2 Add failing generated-project/direct-source tests for `blkit-core` imports, exact version pin, and no transpiler dependency; verify they fail on current output.
- [x] 1.3 Create the two-package Cargo workspace and move compiler, semantics, codegen, `Project`, and the `blkit` binary into `blkit-transpiler`, with shared policy/helper types and runtime in `blkit-core`; verify `cargo check --workspace` and CLI tests without any core-to-transpiler dependency.
- [x] 1.4 Adapt `build.rs`, repository example binaries, and package-path fixtures to the workspace; verify direct CLI examples, `cargo check --workspace --all-targets`, and affected CLI/generated tests.

## 2. Isolate additive runtime capabilities

- [x] 2.1 Decouple base compiled graph/evaluation APIs from Turso and PostgreSQL imports; gate local store on `local-persistence` and verify no-default-features core plus local store tests.
- [x] 2.2 Add independent `api-server`, backend-independent `worker`, `remote-persistence`, and `logging` features; gate optional HTTP/PostgreSQL/OTLP dependencies and verify isolated feature-set `cargo check` and normal dependency trees for API-only, worker-only, local combined, remote combined, and both-persistence combinations.
- [x] 2.3 Adapt repository API/worker/dev binaries and existing runtime/HTTP/logging tests to explicit feature sets; verify relevant suites and that `api-server` without `worker` does not compile worker-execution dependencies.

## 3. Poll the local durable queue in-process

- [x] 3.1 Add failing local runtime/HTTP tests for a persisted pending instance being claimed by an in-process worker, immediate invalid-input rejection, cancellation before claim, bounded concurrency, and startup recovery; verify current direct-spawn path does not satisfy the polling checks.
- [x] 3.2 Use existing local instance persistence as the queue, decouple `Engine::start` from spawning execution, and add an in-process worker loop that claims eligible work; verify 3.1 tests, checkpointed retry/wait/deadline recovery, and no replay of committed tasks after restart.
- [x] 3.3 Document local API+worker admission and in-process polling, without claiming a separate process or PostgreSQL queue, and verify the documented local start/restart path in HTTP/store tests.

## 4. Move distributed admission and graph initialization

- [x] 4.1 Add failing PostgreSQL-backed tests for exact identity/version matching, recent heartbeats, draining/stale workers, worker exit after admission, and absent capability; verify current distributed API lacks admission checks.
- [x] 4.2 Persist immutable process/version retry/deadline metadata at worker registration and use it in transactional capability-checked instance admission without graph definitions; verify 4.1 tests and queue-origin timeout/reconciliation tests, including an exited worker.
- [x] 4.3 Add failing tests for accepted typed-invalid JSON, worker initialization of a checkpoint before execution, terminal failure without retry or dispatch, cancellation/deadline fencing, and recovery of previously checkpointed instances; verify current eager API validation fails the tests.
- [x] 4.4 Move distributed typed-input validation and initial checkpoint to the claiming worker and reconcile persisted policies without a graph in the API; verify 4.3 tests plus existing distributed recovery/retry/deadline tests.
- [x] 4.5 Document distributed API admission (live worker required, syntactically invalid JSON rejected, typed failures after 202 acceptance, pending after worker exit) and verify documented requests against HTTP integration tests.

## 5. Generate role-specific project targets

- [x] 5.1 Add failing project/CLI tests for source-free `api-only`, local and remote `api-worker` with required persistence choice, `api-worker-split`, `worker-only`, legacy `server`/`worker` aliases, and invalid combinations; verify current transpilation rejects new targets.
- [x] 5.2 Generate a standalone remote `api-only` project with no compiled process definitions and no `.bl` requirement; validate any present sources, and verify source-free compilation plus no generated graph/worker dependency in the API binary.
- [x] 5.3 Generate combined local/remote API+worker entrypoints and independently buildable split API/worker packages while preserving the root generated library for projects with sources; verify 5.1 tests, package-specific builds, and end-to-end local, remote combined, and remote split smoke tests.
- [x] 5.4 Change codegen/templates and generated manifests to `blkit_core::`, exact matching versions, and role-specific direct dependencies/features; verify direct-source and legacy `crate`/`worker`/`server` consumer/build tests from 1.2 and lockfile/update tests pass.
- [x] 5.5 Document all target configurations, source-free API-only and worker registration, package-specific Cargo commands for split outputs, local queue semantics, and Rust consumer/version migration in README; verify commands against generated sample projects.
- [x] 5.6 Compare isolated normal dependency trees against the 1.1 baseline; verify crate excludes compiler/local/HTTP/PostgreSQL/logging, API-only and split API exclude worker/generated definitions/local, worker-only excludes HTTP/local, local combined builds with both roles.

## 6. Integration and review

- [x] 6.1 Run relevant temporal/generated-language suites, `cargo fmt --check`, `cargo clippy --all --workspace`, `cargo test --all --workspace` (serialize Docker-backed tests if needed), and `openspec validate split-transpiler-runtime-crates --strict`; record environment-limited checks rather than claiming they passed.
- [x] 6.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (leonardomso/rust-skills; reviewers must not see each other's findings); have the parent review the four reports and adjudicate every finding (including disagreements) against the code and sources, record which to accept or reject and why, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish.
