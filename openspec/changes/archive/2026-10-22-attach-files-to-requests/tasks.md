# Tasks

## 1. Attachment storage and durable metadata

- [x] 1.1 Add feature-gated async `object_store` integration for local filesystem and S3/GCS/Azure, with a persistent local default beside the database, mandatory cloud settings for all remote roles, startup validation, generated keys, and role-appropriate feature flags; write tests for missing/invalid remote settings, unwritable local roots, path safety, and object roundtrips, then run focused core tests and feature checks.
- [x] 1.2 Add idempotent `instance_attachments` schema and accessors to Turso and PostgreSQL for instance and optional input-request scopes; verify FK, ordering, restart, consumed-response persistence and no byte columns with local and remote store tests. Document the schema and required storage provisioning in `README.md`.
- [x] 1.3 Expose worker-side Rust attachment metadata/list/read access by instance and optional request ID, without exposing raw keys in client output; verify local restart and separate remote worker can read uploaded objects through focused tests and document the worker accessor.

## 2. HTTP admission and reply handling

- [x] 2.1 Add bounded streaming multipart parsing to the existing start and response routes, accepting respectively one `input` or `response` JSON part plus repeated `files` parts while preserving JSON-only calls; add API tests for valid multiple files, wrong parts/JSON, filenames, file count and byte limits, and run focused HTTP tests. Document multipart examples for both routes and exact limits in `README.md`.
- [x] 2.2 Make local and distributed start persistence atomically commit instance plus attachment references after uploads; write failure-injection tests for upload/DB errors and local typed-invalid and remote deferred typed-invalid inputs, then run local and PostgreSQL start tests.
- [x] 2.3 Make local and distributed input-request response persistence atomically commit the validated reply plus attachment references, using existing outstanding/active/deadline predicates; test duplicate replies, cancellation and deadline races, remote notification/worker resumption, and JSON-only compatibility, then run local and PostgreSQL reply tests.
- [x] 2.4 Clean up uploaded objects after rejected or failed starts/replies without deleting objects for commits of uncertain outcome; test partial uploads, commit errors, and failed reply races, then run focused tests. Document crash-orphan audit and retention/backup limitations in `README.md`.

## 3. Generated deployment paths and integration

- [x] 3.1 Wire required attachment-store initialization into generated local (persistent default), remote API-only, worker-only, combined and split binaries and dev-harness binaries, keeping reusable crates and unused roles free of storage dependencies; verify startup fails without cloud configuration for every remote role, local default is writable, generated feature/manifest tests pass, and each affected target compiles.
- [x] 3.2 Add end-to-end local and distributed multipart start and input-reply tests showing that a worker retrieves the correct bytes after a handoff/restart and that the database stores references only; run the focused generated-project and PostgreSQL integration tests.
- [x] 3.3 Document mandatory cloud backend credentials/bucket configuration for each remote role, the persistent local default and override, and database/object backup requirements in `README.md`; verify examples match generated binaries and run `openspec validate attach-files-to-requests --strict`.

## 4. Integration verification and review

- [x] 4.1 Run `cargo fmt --check`, `cargo clippy --all --workspace`, and `cargo test --all --workspace -- --test-threads=1` with a tool timeout of at least 5400 seconds; record actual results, fix regressions, and repeat affected checks.
- [x] 4.2 Run four independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, `.pi/skills/review-rust-google/SKILL.md`, and `.pi/skills/rust-skills/SKILL.md` (leonardomso/rust-skills; reviewers must not see each other's findings); have the parent review the four reports and adjudicate every finding (including disagreements) against the code and sources, record which to accept or reject and why, implement accepted changes, then use the existing ponytail-review skill on the revised diff, address its actionable findings, and re-verify affected work. Keep this task unchecked until all stages finish.
