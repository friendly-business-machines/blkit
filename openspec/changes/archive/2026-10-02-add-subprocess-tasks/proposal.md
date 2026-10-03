# Proposal

## Why

A `.bl` graph cannot reuse another process as a node; authors must duplicate its graph or move business behavior into a task, losing the child's process-level routing, retry, and deadline semantics. Allowing a process to call another within one instance makes reusable workflows possible while preserving typed control flow and durable recovery.

## What Changes

- Add a typed `subprocess` node that calls a process in the same namespace/version, including a process declared in another project file. Reject missing, mismatched, cross-scope, and recursive process references.
- Add per-outcome links for child `error`, `cancel`, and `terminate`; unhandled child outcomes propagate to the parent. A normal child `end` supplies the subprocess node's typed output.
- Execute child graphs as part of the parent instance, with durable nested checkpoints and child-scoped retry/deadline behavior. Parent cancellation and deadline remain instance-wide and uncatchable.
- Preserve existing task/process syntax and standalone single-file compilation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Subprocess node syntax, static resolution/type checking, terminal routing, and recursion validation.
- `process-runtime`: Nested graph execution, checkpointing, recovery, outcome handling, and policy/cancellation precedence.
- `project-build`: Same-scope cross-file process references in discovered sources.

## Impact

Compiler parser, semantic analysis, generated graph definitions, local/distributed graph execution and checkpoint storage, project build validation, language/runtime/project tests, and README examples. No new dependencies or external REST endpoints.
