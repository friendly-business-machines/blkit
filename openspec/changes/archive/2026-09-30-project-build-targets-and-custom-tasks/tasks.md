# Tasks

## 1. Project crate build

- [x] 1.1 Add `blkit build` manifest parsing with exactly one `build_target` and exact blkit CLI version, and recursively discover project `.bl` files while excluding generated/hidden directories; verify tests cover zero files, nested files, invalid/multiple/missing targets, and version mismatch while existing `blkit SOURCE.bl OUTPUT.rs` tests still pass.
- [x] 1.2 Parse and merge discovered files by namespace/version before validation and generation, retaining source paths in diagnostics; verify tests cover cross-file types, tasks, decisions and forward references, isolated groups, and duplicate declarations.
- [x] 1.3 Generate one `.blkit/` Cargo library from validated namespace/version groups, aggregating compiled definitions without duplicate process identities; verify an integration test imports the library from a multi-file project and observes an executable cross-file process and independent identities.
- [x] 1.4 Resolve blkit and optional extension dependencies through Cargo, retain `.blkit/Cargo.lock` across builds and require locked resolution on rebuild; verify a local test dependency remains at its locked revision/version until explicitly updated and unresolved dependencies fail clearly.
- [x] 1.5 Document automatic source discovery, scope rules, `build_target`, crate build, generated-vs-authored files, lockfile handling, and Rust integration in README; verify a documented multi-file crate example builds.

## 2. Custom async task nodes in the shared crate

- [x] 2.1 Define and load a discoverable task descriptor shipped by resolved extension crates, including exported name, function path, and `.bl` input/output types; verify local extension-crate tests for valid metadata, missing metadata, unknown exports, and malformed paths/types.
- [x] 2.2 Parse crate-qualified task calls in `.bl` and validate advertised signatures against types in the caller's namespace/version scope without relaxing source-defined task checks; verify tests cover valid calls across files, unconfigured providers, missing types, and incompatible arguments/downstream uses.
- [x] 2.3 Generate callable glue to the provider Rust crate's async `Value` task function with typed input/output JSON conversion and helpful missing-provider/build errors; verify an integration test builds a local example extension crate and rejects an incompatible callable.
- [x] 2.4 Teach shared local/distributed task scheduling to await custom futures under existing concurrency limits without blocking executor threads; preserve synchronous built-in graph behavior and verify concurrency/limit tests for two I/O tasks alongside a built-in task.
- [x] 2.5 Apply output validation, execution retries, cancellation/deadline fencing, and at-least-once interruption behavior to async tasks; verify tests for malformed output, failure/retry, cancellation after side effect, and uncommitted work re-execution.
- [x] 2.6 Document qualified `.bl` calls, the crate task-descriptor format, the extension function interface, error handling, idempotency obligation, and async-only execution limits in README; verify the documented extension example builds and executes.

## 3. Worker packaging target

- [x] 3.1 Add a thin `worker` binary target over the generated crate and existing distributed worker/store; verify a project build contains its process identities and an integration test claims matching work but not other process versions.
- [x] 3.2 Document PostgreSQL configuration and the separate distributed API prerequisite for project worker builds; verify the documented worker build/run command against an available test database.

## 4. All-in-one server packaging target

- [x] 4.1 Add a thin `server` binary target over the same generated crate and existing local engine/store/REST router, binding to loopback by default; verify a project-built server accepts, executes, and reports a compiled process through REST without PostgreSQL.
- [x] 4.2 Exercise an async custom task through the project-built local server and after restart with a committed checkpoint; verify result/type handling and no replay of committed work in integration tests.
- [x] 4.3 Document server target configuration and the distinction between local all-in-one server and distributed worker/API topology; verify README start/request examples work as written.
