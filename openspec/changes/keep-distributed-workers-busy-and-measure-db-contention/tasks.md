# Tasks

## 1. Baseline and regression tests

- [x] 1.1 Measure the current single-connection store under repeated API-only and combined API/worker PostgreSQL workloads described in `design.md` (including mutex wait, operation latency, throughput, and database time); record environment, commands, raw summary, and baseline in `measurement.md`, and verify repeated runs are comparable.
- [x] 1.2 Add failing PostgreSQL-backed tests in `tools/dev-harness/tests/postgres.rs` for replacement claims while a slow claim remains active, for work arriving after capacity frees, and for never exceeding the configured active/task limits; verify targeted tests fail against current batch-only behavior.
- [x] 1.3 Add/extend a drain-during-refill test with queued backlog and a capable peer; verify the draining worker keeps its existing claim and never claims backlog after a committed drain request, and the peer can claim it.

## 2. Keep worker capacity filled

- [x] 2.1 Change `DistributedWorker::run_once` to refill only free slots, poll for eligible work while active slots are free, renew every active claim, and return after all active claims are released and no work is available; verify the new tests and existing PostgreSQL claim/recovery tests pass.
- [x] 2.2 Preserve claim/drain serialization at the PostgreSQL boundary under refill races if the targeted test exposes a gap; verify the committed-drain test and exact-identity/lease-fencing tests pass without double claims.
- [x] 2.3 Verify generated remote worker and combined API/worker entrypoints still drain and exit with in-flight work, and update README only if their documented behavior needs clarification; run targeted generated-project and harness tests.

## 3. Decide connection management with evidence

- [x] 3.1 Re-run the identical API-only and combined workload after worker refill; record p50/p95 mutex wait and operation latency, throughput, database time, repetitions, and baseline comparison in `measurement.md`, and verify whether lock waiting is material versus time in PostgreSQL.
- [ ] 3.2 If lock contention is material, trial a bounded multi-connection option (separate API/worker connections first for cross-role contention, otherwise a small established pool), run the same workload, and record measured benefit and connection count in `measurement.md`; otherwise explicitly record why the single connection stays. Verify the documented keep-or-change decision is backed by numbers, not assumptions.
- [ ] 3.3 Only if the trial measurably improves throughput or p95 latency without harming correctness, implement the smallest bounded option, keep existing store/CLI contracts, add focused PostgreSQL concurrency and drain/fencing tests, and verify those tests plus the workload pass; otherwise remove the trial code/dependency and verify the original store tests still pass.

## 4. Integration checks

- [ ] 4.1 Run `cargo fmt --check`, `cargo clippy --all --workspace`, `openspec validate keep-distributed-workers-busy-and-measure-db-contention --strict`, and `cargo test --all --workspace -- --test-threads=1` with timeout >= 5400 seconds; record actual outcomes and investigate failures rather than treating a timeout as a pass.
- [ ] 4.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (reviewers must not see each other's findings); have the parent adjudicate every finding and disagreement against code and sources and record accept/reject reasons, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish.
