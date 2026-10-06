# Spec Delta

## ADDED Requirements

### Requirement: Declarations and statements have explicit delimiters
A `.bl` source SHALL end every statement, including namespace/version, field, variant, port, expression, rule, `flow`, and `bind` statements, with `;`. A declaration or control block SHALL close with `}` rather than indentation or a terminating semicolon. Processes and peer graph nodes SHALL have braced bodies, including empty bodies. Decision-task bodies and their kind-specific decision nodes SHALL be braced. Existing record and enum declaration layouts MAY retain their current colon/indentation structure, but their member statements SHALL end in `;`. Whitespace and newlines alone SHALL NOT terminate statements. A semicolon inside a quoted literal SHALL NOT terminate a statement.

#### Scenario: Braces and semicolons
- **WHEN** a source uses `namespace demo;`, `version "1.0";`, and `process p { flow start -> done; }` with braced peer nodes
- **THEN** the source passes delimiter validation

#### Scenario: Missing delimiter
- **WHEN** a `flow` lacks `;`, a braced declaration lacks `}`, or a declaration uses the old colon/indentation form where braces are required
- **THEN** compilation fails with a syntax diagnostic and emits no Rust

### Requirement: Process flow and data bindings are separate
A process body SHALL reference named peer nodes using `flow <source> -> <target>;` for execution order and `bind <source>.<output> -> <target>.<input>;` for data transfer. A `flow` SHALL NOT implicitly pass data; a `bind` SHALL NOT imply execution order. Each bound input used on an activated route SHALL have a compatible source output available before its target executes. Unknown nodes/ports, incompatible types, unavailable outputs, missing required inputs, and ambiguous bindings SHALL fail validation. Nodes SHALL NOT be redefined in a process body. A source node with exactly one output MAY be named without `.output` in a value reference; a multi-output source SHALL use a qualified output name.

#### Scenario: Distinct edges
- **WHEN** a process contains `flow start -> calculate;`, `flow calculate -> done;`, `bind start.my_input_number -> calculate.amount;`, and `bind calculate.result -> done.result;` with valid peers
- **THEN** the task runs after `start`, and the bound values reach the matching typed inputs

#### Scenario: Data does not schedule tasks
- **WHEN** a process binds an output to a task input but provides no executable control-flow path to that task
- **THEN** validation rejects the graph rather than treating the binding as a flow edge

#### Scenario: Missing or mistyped input
- **WHEN** a decision-task input has no binding on an activated route or a `Number` output is bound to a `String` input
- **THEN** validation fails before generating Rust

## MODIFIED Requirements

### Requirement: Source-defined tasks and decisions are shared within a project scope
A project build SHALL resolve peer `start_event`, `end_event`, `decision_task`, process, and domain-type declarations across `.bl` files with matching namespace and process version before validating process graphs, independently of source-file order. Declarations from another namespace or version SHALL NOT become visible implicitly; duplicate peer names within a scope SHALL be rejected. Standalone single-file compilation SHALL resolve peers from that file alone. Generic `task` and `decision` declarations SHALL NOT be accepted.

#### Scenario: Cross-file task call
- **WHEN** one file defines a typed `decision_task` and another file in the same scope has a process whose `flow` and `bind` reference it
- **THEN** validation accepts the references irrespective of source-file order

#### Scenario: Cross-file decision call
- **WHEN** a process references a decision task in another file with matching namespace and version
- **THEN** validation accepts and emits its decision evaluation

#### Scenario: Duplicate or out-of-scope declaration
- **WHEN** two files in one scope define the same peer name, or a process references a decision task only defined in another namespace or version
- **THEN** project validation rejects the duplicate or unknown reference before emitting Rust

### Requirement: Processes have typed input and output
A process SHALL be declared as `process <name> { ... }` without parenthesized parameters or an arrow output type. It SHALL have exactly one reachable `start_event` in this change. Its incoming JSON value SHALL be a JSON object with keys and values matching that start event's outputs; every declared start output SHALL be present, with no unknown keys. A normal result SHALL come from a referenced `end_event` whose declared input ports are bound to compatible outputs. All reachable normal end events of the same process SHALL declare the same result shape and types. One normal end input SHALL produce that input's value directly; multiple inputs SHALL produce a JSON object keyed by port names. Exceptional terminal routes SHALL NOT require normal end inputs.

#### Scenario: Valid process signature
- **WHEN** `start_event start { output amount: Number; }`, `end_event done { input result: Number; }`, and a process connect the ports through a decision task
- **THEN** start input `{"amount": 3}` validates and a normal `done` produces its bound numeric value

#### Scenario: Invalid process signature type
- **WHEN** start input is a scalar, lacks a declared port, contains an unknown key, or has an incompatible value type
- **THEN** validation rejects the start request before an instance begins

#### Scenario: Exceptional route
- **WHEN** a route reaches a named error, cancel, or terminate event rather than a normal end event
- **THEN** that outcome does not require an end-event result value

#### Scenario: Ambiguous process shape
- **WHEN** a process has two reachable start events or two normal end events with incompatible input-port shapes
- **THEN** validation fails before generation

### Requirement: Processes declare their task graph in `.bl`
The language SHALL support process graphs assembled from named peer start, decision-task, gateway, join, wait, subprocess, and terminal nodes connected by explicit `flow` and typed `bind` statements. Graph nodes SHALL use their specific kind as the declaration keyword and enclose their properties in `{ ... }`; only `decision_task` SHALL be an authored task kind in this change. The compiler SHALL reject unknown or duplicate nodes, unreachable nodes or terminals, dead-end routes, invalid gateway split/join pairing, and unavailable/mistyped bindings. Cycles SHALL require a positive process deadline; backward flow SHALL NOT enter a start event or cause unmatched joins. An old `node <name> = <kind>` declaration, `link`, generic `task`, business-rule call, process signature, implicit sequencing, or process-level `return` SHALL NOT be accepted.

#### Scenario: Valid source-defined process map
- **WHEN** a process connects a start event, a decision task, and an end event with typed flow and bindings
- **THEN** validation accepts the graph without redeclaring any of those nodes inside the process

#### Scenario: Invalid graph link
- **WHEN** flow references a missing peer, strands a route, or bind references an unavailable output
- **THEN** validation fails before emitting Rust

#### Scenario: Bounded cyclic graph
- **WHEN** a graph contains a reachable backward flow, a valid exit, and a positive process deadline
- **THEN** repeated node visits are accepted

#### Scenario: Cycle without deadline
- **WHEN** a graph has a directed cycle without a positive process deadline
- **THEN** validation rejects it before Rust generation

#### Scenario: Existing process stays valid
- **WHEN** an existing process is migrated to braced peers, typed `flow` and `bind`, and supported decision-task nodes
- **THEN** its business behavior can still validate and execute, while its unmigrated legacy syntax is rejected

### Requirement: Gateway conditions and joins are typed
The language SHALL support named, explicitly connected AND, OR, and XOR split and join gateways in a `.bl` process map. Each gateway SHALL use a kind-specific braced declaration; `flow` statements SHALL carry typed branch conditions, ordered fallbacks, and AND branch labels as applicable; typed `bind` statements SHALL carry required values separately. AND splits SHALL activate all outgoing branches; XOR splits SHALL activate the first matching branch in outgoing-flow declaration order; OR splits SHALL activate every matching branch. Conditions SHALL be `Bool` expressions that can reference start-event outputs and upstream task outputs available on every route to that gateway. XOR and OR splits SHALL provide a fallback when no condition matches. Joins SHALL account for the branches activated for that instance: AND waits for all incoming branches and combines their results into a declared record, XOR accepts the selected branch's result of a common type, and OR waits for every selected branch and produces a `List<T>` of compatible branch results in split-flow declaration order. Every route to a normal end event SHALL bind compatible values to its required input ports.

#### Scenario: Conditions use task output and input
- **WHEN** an XOR gateway's `flow` condition uses a completed decision-task output and a start-event output
- **THEN** validation accepts its `Bool` condition and selects exactly one branch

#### Scenario: Inclusive parallel routing
- **WHEN** two OR conditions match
- **THEN** both branches activate and the OR join waits for both, but not for inactive branches

#### Scenario: Invalid reference or route
- **WHEN** a condition uses an unavailable output or a normal end-event input cannot be bound on a reachable route
- **THEN** validation fails before generation

#### Scenario: Multiple XOR conditions match
- **WHEN** multiple XOR conditions match
- **THEN** the first matching outgoing flow in declaration order is selected

#### Scenario: Inclusive join output
- **WHEN** an OR gateway selects two branches of the same declared output type
- **THEN** its join produces a typed list in split-flow declaration order

### Requirement: Process graphs may call a typed subprocess
A peer subprocess node SHALL invoke a process in the caller's namespace and version. It SHALL declare typed input and normal-output ports and a braced body; its incoming values SHALL be supplied by `bind`, not by positional call arguments. A normal child result SHALL be available only on its success route, and its port bindings SHALL be type checked. Standalone compilation SHALL resolve its child from the file; project builds SHALL resolve processes across the same scope. Unknown, out-of-scope, incompatible, or recursive process calls SHALL fail before Rust generation.

#### Scenario: Typed successful call
- **WHEN** a parent flows through a subprocess peer, binds its input from a compatible output, and binds its normal result to a successor
- **THEN** validation succeeds and the generated graph executes the child

#### Scenario: Invalid call
- **WHEN** the child is missing, a bound input is incompatible, or a success-only output is used on an exceptional route
- **THEN** validation fails before emitting Rust

### Requirement: Subprocess outcomes have explicit per-kind routes
A subprocess node SHALL have exactly one ordinary success `flow` and MAY have at most one exceptional outgoing `flow` each with `on error`, `on cancel`, or `on terminate`. Exceptional flows SHALL carry no child output; ordinary flows SHALL run only after a normal child end event. Duplicate or inappropriate outcome flows SHALL fail validation. Unhandled child outcomes SHALL propagate to the parent; caught child outcomes SHALL permit parent continuation. A child error SHALL retain its terminal name when propagated. External parent cancellation and parent deadline expiry SHALL NOT be catchable by child outcome flows.

#### Scenario: Catch only modeled error
- **WHEN** a child reaches a named error event and its parent has `flow called -> handler on error;`
- **THEN** only that handler route activates without a normal child output

#### Scenario: Unhandled child terminal
- **WHEN** a child reaches terminate and its parent has no `on terminate` flow
- **THEN** the parent terminates rather than taking its normal route

#### Scenario: Invalid exceptional routing
- **WHEN** a non-subprocess node declares `on error` or a subprocess declares two `on cancel` flows
- **THEN** validation rejects the graph

### Requirement: Process graphs end in named terminal nodes
A process graph SHALL connect through `flow` to named, braced terminal-event peers of kind `end_event`, `error_event`, `cancel_event`, or `terminate_event`. A normal end event SHALL require bound, typed input ports; a named error event SHALL identify the business error and require no normal output; cancel and terminate events SHALL require no normal output. Top-level terminals SHALL apply to the instance; child terminals SHALL end only the child scope before handling or propagation. No compensation is implied.

#### Scenario: Normal typed end
- **WHEN** a reachable `end_event done { input result: Number; }` receives a compatible bound result
- **THEN** validation accepts the output

#### Scenario: Modeled business error
- **WHEN** a route flows to a named `error_event` peer
- **THEN** validation accepts it without a normal result on that route

#### Scenario: Incompatible normal end
- **WHEN** a `Bool` output is bound to a `Number` end-event input
- **THEN** validation rejects the binding

#### Scenario: Child terminal is scoped
- **WHEN** a subprocess reaches `cancel_event` and its caller handles `on cancel`
- **THEN** the child scope ends and the parent follows its handler

### Requirement: Intermediate waits and process deadlines are source-configurable
A process SHALL support pause-for with a positive duration literal and pause-until with a `DateTime` value supplied by a typed binding. Waits SHALL use kind-specific peer declarations with braced bodies, and the process deadline policy SHALL be a semicolon-terminated statement in its braced process body selecting a positive duration since queued or first claimed. The compiler SHALL reject invalid duration/type/origin; `timeout` remains reserved as a terminal name. Waits SHALL retain their existing durable runtime behavior.

#### Scenario: Dynamic pause until
- **WHEN** a pause-until peer receives a `DateTime` input through `bind`
- **THEN** validation accepts the wait

#### Scenario: Wrong wait type
- **WHEN** a pause-until peer receives a `Number` binding
- **THEN** validation rejects it before generation

### Requirement: Task iteration is explicit and bounded
A `decision_task` used in a process SHALL support a conditional loop checked before or after each invocation and SHALL declare at least one positive maximum iteration count or elapsed duration. It SHALL support sequential or parallel multi-instance execution over a typed `List<T>`, passing each item through the named input port and yielding ordered typed results. Conditions SHALL be `Bool` expressions with unambiguous availability; typed `bind` statements SHALL supply inputs and initial results where needed. Invalid bounds, element types, or repeated-output references SHALL fail validation. Old generic `node ... = task ... repeat_*` syntax SHALL be rejected.

#### Scenario: Post-check loop
- **WHEN** a bounded post-check decision task's condition is false after its first invocation
- **THEN** it runs once and exposes its output downstream

#### Scenario: Pre-check loop
- **WHEN** a pre-check condition is initially false with a type-correct initial result
- **THEN** the task is not invoked and the initial result is available downstream

#### Scenario: Sequential and parallel instances
- **WHEN** a three-element list enters a sequential or parallel decision task
- **THEN** it runs once per item and returns outputs in input order

### Requirement: Fallible string expressions report execution errors through generated calls
Named, typed `.bl` output port declarations SHALL continue to describe their normal types. Generated Rust calls containing runtime-fallible string expressions MAY return `Result<Output, String>` instead of plain `Output`; callers SHALL receive an execution error for invalid runtime string operations instead of a silent fallback or process panic. Valid supported `.bl` programs without fallible expressions SHALL retain their evaluation behavior.

#### Scenario: Dynamic invalid regex in a graph
- **WHEN** a decision task evaluates `matches(input, pattern)` with an invalid runtime `pattern`
- **THEN** its execution reports an error rather than returning `false`, `[]`, or crashing the process

## REMOVED Requirements

### Requirement: Process graphs may invoke crate-qualified custom task nodes
**Reason**: No non-decision task kind is supported in the new authored syntax.
**Migration**: Replace eligible logic with a `decision_task` or defer crate-backed process steps until a specific task kind is introduced. Existing crate-backed `task` calls SHALL fail source validation.

### Requirement: Process bodies support minimal deterministic control flow
**Reason**: The generic expression-task body is removed; expression logic belongs to kind-specific decision nodes in a `decision_task`.
**Migration**: Rewrite applicable business expressions as `literal_expression` nodes and expose typed decision-task outputs; generic `task` declarations and process-level `return` SHALL fail validation.

### Requirement: Business-rule nodes invoke decision models
**Reason**: `decision_task` peers now encapsulate the decision and participate directly in process flow.
**Migration**: Convert `decision` declarations to `decision_task`, `node ... = business_rule ...` to references through process `flow`, and positional values to typed `bind` statements.
