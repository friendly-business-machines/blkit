# Tasks

## 1. Decision model language and evaluation

- [x] 1.1 Add parser/AST support for typed decision-model declarations, dependency links, literal decisions, boxed contexts, and typed knowledge models in `src/compiler.rs` (or a focused module); verify parse and malformed-declaration cases in `tests/language.rs`.
- [x] 1.2 Validate decision-graph dependencies, model/knowledge calls, context scope and types, duplicate/unknown references, and cycles in `src/semantic.rs`; verify accepted multi-decision graphs and diagnostics in `tests/language.rs`.
- [x] 1.3 Add decision-table syntax with expression input columns, rule predicates and outputs, default values, and explicit priorities; verify malformed rows, multiple outputs, and invalid policy/aggregate shapes in compiler tests.
- [x] 1.4 Compile/evaluate all seven hit policies and COLLECT SUM/MIN/MAX/COUNT in `src/codegen.rs`; verify rule order, priority ties, UNIQUE/ANY violations, defaults, no matches, numeric results, and multi-output records using generated-code tests in `tests/generated.rs`.
- [x] 1.5 Add typed business-rule nodes in process graphs, connecting compiled model results to links and gateways; verify a decision-driven process compiles and runs in `tests/graph_syntax.rs` and `tests/generated.rs`, and document its `.bl` example in `README.md`.

## 2. DateTime, waits, and deadlines

- [x] 2.1 Add offset-aware `DateTime` typing, RFC 3339 decoding, expression comparison, and `pause for`/`pause until` parser and type checks; verify invalid offsets, wrong wait types, and dynamic input timestamps in compiler tests.
- [x] 2.2 Add source-configured queue/first-claim process deadlines and reserved timeout/iteration-limit terminal names; verify invalid duration/origin and cycle-without-deadline diagnostics in `tests/graph_validation.rs`.
- [x] 2.3 Persist creation/first-claim timestamps, wake instants, and deadline metadata with additive migrations in `src/store.rs` and `src/postgres_store.rs`; verify old records are readable, first claim is immutable, and due instances are selected in `tests/store.rs` and `tests/postgres.rs`.
- [x] 2.4 Implement local and distributed checkpoint-on-entry waits, claim release, wake reconciliation, and waiting/cancellation status; verify a wait survives restart and worker loss and resumes without extending its wake time in `tests/engine.rs`, `tests/http.rs`, and `tests/postgres.rs`.
- [x] 2.5 Implement atomic deadline reconciliation and fenced timeout terminalization across queued, waiting, retrying, and running states; verify queue expiry with no worker, first-claim takeover, late completion, and cancel-vs-timeout races in `tests/store.rs`, `tests/engine.rs`, and `tests/postgres.rs`; document status/wake/deadline fields in `README.md`.

## 3. Iteration and cyclic graphs

- [x] 3.1 Replace DAG-only validation/scope calculation with cycle-aware route typing and split/join validation, requiring a deadline for every process cycle; verify valid back-edges, missing deadlines, unreachable exits, ambiguous references, and invalid joins in `tests/graph_validation.rs`.
- [x] 3.2 Introduce activation IDs, route-local outputs, and split/join generations in `src/named_runtime.rs`; version existing checkpoint decoding without replaying committed tasks, and verify repeated visits, branch-specific joins, and legacy resume in `tests/engine.rs` and `tests/store.rs`.
- [x] 3.3 Parse and validate pre/post conditional task loops with mandatory count/time bounds and typed zero-iteration results; checkpoint each iteration and verify false first condition, repeated output, bound terminalization, and restart recovery in compiler and engine tests.
- [x] 3.4 Parse and execute sequential/parallel multi-instance tasks over typed lists with indexed checkpoints and ordered `List<U>` results under shared capacity; verify empty input, parallel completion out of order, cancellation, and retry without committed-item replay in `tests/engine.rs` and `tests/concurrency.rs`.
- [x] 3.5 Apply activation-scoped progress and claim-generation fencing in distributed execution; verify worker takeover mid-cycle/multi-instance, bound/deadline precedence, and no stale writes in `tests/postgres.rs`; add a cyclic and multi-instance `.bl` example with documented limits in `README.md`.

## 4. Integration checks

- [x] 4.1 Run `cargo test` and `BLKIT_TESTCONTAINERS_HOST=host.containers.internal cargo test --test postgres` (or the discovered Testcontainers host outside this container); verify decision-driven and long-running workflows pass in both local and distributed modes.
- [x] 4.2 Run `openspec validate decision-models-waits-iteration --strict` and inspect the generated `.bl` example compiled by `cargo run -- <example> /tmp/blkit-decisions.rs`; verify proposal, specs, design, and docs describe the same syntax and outcomes.
