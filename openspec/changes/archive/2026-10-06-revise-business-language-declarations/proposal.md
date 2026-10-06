# Proposal

## Why

The current `.bl` syntax mixes indentation, untyped-looking `node ... = ...` declarations, and arguments embedded in control-flow links. The language should expose BPMN-like event/task kinds and typed data connections separately from execution order.

## What Changes

- **BREAKING**: Require `{ ... }` for process, event/task, and decision-node bodies, and `;` at the end of statements; reject the prior indentation/colon and unterminated-statement forms. Blocks end with `}`, not `};`.
- **BREAKING**: Replace generic `task`, `decision`, `node ... = ...`, `business_rule`, and `link` authoring forms with peer declarations for `start_event`, `end_event`, and `decision_task`, kind-specific decision nodes (`decision_table`, `literal_expression`, `context`), and process `flow` and `bind` statements. Process bodies reference peer nodes; they do not redeclare tasks.
- **BREAKING**: Process inputs are declared as start-event outputs and normal results as end-event inputs; decision-task inputs and outputs are named and typed in its body rather than positional call arguments. Typed `bind` statements move values, separate from control-flow `flow` statements. Decision dependencies are inferred from node references, not declared as edges. A sole output can be referenced by its node name; otherwise use `node.output`.
- **BREAKING**: Only `decision_task` is supported as an authored task kind for now. Old expression-based tasks and crate-backed task calls are rejected, not accepted as legacy syntax. Keep existing decision-table evaluation and expression behavior where expressible in the new syntax; migrate repository examples and docs.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `business-language`: Replace process/task/event declaration syntax, process signatures, graph nodes, flow/value wiring, and obsolete task-call syntax.
- `decision-models`: Move decision graphs into `decision_task` peers with braced, kind-specific nodes and implicit typed dependencies.
- `project-build`: Resolve event/task peers across source files and clarify the breaking impact on old task and extension declarations.
- `process-runtime`: Define JSON start input and normal end output shapes for named event ports.

## Impact

The parser (`src/compiler.rs`, `src/graph.rs`, `src/decision.rs`, `src/expr.rs`), semantic checks and Rust generation (`src/semantic/`, `src/codegen/`, `src/project.rs`), tests, `.bl` examples, and README require updates. Runtime process identity, scheduling, checkpointing, and decision-table policy semantics should remain unchanged. Runtime start requests become JSON objects keyed by start-event output ports; a single normal end-event input returns its value directly, while multiple end-event inputs return an object keyed by port. Existing `.bl` sources and clients must migrate before transpilation/execution.
