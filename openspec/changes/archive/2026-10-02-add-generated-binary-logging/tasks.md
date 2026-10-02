# Tasks

## 1. Shared logging setup

- [x] 1.1 Add failing tests for default stdout/INFO, selected file append, mixed outputs/level filtering, and invalid level/output/path/endpoint in a generated-binary or isolated initializer harness; verify they fail for the missing behavior.
- [x] 1.2 Implement shared tracing initialization and minimum required dependencies for stdout/file/OTLP HTTP log records; verify the tests from 1.1 pass, including clear stderr initialization errors and no runtime work on startup failure.
- [x] 1.3 Test export against a local OTLP HTTP logs receiver with simultaneous stdout/file output and verify records include a service identity; document best-effort delivery and local file retention in README.

## 2. Generated entrypoints and diagnostic events

- [x] 2.1 Add a failing `tests/project.rs` check that a generated worker and server initialize logging before runtime work, default to stdout, and log readiness/fatal errors without exposing process inputs/results; verify it fails before changes.
- [x] 2.2 Update worker/server templates in `src/project.rs` to initialize shared logging and emit operational events; verify the generated-binary tests from 2.1 pass and `crate` builds still need no configuration.
- [x] 2.3 Add failing checks for worker claim-loss and server storage-error events going to configured destinations; convert their shared diagnostic sites (and PostgreSQL connection diagnostics) to structured events with available identifiers, preserve repository example-binary stderr diagnostics, and verify the focused tests plus existing example/CLI tests pass.
- [x] 2.4 Document environment variables, combined outputs, a generated-worker/server launch example, and the distinction between operational logs and persistent instance status in README; verify documented commands and names match tested behavior.

## 3. Integration and review

- [x] 3.1 Run `openspec validate add-generated-binary-logging --strict`, `cargo test`, and generated project build/run checks with multiple destinations; verify defaults, invalid configuration, output selection, privacy, and exporter behavior against the spec.
- [x] 3.2 Run three independent read-only reviews of the same completed diff using `.pi/skills/review-rust-project/SKILL.md`, `.pi/skills/review-rust-microsoft/SKILL.md`, and `.pi/skills/review-rust-google/SKILL.md` (reviewers must not see each other's findings); adjudicate agreements and disagreements against code and sources, record accepted/rejected findings and reasons, implement accepted changes, then run the existing ponytail-review skill on the revised diff and re-verify affected work. Keep this task unchecked until every stage finishes.
