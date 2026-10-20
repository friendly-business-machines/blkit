# Tasks

## 1. Language and typed request metadata

- [x] 1.1 Add failing parser and validation tests for `input_request` prompt/ports, flow and bind availability, invalid types, and malformed declarations; implement task-node parsing and validate with `cargo test -p blkit-transpiler --test language --test graph_validation --test graph_syntax`.
- [x] 1.2 Add compiler/codegen tests for typed output bindings and resolved portable descriptors including named dictionaries; emit compiled request nodes and verify with `cargo test -p blkit-transpiler --test generated` and a generated Rust build.
- [x] 1.3 Add a small `.bl` request/response example and source syntax documentation in `README.md`; verify the example validates using the documented CLI command.

## 2. Graph activations and local durability

- [x] 2.1 Add failing graph tests for independent repeated/parallel/nested request activations and response-dependent routing, then implement checkpointed activation IDs, request creation and completion in `compiled_graph.rs` and subprocess handling; verify with `cargo test -p blkit-core --features 'worker local-persistence' --test engine`.
- [x] 2.2 Add local-store tests for atomic checkpoint/request persistence, `waiting-for-input` release, response CAS, deadline/cancel races, and restart from persisted state; implement the Turso schema/transactions and local poller wake-up, and verify with `cargo test -p blkit-core --features 'worker local-persistence' --test store --test engine`.
- [x] 2.3 Add a local runtime test where a request is answered while a sibling task is running, then make the scheduler consume that response at a safe checkpoint without losing it or holding an idle worker; verify with the focused engine/concurrency tests.

## 3. Distributed request safety and shared validation

- [x] 3.1 Add contract tests for shared JSON response validation against supported compiled port descriptors (including nested named dictionaries, lists, decimal/temporal values and invalid extras); implement the portable validator and verify local/API-versus-worker agreement with the focused generated and core tests.
- [x] 3.2 Add PostgreSQL tests for atomic request/checkpoint commits, fenced owner updates, concurrent reply CAS, reply versus release/timeout/cancel, durable no-worker replies and unchanged retry/deadline origins; implement the remote schema and claim transitions and verify with focused `tools/dev-harness/tests/postgres.rs` and `postgres_contention.rs` tests.
- [x] 3.3 Document distributed request persistence, typed validation and versioned-worker rollout in `README.md`; verify the documented commands and worker-version behavior in the PostgreSQL integration tests.

## 4. HTTP response and local SSE

- [x] 4.1 Add HTTP tests for request visibility and response POST (malformed JSON, invalid types, missing request, duplicate, late answer, success) in local and distributed router modes; wire `server.rs` to the store transitions and verify with focused `tools/dev-harness/tests/http.rs` and `postgres.rs` tests.
- [x] 4.2 Add local SSE tests for snapshot-before-change, live request/resolution/cancel events, reconnect snapshot and slow/disconnected clients; implement bounded in-process commit notification and SSE stream handling and verify with focused HTTP tests.
- [x] 4.3 Document POST, GET and SSE request/response examples, SSE reconnect behavior, client-type limitations and ingress authorization in `README.md`; verify example responses with HTTP integration tests.

## 5. Selective remote SSE notifications

- [x] 5.1 Add multi-replica PostgreSQL tests for per-replica channels, leased instance interests, no-interest/no-unrelated-checkpoint notifications, rollback and expiry; implement narrowly scoped database-side fan-out and verify with `tools/dev-harness/tests/postgres.rs`.
- [x] 5.2 Add tests for committed LISTEN-before-snapshot ordering, listener outage and reconnect while SSE clients stay attached, expired live registration recovery and duplicate hints; implement the dedicated listener, renewal/resync and per-replica SSE fan-out, then verify with remote HTTP/PostgreSQL tests.
- [x] 5.3 Document the additional PostgreSQL connection per API replica, NOTIFY commit/queue risks, listener recovery and diagnostics; verify with an explicit two-replica integration test and `pg_notification_queue_usage()` inspection.

## 6. Integration and review

- [x] 6.1 Run a representative concurrent request-transition/fan-out load check, record commit latency, SSE notification latency, NOTIFY queue use and session count against the existing role budgets; verify the results are documented in this change's review notes and that ordinary checkpoints do not emit NOTIFY.
- [x] 6.2 Run `cargo fmt --all -- --check`, relevant clippy/build checks, `openspec validate add-client-input-requests --strict`, and the full suite `cargo test --all --workspace -- --test-threads=1` with a timeout of at least 5,400 seconds; record results and resolve failures before checking this task.
- [x] 6.3 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (leonardomso/rust-skills; reviewers must not see each other's findings); have the parent review all four reports and adjudicate every finding, including disagreements, against code and sources, record accept/reject with reasons, implement accepted changes, then run the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Verify the reports, adjudication, review and re-verification are recorded; keep this task unchecked until every stage is finished.
