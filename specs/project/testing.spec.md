---
name: Testing
description: Test suite philosophy and conventions — Interface Specifications serve as the authoritative checklist for test coverage of the Go implementation
status: implemented
---

# Testing

blkit's test suite is written directly in Go alongside the implementation. The Interface Specifications serve as the checklist for what must be tested, ensuring coverage does not drift through omission.

## Testing Framework

The Go standard-library [`testing`](https://pkg.go.dev/testing) package **only** — no third-party test framework. Assertions are plain comparisons that fail via `t.Errorf` / `t.Fatalf` (`if got != want { t.Errorf("got %v, want %v", got, want) }`), tables are driven with `t.Run` subtests, and lifecycle/fixtures use `testing` primitives (`t.Cleanup`, `t.Helper`, `TestMain`). Tests are run with `go test ./...`.

All tests run in-process — no separate test process, no browser runtime, no external services beyond what an individual test fixture deliberately spins up. The message-broker backends share a conformance suite that lives in core: the in-memory broker runs it in-process with no setup, NATS runs it against an embedded `nats-server`, Redis/Valkey and RabbitMQ spin up throwaway containers via [testcontainers-go](https://golang.testcontainers.org/), and the cloud brokers (Azure Service Bus, Google Pub/Sub, AWS SQS/SNS) run against their emulators / LocalStack via testcontainers-go — matching how the state-store SQL backends are tested.

## Test Location

Tests live alongside the source they cover, following the standard Go convention: a file at `processes/task.go` is paired with `processes/task_test.go` in the same package. Black-box tests that exercise the public API only may use the `<package>_test` package name in the same directory to enforce access through exported identifiers.

The package layout is described in [project-directory-structure.spec.md](project-directory-structure.spec.md); the repository is flat — implementation packages live at the repository root, not under a `go/` subdirectory.

## Spec-to-Test Mapping

Each spec in `specs/` corresponds to one or more test files in the package that implements it. When a spec describes a class or module, each section of that spec must be covered by at least one test case. The spec is the authoritative checklist; a missing test for a spec requirement is a coverage gap, not a deliberate omission.

Test files mirror the spec hierarchy directly:

```
specs/expressions/number.spec.md           → number_test.go (root blkit package)
specs/decision-tasks/decision-table.spec.md → decisions/decision_table_test.go
specs/processes/task-nodes.spec.md         → processes/task_test.go (and per-task companions)
specs/data/state-store.spec.md             → data/state_store_test.go
specs/message-brokers/overview.spec.md     → core/message_broker_test.go (and brokers/<name>/broker_test.go per backend)
specs/worker/worker.spec.md                → worker/worker_test.go
```

Larger specs may span several test files split by section — e.g. `task-nodes.spec.md` is split across `task_test.go`, `subprocess_task_test.go`, `trigger_process_task_test.go`, and `request_input_task_test.go` to match its target files.

## Coverage Requirements

Every public method and constructor defined in an Interface Specification's pseudocode block is an implicit test requirement. A test implementation is considered complete for a given spec when:

- Every method in the pseudocode block has at least one positive-path test.
- Every documented error condition or edge case has a corresponding test.
- Methods that return `(T, error)` are exercised on both the success and error branches.
- Cancellation-aware methods (those accepting `context.Context`) are tested with a cancelled context where the behaviour is observable.

Tests must not rely on implementation details — they exercise only the public API surface described in the spec. Package-private helpers may have their own focused tests in the implementation file, but the spec-level checklist is satisfied only by tests against the public surface.

## Documentation Examples as Tests

The working code samples in the Examples section of the documentation site are a first-class part of the test suite. Each `docs/examples/<name>/example.go` file represents a complete, end-to-end use of the blkit public API against a real business scenario, and must be verified to compile and produce the expected output on every CI run.

### Requirements

- Every example file under `docs/examples/<name>/` must be compiled and executed as part of the CI test run.
- Each example must produce deterministic, verifiable output. The expected output is recorded alongside the example (e.g. as an `// Output:` comment block, recognised by `go test`, or in an adjacent fixture) and asserted in CI.
- An example that compiles but produces incorrect output is treated as a test failure, not a documentation issue.
- A broken example blocks the same CI checks as a broken unit test — it must be fixed before merging.

### Location and Naming

Example test drivers live alongside the other tests, in a top-level `examples/` package:

```
examples/<name>/<name>_test.go
```

The driver imports the example source from `docs/examples/<name>/example.go` (or reproduces it inline if the doc toolchain does not support direct import) and asserts the expected output.

### Relationship to Business Process Specs

The business process spec in `specs/examples/<name>.spec.md` defines the expected behaviour — inputs, rules, and outcomes — and the worked examples in that spec provide the test cases. Each worked example row in the spec must have a corresponding assertion in the test driver. If the spec is updated (e.g. a new worked example row is added), the test driver must be updated to match before the change is considered complete.

## Adding Tests for a New Spec

When a new Interface Specification is written or an existing one is updated, the contributor must:

1. Identify every new public method or behaviour described in the spec.
2. Add corresponding test cases before the implementation is considered complete.
3. Link the test files from the spec's Verification section (plain markdown links, per [spec-format.spec.md](spec-format.spec.md)) so coverage can be traced.
4. Run the full test suite (`go test ./...`) to confirm no regressions.
