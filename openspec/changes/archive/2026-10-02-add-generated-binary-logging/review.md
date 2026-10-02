# Review disposition

Independent read-only reviews of the pre-fix implementation: [Rust-project](../../../../.pi/skills/review-rust-project/SKILL.md), [Microsoft](../../../../.pi/skills/review-rust-microsoft/SKILL.md), [Google](../../../../.pi/skills/review-rust-google/SKILL.md). The three reports were produced in parallel without sharing findings (workflow `8b7c6ebc-f41c-47cb-8d4e-597b16ad9e3a`).

- **Accepted (all three):** `src/logging.rs` defaulted on non-UTF-8 values as if unset. Default only when missing; Unix child-process regression test confirms malformed values fail on stderr before logging.
- **Accepted (Microsoft, Google):** `src/project.rs` discarded fatal failure context. Generated entrypoints now report a stage-specific static reason, deliberately omitting potentially sensitive raw errors, URLs and process data; the generated-binary test checks both roles.
- **Accepted (Microsoft, Google):** `src/server.rs` dropped the available instance ID on storage failure. Structured events now include operation and optional instance ID; existing HTTP behavior and error redaction remain unchanged, covered by the server test.
- **Partially accepted (Google):** `src/distributed.rs` could not distinguish asynchronous claim loss from other execution infrastructure errors. Categorize only the known claim-loss sentinel and other infrastructure failures without exposing the raw error; the claim-loss integration test exercises it. **Rejected:** logging arbitrary execution errors or further categorizing PostgreSQL connection failures: the existing `postgres connection failed` event already identifies the operation and raw errors can contain credentials or URLs. More granular safe error types would be a separate API change, not justified by this change's contract.

No unsafe/FFI or other review findings. See the same workflow's retention-managed `reviews/rust-{project,microsoft,google}.md` reports for the individual findings and severity labels.

## Ponytail review of the revised diff

Lean already. Ship. The subscriber/OTLP dependencies implement explicitly required destinations; the two `run` functions protect output privacy, and the stored-error argument in `internal` keeps the redaction contract testable. No speculative abstraction or replaceable dependency to cut; net: -0 lines possible.

## Re-verification

`openspec validate add-generated-binary-logging --strict`, `cargo fmt --check`, and `git diff --check` passed. All focused RED→GREEN regressions passed. An online full test rerun passed every test except `renaming_project_removes_stale_generated_binary`, whose `cargo update` failed after crates.io timeouts; the isolated test and full suite both passed with `CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/workspaces/blkit/target cargo test -- --test-threads=1` (no failed tests).
