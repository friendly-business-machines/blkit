# Design

## Context

See [proposal.md](proposal.md). `src/compiler.rs` currently identifies indentation-delimited declarations, `src/graph.rs` parses `node`/`link` with embedded arguments and values, `src/decision.rs` parses explicit decision `link`s, and `src/expr.rs` parses task bodies. `src/semantic/` and `src/codegen/` assume one typed process input and one result. `src/project.rs` merges declarations by namespace/version. Generated graphs and runtimes already execute typed links and decision logic; retain those execution contracts wherever possible.

## Goals / Non-Goals

**Goals:** One coherent, testable source grammar, typed port/flow separation, and predictable migration errors for the deliberately unsupported generic/extension task sources. Preserve current decision-table policies, graph routing/retry semantics, and process identity.

**Non-Goals:** Add new task kinds, legacy syntax aliases, a general BPMN/DMN interchange format, or a new dependency/parser library. Do not change the runtime's storage schema merely to parse new syntax.

## Decisions

### 1. Peer declarations and graph ownership

At namespace/version scope, declare `start_event`, `end_event`, `decision_task`, other existing non-task node kinds (`error_event`, `cancel_event`, `terminate_event`, split/join gateways, waits, subprocess), `process`, records, and enums as named peers. A process owns its `flow`/`bind` statements and resolves referenced peers in its declaration scope; it does not create a second task declaration or pass arguments in a call. Exactly one reachable start event is allowed per process in this change. The same peer can be referenced from multiple processes, with type/binding validation performed for each process independently. Require matching normal-end port shape across a process's reachable normal end events.

Example of the core form (other graph kinds keep their existing behavior with kind-specific braced property declarations):

```bl
namespace demo;
version "1.0";

start_event start {
  output my_input_number: Number;
}

decision_task calculate {
  input amount: Number;
  output result: Number = compute.result;
  literal_expression compute {
    output result: Number;
    expression amount;
  }
}

end_event done {
  input result: Number;
}

process example {
  flow start -> calculate;
  flow calculate -> done;
  bind start.my_input_number -> calculate.amount;
  bind calculate.result -> done.result;
}
```

For a one-output node, `= compute;` is equivalent to `= compute.result;`; do not use bare nodes for multi-output references. The process input is `{"my_input_number": 3}`, not `3`. A one-port end event returns a direct JSON value; a multi-port end event returns a JSON object. Alternative rejected: a separate in-process `node` instantiation; it duplicates peer identity and reintroduces the old abstraction.

### 2. Two process edge types; inferred decision edges

Parse `flow source -> target [when <Bool>|else|as <label>|on <outcome>];` as ordered control edges and `bind source.output -> target.input;` as typed data edges. `flow` never transports values and `bind` never schedules execution. For a task activation, collect required port values from bindings available on the active route; reject missing, ambiguous, mistyped, or unavailable sources before generation. Preserve gateway ordering, branch labels, retries, subprocess handlers, and wait behavior by lowering validated flow and binding pairs to the existing compiled graph input/value closures, not by adding a second runtime graph interpreter. Gateway/join/wait/subprocess declarations have their kind-specific braced properties and typed ports; e.g. `pause_for hold { duration "1m"; }` and `xor_join chosen { split route; output result: Number; }`. Adapt existing loop/multi-instance properties to semicolon-terminated process statements targeting a named `decision_task` (`repeat ...;`, `multi_instance ...;`) rather than preserving old `node ... = task ...` syntax.

Inside `decision_task`, references to declared task inputs or previous node outputs in `expression`, `rule`, `entry`, knowledge expressions, and `output ... = node.port;` imply directed dependencies. Collect these references, resolve types, topologically validate the graph, and reject cycles; no explicit `link`/`dependency` statement. Keep existing table policy/evaluation implementations. A table's output columns still determine its scalar, record, or list result; expose that evaluated result through a typed node output port rather than treating every column as a separate node result. Alternative rejected: explicit decision dependency edges, because they duplicate the dependencies in expressions.

### 3. Parse structurally, then reuse semantic and codegen paths

Scan declaration bodies using balanced braces and semicolon boundaries while respecting quoted strings, nested expression parentheses, ranges/lists, and comments. Preserve the current record/enum colon blocks but require semicolons for member statements. Require braces for every process, event/task, and decision-node body, including empty bodies; `}` closes a block without an extra semicolon. Use existing expression parsing for statement payloads; don't make line breaks or indentation semantic in the new blocks. Fail fast with a diagnostic for missing delimiters and old `task`, `decision`, `node =`, `link`, and positional call syntax. Update ASTs from positional input/output to named ports and from combined link arguments to separate flow/binding data. Reuse current validation of reachability, gateway pairing, temporal/range expressions, decision policies, and Rust generation after lowering into typed graph structures.

Process codegen builds typed input decoding from start-event ports and a normal output mapper from end-event ports. Typed bindings provide task inputs and values for gateway/join/terminal closures; do not rely on task bodies accessing process-local `start.*`. Keep runtime checkpoint and scheduling logic as-is unless a concrete typed-port test demonstrates an adapter is required. Alternative rejected: a second grammar implementation or a runtime data-graph engine; both increase scope without adding user-visible behavior.

## Risks / Trade-offs

- [Existing examples/tests predominantly use old syntax] → Migrate representative examples and integration fixtures together with parser changes; assert old syntax is rejected.
- [Independent `bind` and `flow` can create values unavailable on a route] → Validate bindings per activation/route before emitting Rust, including conditional branches, joins, and subprocess outcomes.
- [Named event input changes public API request shape] → Document and test object-keyed start requests and single/multi-port end results across direct and project builds.
- [Non-decision task programs stop compiling] → Provide explicit syntax diagnostics and migration notes; do not pretend crate-backed tasks remain authorable.
- [Other process node kinds and loop syntax need larger migrations than the simple example] → Retain their existing control-flow semantics through kind-specific braced peers and focused tests; do not add unrelated task kinds to close the gap.

## Migration Plan

1. Implement the grammar and typed AST with failing acceptance/rejection tests for the core event/decision/process example and both semicolon/brace errors.
2. Map decision dependencies, process flow, and bindings into semantic validation/codegen; exercise routing, typed results, and old-syntax rejection before migrating existing fixtures.
3. Update repository `.bl` examples, docs, single-file and project tests; document breaking changes and JSON start input migration.
4. Rollback, if necessary, is a coordinated compiler/source rollback: the old and new `.bl` grammars are intentionally not simultaneously supported. Previously persisted runtime instances remain governed by their linked compiled graph definitions.
