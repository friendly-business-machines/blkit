# Tasks

## 1. Named explicit `.bl` graphs and executable representation

- [x] 1.1 Write failing parser tests for named start/task/split/join/terminal nodes, ordered explicit links, and process retry declarations; implement parsing while keeping typed task-body `return` and verify `cargo test --test graph_syntax` passes.
- [x] 1.2 Write failing validation tests for unknown/duplicate/unreachable nodes, cycles, dead ends, and unmatched or unstructured split/joins; implement explicit-graph structure checks and verify `cargo test --test graph_validation` passes while legacy fixtures still run.
- [x] 1.3 Write failing validation tests for typed link arguments, normal `end` outputs, path-available gateway conditions, AND record joins, and named exceptional terminals; implement type checks and verify graph/language tests pass.
- [x] 1.4 Write failing executor tests for a compiled named-node/link graph that restores a serializable checkpoint with completed nodes, typed outputs, selected XOR/OR routes, and partial AND/OR joins; verify XOR takes the first matching link and OR join results follow selected split-link declaration order; implement the runtime graph representation and checkpointable executor, and verify `cargo test --test engine` passes.
- [x] 1.5 Write failing tests for max-retries/retry-for/retry-delay/exponential policy validation and defaults; emit retry metadata, executable named graph links, and terminal nodes in generated Rust targeting the runtime representation from 1.4, and verify `cargo test --test generated` passes.
- [x] 1.6 Write failing dev-server tests for registering and dispatching generated named graphs, then wire the bundled `blkit-dev` binary to those definitions; migrate `examples/approve.bl`, `examples/graph.bl`, `build.rs` fixtures and legacy process tests to explicit links/end nodes; write a failing test for old process-level `return`, reject implicit process syntax while retaining task `return`, document the breaking syntax in README, and verify `cargo test --test cli --test generated --test language --test graph_validation --test http` and the full suite pass. Restart/retry recovery remains in 3.1.

## 2. Turso checkpoint store and durable graph transitions

- [x] 2.1 Write failing store tests for checkpoint, attempt count, first-failure time, next eligibility, and terminal event name persistence/reopen; migrate the Turso schema while retaining existing terminal records and verify `cargo test --test store` passes.
- [x] 2.2 Write failing parallel-task tests where B's completion commits before C fails and vice versa; serialize process-state transitions, persist completed outputs/routing before successor dispatch through the Turso checkpoint store from 2.1, and verify committed B is not rerun on retry while uncommitted work may be rerun.
- [x] 2.3 Write failing tests for normal typed end, named business error, process-wide cancel, and terminate with in-flight siblings; implement distinct outcomes and cancellation hooks, and verify `cargo test --test engine` passes.
- [x] 2.4 Write failing tests for execution-error versus business-error handling, `max_retries` additional attempts, first-failure retry window, minimum delay, exponential backoff, and whichever limit expires first; implement one shared retry decision for both storage modes and verify engine tests pass.
- [x] 2.5 Update README's execution/status description for checkpoints, named terminal outcomes, and at-least-once uncommitted work; verify the documented example and engine tests agree.

## 3. Local Turso recovery and REST parity

- [x] 3.1 Write failing restart tests for interrupted running work with and without retry, preserved completed parallel outputs, pending work, and accepted cancellation; extend the named-graph local dispatch from 1.6 with persisted checkpoints and retry-aware restart/recovery through the shared executor, and verify `cargo test --test store --test engine` passes.
- [x] 3.2 Write REST tests for retry-waiting, named business error versus execution failure, terminated outcome, and cancellation while waiting; update `src/server.rs`/`src/bin/blkit-dev.rs` only if needed and verify `cargo test --test http` passes.
- [x] 3.3 Document the local restart/retry behavior and Turso migration in README; run the documented single-binary dev-server example and verify it reaches its end node.

## 4. PostgreSQL queue and ownership

- [x] 4.1 Add the Rust `testcontainers-modules` PostgreSQL dev dependency with a pinned PostgreSQL image tag; verify an integration test launches and cleans up an isolated database through this devcontainer's Podman socket, then add schema/store operations for queued/claimed instances, checkpoints, retry metadata, workers, and advertised identities and verify create/get/reopen tests pass.
- [x] 4.2 Write concurrent-claim integration tests for exact namespace/version/name eligibility, multiple instances per worker, `SKIP LOCKED` exclusivity, lease renewal, and monotonically changing claim generation; implement claim/renew operations and verify tests pass against the Testcontainers PostgreSQL instance.
- [x] 4.3 Write integration tests for expired claim reconciliation with and without retries, idempotent concurrent reconcilers, and an old worker unable to commit after a new claim; implement lease fencing and reconciliation and verify tests pass against the Testcontainers PostgreSQL instance.
- [x] 4.4 Document the Testcontainers-based test command, PostgreSQL queue-as-instance-row semantics, lease configuration, and test isolation; verify the documented command starts, checks readiness, runs tests, and cleans up PostgreSQL.

## 5. Distributed worker, rollout, and control

- [x] 5.1 Write tests that a worker binary advertises only its linked process identities, heartbeats, runs multiple claimed instances under a local task bound, and performs no runtime `.bl` compilation; implement worker launch/claim loop and verify tests pass.
- [x] 5.2 Write integration tests for worker drain, old/new version routing, queued old-version work without a capable worker, and retry released during drain; implement graceful drain and verify tests pass against Testcontainers PostgreSQL.
- [x] 5.3 Write REST and worker integration tests for PostgreSQL-backed start/status/cancel, cancellation racing with checkpoint/lease expiry, and remote in-flight hook signaling; implement distributed REST launch and verify tests pass against Testcontainers PostgreSQL with loopback binding by default.
- [x] 5.4 Write a two-worker process test against the same Testcontainers PostgreSQL instance that kills an owner mid-instance after B's parallel output commits, observes lease expiry, and proves a capable worker resumes C without rerunning B; verify stale-owner writes cannot overwrite the result.
- [x] 5.5 Document building/linking generated Rust into API/worker binaries, starting PostgreSQL/API/workers, version rollout and drain, authenticated ingress for remote REST, and at-least-once side-effect limits; verify the documented multi-worker example executes.

## 6. Integration checks

- [x] 6.1 Run `cargo fmt --check`, `cargo test` including the Testcontainers PostgreSQL integration tests, and `openspec validate distributed-execution --strict`; verify all pass and that no broker, cyclic execution, runtime `.bl` compiler, or graph-compatibility fingerprint was added.
