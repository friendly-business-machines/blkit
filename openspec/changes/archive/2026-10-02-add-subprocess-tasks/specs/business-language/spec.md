# Spec Delta

## ADDED Requirements

### Requirement: Process graphs may call a typed subprocess
A named node `node <name> = subprocess <process>(<input>)` SHALL invoke a process declared in the caller's namespace and version. The call argument SHALL match the child's declared input type; the child's normal output SHALL be the node's output type and be available only on its normal-success route. Standalone transpilation SHALL resolve processes in its single source; project builds SHALL resolve processes across sources in the same scope. Unknown, out-of-scope, or type-incompatible calls SHALL fail before Rust generation. Existing `task` nodes SHALL continue to resolve tasks, not processes.

#### Scenario: Typed successful call
- **WHEN** a caller links `node result = subprocess calculate(input)` to a normal successor using `result`, and `calculate` takes the input type and returns the successor's required type
- **THEN** validation succeeds and the generated graph calls the child's process graph

#### Scenario: Invalid call
- **WHEN** the named process is absent from the same namespace/version, the argument has an incompatible type, or a success-only result is referenced on an exceptional route
- **THEN** validation fails before emitting Rust

### Requirement: Subprocess outcomes have explicit per-kind routes
A subprocess node SHALL have exactly one ordinary link for normal completion and MAY have at most one exceptional outgoing link each of the forms `link <node> -> <target> on error`, `on cancel`, and `on terminate`. These links SHALL carry no child output or value; ordinary links SHALL run only after a normal child `end`. Duplicate or inappropriate `on` links SHALL fail validation. If an outcome has no matching exceptional link, it SHALL propagate to the parent instance; caught child outcomes SHALL allow the parent graph to continue. A child `error` SHALL retain its terminal name on propagation. External cancellation of the parent and expiry of the parent's own deadline SHALL NOT be catchable by these links.

#### Scenario: Catch only modeled error
- **WHEN** a child reaches a named `error` and the parent has an `on error` link
- **THEN** only that handler route activates without a normal child output

#### Scenario: Unhandled child terminal
- **WHEN** a child reaches `terminate` and the parent has no `on terminate` link
- **THEN** the parent instance terminates rather than taking the normal route

#### Scenario: Invalid exceptional routing
- **WHEN** a non-subprocess node declares an `on error` link or a subprocess declares two `on cancel` links
- **THEN** validation rejects the graph

### Requirement: Subprocess call graphs cannot recurse
The compiler SHALL reject direct and indirect cycles between process calls in the same namespace/version while permitting acyclic nested calls, irrespective of process graph deadlines.

#### Scenario: Nested nonrecursive call
- **WHEN** A calls B and B calls C without a call cycle
- **THEN** validation accepts their call graph when their types and routes are valid

#### Scenario: Recursive call cycle
- **WHEN** A calls B and B calls A, or A calls itself
- **THEN** validation rejects the cycle before Rust generation

## MODIFIED Requirements

### Requirement: Process graphs end in named terminal nodes
A process graph SHALL connect via explicit links to named terminal nodes of kind `end`, `error`, `cancel`, or `terminate`. A normal `end` SHALL receive a value of the process's declared output type; a named `error` node SHALL itself identify the business error and SHALL require no payload. `cancel` and `terminate` SHALL require no output. For a top-level process, these terminals apply to the whole instance. For a subprocess, its terminal ends that child scope and either follows a corresponding parent handler route or propagates to the parent; no transaction compensation is implied.

#### Scenario: Normal typed end
- **WHEN** a valid route links a `Decision` value into a named `end` node in a process returning `Decision`
- **THEN** validation accepts the typed output

#### Scenario: Modeled business error
- **WHEN** a route links to a named `error` node
- **THEN** validation accepts the error node without requiring a process output value on that route

#### Scenario: Incompatible normal end
- **WHEN** a route sends `Bool` to an `end` node of a process returning `Decision`
- **THEN** validation rejects the link

#### Scenario: Child terminal is scoped
- **WHEN** a subprocess reaches `cancel` and its caller handles `on cancel`
- **THEN** the child scope ends and the parent can follow its handler route
