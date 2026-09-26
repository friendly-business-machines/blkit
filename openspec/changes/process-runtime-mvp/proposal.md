# Proposal

## Why

The language MVP produces callable Rust decisions but cannot describe the task graph of a business process or run an instance of that graph. The `.bl` source must own the process map before a single-node runtime can execute, track, persist, and cancel it.

## What Changes

- Add a single-node process runtime with persisted instance identity, status, inputs, and terminal outcomes.
- Extend `.bl` to declare tasks, typed links, and AND/OR/XOR split and join gateways. Gateway conditions may reference process inputs and available upstream task outputs.
- Validate the complete graph in the compiler and emit its executable graph description with compiled `.bl` task logic; the runtime consumes that description instead of assembling a process map.
- Run independent ready tasks concurrently under a shared bound; separate instances may also run concurrently.
- Define cancellation as stopping further token advancement and requesting cancellation of every in-flight task, not merely removing queued work.
- Add a single-binary development server with REST start, status, and cancel endpoints for registered, versioned processes.
- Keep external Rust/Python tasks, loops, distributed workers, messaging, and client interaction outside this change. Preserve existing single-function `.bl` processes.

## Capabilities

### New Capabilities

- `process-runtime`: Single-node process execution, persistence, cancellation, bounded concurrency, and REST control.

### Modified Capabilities

- `business-language`: Add `.bl` task-graph declarations, typed gateway conditions and data flow, validation, and graph code generation without removing existing decision processes.

## Impact

- Extends parser, AST, validation, and Rust generation to produce graph definitions from `.bl` process maps; existing single-function compilation stays supported.
- Adds a Rust runtime and development-server entry point alongside the existing compiler CLI, with a compiled `.bl` graph example.
- Requires local persistence and HTTP/JSON support; does not require a broker, registry, or external service.
