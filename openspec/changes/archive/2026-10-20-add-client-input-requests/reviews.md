# Independent review reports — add-client-input-requests

All four Rust-guidance reviewers independently examined the same committed Rust range `79dc6edb2adfa171c2f568bc347c8f7de9b04b70..97d3cba` without seeing one another's findings. They made no edits or test runs. Parent decisions for **every** finding, evidence, and fix/re-review rounds are in [review-notes.md](review-notes.md) §6.3. Original full outputs were saved in the parent session's subagent artifacts; the reports below preserve their findings and verdicts.

## Rust-project guidance

- Correct: The public request API validates response fields and types before accepting a response (`crates/blkit-core/src/compiled_graph.rs:36–160`; local and PostgreSQL acceptance paths). The committed Rust diff adds no `unsafe`.
- **No issues found.** Merge verdict: **OK** for this source-focused review.
- Limit: No builds/tests; external guideline pages could not be opened, so no disputed language-rule claim was asserted.

## Microsoft pragmatic guidance

- Correct: Response validation returns errors for malformed values (`compiled_graph.rs:36–155`), and PostgreSQL listener failures/recovery have integration coverage (`tools/dev-harness/tests/postgres.rs:5185–5240,5962–6063`); no new mock layer needed.
- **P2 recommendation:** `RemoteSubscription::drop` unconditionally called `tokio::spawn` (`distributed.rs:177–184`); dropping this public subscription outside an active Tokio runtime panics. Under [M-PANIC-IS-STOP](https://microsoft.github.io/rust-guidelines/guidelines/correctness/#M-PANIC-IS-STOP), use `Handle::try_current()` and allow interest expiry without a runtime.
- **P2 recommendation:** Scheduler queried stored answers even when `checkpoint.requests()` was empty (`scheduler.rs:253–273`), adding a store query to ordinary graphs. Under [M-HOTPATH](https://microsoft.github.io/rust-guidelines/guidelines/performance/#M-HOTPATH), skip those queries. Do not infer that this caused the watched-request latency gap.
- Verdict: **OK with notes.** Limit: no tests/build or sustained production-like load.

## Google Comprehensive Rust guidance

- Correct: Request activations retain typed schemas and reject invalid responses before graph advancement (`compiled_graph.rs:709–750`), with generated-type agreement tests (`crates/blkit-transpiler/tests/generated.rs:2197–2261`).
- **P2:** `scheduler.rs:277–279` cloned the whole `NamedRegistry` for each accepted reply though both executor callers already own `Arc<NamedRegistry>`; pass/clone the Arc instead. Context: [ownership guidance](https://google.github.io/comprehensive-rust/idiomatic/welcome.html#dont-fight-the-borrow-checker).
- **P2:** `runtime/mod.rs:133–144` cloned top-level input and nested child JSON on each reply although `complete_request` takes `&Value`; borrow inputs while mutating separate checkpoint fields. Same contextual ownership guidance.
- Verdict: **OK with notes.** Limit: no tests/build; performance recommendations not benchmarked.

## leonardomso/rust-skills guidance

- Correct: The branch adds durable request checkpoints, persisted-schema validation, and fenced distributed consumption (`postgres_store.rs:287–458`); validation tests cover malformed and nested values (`tests/request_validation.rs:1–192`).
- **P1 correctness:** The global `RemoteState` mutex spanned PostgreSQL registration, snapshots, resync, hints and renewal (`distributed.rs:72–107,219–259`). A blocked registration for A prevented unrelated watcher B from receiving hints or renewing. Move DB awaits outside the global guard, retain per-instance ordering and add a blocked-A/B-live regression. Rules: `.pi/skills/rust-skills/rules/async-no-lock-await.md`, `anti-lock-across-await.md`.
- Verdict: **BLOCK** pending that fix. Limit: static review; no tests/build.

## Ponytail review of revised branch

Reviewed `79dc6edb2adfa171c2f568bc347c8f7de9b04b70..6e5ce95` (418 KB committed diff) and connected runtime/store/SSE/tests/specs/README; no tests run and not every line of the diff was inspected. The branch adds typed source requests, durable replies and local/remote SSE. PostgreSQL reply consumption is fenced and atomic; README documents the required authenticated per-instance ingress.

1. **Must fix — local reply behind unrelated timer (P1).** Local `accept_response` did not wake `waiting`; `set_wait` could commit after a reply (`store.rs:345–356,439–455`, `runtime/scheduler.rs:14–46,323–334`). Both commit orders could leave a valid reply idle until the timer. Fix both the conditional handoff and reply wake/re-drive; test with a long timer. If skipped, unrelated timer controls reply progress.
2. **Must fix — blocked resync expires unrelated interests (P1).** Listener disabled all renewal until every reconnect resync completed, while A's registration could block indefinitely (`distributed.rs:263–285,296–301,332–343`; `postgres_store.rs:188–205`). A lock held beyond the lease can cause B's healthy stream to miss changes. Keep successfully resynced B renewing while A blocks; test B across lease expiry. If skipped, the stream silently loses hints after listener recovery.

Verdict: **BLOCK** until both fixes. The measured watched-commit p95 is a deployment risk, not proof of NOTIFY causality. Parent resolution, focused tests, and independent re-reviews are documented in [review-notes.md](review-notes.md) §6.3.
