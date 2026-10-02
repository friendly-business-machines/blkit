# Proposal

## Why

Worker and server executables produced by `blkit build` currently emit only a handful of plain stderr messages. Operators need configurable operational logs in their console, a file, and/or an OpenTelemetry collector without changing the process language or persisting an event history.

## What Changes

- Generated worker and server binaries initialize operational logging on startup, defaulting to stdout.
- Operators can enable stdout, an append-only file, and OpenTelemetry log-record export independently and simultaneously, and control log verbosity with environment variables.
- Startup and execution failures are logged with useful worker/instance identifiers where available; process inputs and results are not logged.
- Configuration errors for requested outputs fail startup with actionable diagnostics. Document configuration and verify generated-binary behavior.

## Capabilities

### New Capabilities
- `operational-logging`: Startup configuration, destinations, and safe operational log records in generated worker/server executables.

### Modified Capabilities
- None; existing project-build behavior remains intact.

## Impact

Affects generated executable templates in `src/project.rs`, shared runtime diagnostics they invoke, generated-binary tests, and README documentation. Requires Rust logging and OpenTelemetry exporter dependencies. Does not add configurable logging to the `blkit` compiler CLI, `crate` build target, or repository example binaries, and does not change REST APIs or database schemas; traces, event history, and file rotation are out of scope. Existing example-binary diagnostics must remain visible if shared diagnostics are migrated.
