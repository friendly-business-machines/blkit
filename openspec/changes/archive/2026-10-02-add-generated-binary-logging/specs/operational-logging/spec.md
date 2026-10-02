# Spec Delta

## Purpose

Defines configurable operational log records for worker and server executables built from blkit projects, so operators can diagnose execution without changing the process API or storing event history.

## ADDED Requirements

### Requirement: Generated executables configure operational logging at startup
Worker and server executables produced by `blkit build` SHALL configure operational logging before starting runtime activity. They SHALL default to stdout at INFO level. Operators SHALL be able to set `BLKIT_LOG_LEVEL` to a supported level and `BLKIT_LOG_OUTPUTS` to a comma-separated nonempty selection of `stdout`, `file`, and `otlp`; each selected destination SHALL receive the same blkit operational events, including when multiple destinations are selected. The `crate` build target and `blkit` compiler CLI SHALL NOT require logging configuration.

#### Scenario: Default console logging
- **WHEN** a generated worker or server starts without logging environment variables
- **THEN** INFO-and-higher operational log records are emitted to stdout

#### Scenario: Multiple outputs and filtered level
- **WHEN** a generated worker starts with `BLKIT_LOG_OUTPUTS=stdout,file,otlp` and `BLKIT_LOG_LEVEL=error` and valid destination settings
- **THEN** ERROR-level operational records are sent to all three destinations and lower-level records are filtered out

#### Scenario: File-only output
- **WHEN** a generated server selects only `file` and supplies `BLKIT_LOG_FILE` pointing to a writable file
- **THEN** operational records append to that file and are not also sent to stdout

### Requirement: File and OpenTelemetry log destinations are operator-configurable
When `file` is selected, generated executables SHALL append records to the file named by `BLKIT_LOG_FILE`, without truncating prior contents. When `otlp` is selected, generated executables SHALL export OpenTelemetry log records to the OTLP HTTP endpoint specified by `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`. Exported records SHALL identify the running service. Traces and metrics are not required.

#### Scenario: Append across restarts
- **WHEN** a generated server restarts with the same configured log file
- **THEN** new records are appended and prior records remain intact

#### Scenario: OpenTelemetry export
- **WHEN** a generated worker enables `otlp` with a reachable OTLP HTTP logs endpoint
- **THEN** its operational records are exported as OpenTelemetry log records with a service identity

### Requirement: Invalid logging configuration fails visibly
A generated worker or server SHALL refuse to start runtime activity if a selected output cannot be initialized or if the requested outputs, level, file path, or OTLP endpoint are invalid or missing. It SHALL report the configuration error to stderr even when stdout is disabled. Failures to deliver an individual record after successful initialization SHALL NOT prevent process execution.

#### Scenario: Unwritable file
- **WHEN** `file` is selected but its file cannot be opened for append
- **THEN** startup fails with a diagnostic identifying the logging configuration error and the server does not listen or the worker does not claim work

#### Scenario: Missing endpoint
- **WHEN** `otlp` is selected without `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`
- **THEN** startup fails with an actionable diagnostic to stderr

### Requirement: Operational events are useful without exposing process payloads
Generated worker and server executables SHALL log startup/readiness and execution or storage failures with appropriate severity and available worker or instance identifiers. Blkit-generated operational records SHALL NOT include process input or result payloads, database credentials, or connection URL contents. Persistent instance status and checkpoints SHALL remain the source of truth for execution outcomes; operational logs SHALL NOT become an event history.

#### Scenario: Worker loses a claim
- **WHEN** a generated worker detects that an instance claim cannot be renewed
- **THEN** it emits a failure record with the instance identifier but no input or result payload

#### Scenario: Server storage error
- **WHEN** a generated server encounters an internal storage error while serving an instance request
- **THEN** the selected outputs receive an error record, and the existing HTTP error behavior is unchanged
