# Tasks

## 1. Comparable baseline and regression tests

- [x] 1.1 Extend the disposable-PostgreSQL `postgres_contention` harness with a worker-only, persistence-heavy workload and comparable per-mode measurements for API-only, worker-only, and combined roles; retain its explicit disposable opt-in and existing combined 72/72/0 assertions. Record repeated single-client baseline runs, environment, commands, windows and raw summaries in `measurement.md`; verify all three modes pass before changing `PostgresStore`.
- [x] 1.2 Add PostgreSQL-backed RED tests for two independent operations progressing on separate sessions, a bounded maximum under concurrent load, and a checkout that times out while the only slot is held; verify they fail or do not compile against the current single-client store, without changing production code in this task.

## 2. Bounded pooled PostgreSQL store

- [x] 2.1 Add compatible `deadpool-postgres` behind `remote-persistence`, implement a clonable pooled `PostgresStore` with the unchanged `connect(url)` signature/default four and a positive-maximum pool-size constructor, and preserve fail-fast schema initialization and bounded checkout/create/recycle waits. Verify targeted new constructor/startup/invalid-limit tests go RED then GREEN and feature-limited crate builds still exclude PostgreSQL pooling.
- [x] 2.2 Convert every existing store SQL site to a short-lived checkout, holding one checkout across each transaction including early return/rollback; preserve claim worker-row-before-instance ordering and do not add write retries. Verify targeted two-operation/concurrency and limit/timeout tests go GREEN plus existing PostgreSQL claim/rollback, drain, and lease-fencing tests.
- [x] 2.3 Add RED/GREEN PostgreSQL-backed backend-loss/replacement and cross-connection claim/drain tests plus an active-claim pool-contention/renewal regression; verify no lost capacity, false commit, double claim, stale checkpoint, or unbounded wait, and run the full PostgreSQL test file.

## 3. Remote role configuration and deployment guidance

- [x] 3.1 Add generated-binary tests for default and overridden `BLKIT_API_DB_POOL_MAX` and `BLKIT_WORKER_DB_POOL_MAX` on API-only, worker-only, split and remote combined targets; verify invalid active-role settings fail with their names, unused-role settings are ignored, positional arguments remain accepted, connection budgets are independently bounded, and local targets retain no PostgreSQL dependency. Observe RED behavior before modifying generation, then implement and verify GREEN generated-project tests.
- [x] 3.2 Document the two settings, default four per role, sum-of-process/role PostgreSQL connection budgeting, startup and checkout failure behavior, and rollback-by-setting-`1` guidance in `README.md`; verify commands and names match built/generated binaries and re-run targeted generated-role drain and smoke tests.

## 4. Pool-size evidence

- [x] 4.1 Run the identical opt-in disposable workloads at API/worker maxima 1, 4, 8, and 16 (within PostgreSQL connection capacity) with repeated runs for API-only, worker-only and combined modes. Record throughput, p50/p95 operation and pool-checkout waits, SQL time, actual/peak sessions, 72/72/0 combined checks, environment, reproduction commands and limitations in `measurement.md`; record the operator's decision to retain the conservative default of four despite measured API gains at eight, with eight available as a capacity-budgeted opt-in for later stress testing.

## 5. Integration and independent review

- [x] 5.1 Run `cargo fmt --all -- --check`, `cargo clippy --all --workspace`, `openspec validate add-bounded-postgres-role-pools --strict`, and `cargo test --all --workspace -- --test-threads=1` with tool timeout at least 5400 seconds; record real results, investigate failures, and do not treat a timeout as a pass.
- [x] 5.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (leonardomso/rust-skills; reviewers must not see each other's findings); have the parent review all four reports, adjudicate every finding and disagreement against code and sources, record accept/reject reasons, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work (including the full serialized suite if implementation changed). Keep this task unchecked until all stages finish.
