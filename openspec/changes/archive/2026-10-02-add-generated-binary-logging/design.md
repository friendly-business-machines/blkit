# Design

## Context

See `proposal.md` for motivation and `specs/operational-logging/spec.md` for behavior. `src/project.rs` generates worker/server Rust entrypoints as string templates; those binaries call the shared distributed/runtime/server code. Today the templates and shared code use `eprintln!` for sparse diagnostics. Generated crates depend on blkit, so a shared public initialization function in blkit can avoid adding logging setup code or extra dependencies to generated manifests. The direct `blkit` compiler CLI and repository example binaries are distinct from generated project executables.

## Goals / Non-Goals

**Goals:** One small startup configuration path for both generated executable kinds, consistent event fields and severity, and independent destinations without changing process execution semantics.

**Non-Goals:** Event-sourced instance history, tracing spans/trace export, metrics, rotation/retention, changes to the compiler CLI or built `crate` target, configurable logging for the repository's example binaries, or capturing arbitrary stdout/stderr emitted by custom task crates.

## Decisions

1. **Use `tracing` as the event API and a shared `blkit::logging` initializer.** The generated worker/server templates call it early (after help handling, before connecting to storage or serving/claiming), keeping configuration out of `blkit.toml` and the compiler CLI. The initializer installs a global subscriber with a common level filter, stdout and append-mode file formatting layers, and an OpenTelemetry logs layer backed by an OTLP HTTP exporter. It returns/owns whatever guard is needed for file flushing and exporter shutdown. Alternative: separate `println!`/file/OTLP code per binary; rejected because concurrent output fan-out and consistent filtering become bespoke code. No tracing spans or trace pipeline are installed.

2. **Use environment variables, not an additional project manifest schema.** `BLKIT_LOG_OUTPUTS` accepts comma-separated `stdout`, `file`, `otlp` (default `stdout`, reject empty/unknown/duplicate values); `BLKIT_LOG_LEVEL` accepts standard levels (default `info`); selecting file requires `BLKIT_LOG_FILE`; selecting otlp requires `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` as a valid HTTP(S) OTLP logs URL. The service identity comes from the generated binary's package name/role. Only explicitly selected outputs start; stdout and file use readable timestamped records including levels and event fields. Alternative: adding options to `blkit.toml` or per-binary flags; rejected because output destinations are deployment-time, not build-time, concerns. File output uses append (no automatic rotation); operators own retention.

3. **Instrument shared diagnostic sites that generated binaries rely on, as well as entrypoints.** Convert shared server storage, worker claim/execution, and PostgreSQL connection diagnostics to `tracing` events with IDs and no payloads/URLs; log startup and fatal top-level errors from generated entrypoints. Preserve existing status/checkpoint writes as authoritative. Where migration of shared messages would silence existing repository demonstration binaries, give those binaries a plain stderr subscriber to preserve their existing diagnostics, without exposing the new configurable outputs there. Alternative: instrument only generated entrypoints; rejected because server internal failures and worker claim-loss errors occur in shared code and would bypass selected sinks.

4. **Fail fast for invalid requested configuration, not transient delivery failures.** Initialization validates the file/endpoint and reports errors on stderr before starting work. Once started, exporter delivery is best-effort so collector outages do not block or fail a process instance; flush buffered logs on normal exit when possible. Alternative: make log delivery transactional with execution; rejected because logging is diagnostic and must not alter durable workflow state.

## Risks / Trade-offs

- [OTLP HTTP exporter/library version or shutdown behavior differs from expectations] → Verify against the installed Rust SDK API during implementation; run an integration test against a local HTTP receiver and check normal-exit flush.
- [Shared diagnostics migrate from stderr to a subscriber] → Preserve visible diagnostics in repository example binaries while keeping configuration scoped to generated executables; cover existing CLI/example tests.
- [Exporter outage or abrupt worker termination loses log records] → Document best-effort semantics; never claim audit-grade durability. File rotation and external retention remain the operator's responsibility.
- [Custom task crates could log their own sensitive content] → Avoid logging process payloads in blkit events; document that blkit cannot sanitize arbitrary third-party task logs.

## Migration Plan

Existing generated projects rebuild with the updated blkit CLI and run with default stdout logs without changes to `blkit.toml`; operators opt into additional outputs via environment variables. To roll back, redeploy a previously built binary and remove the new logging environment variables. No persistent data migration is needed.
