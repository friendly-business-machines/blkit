# Spec Delta

## MODIFIED Requirements

### Requirement: MVP built-in types are fixed
The language SHALL provide the built-in user-facing types `Bool`, `String`, `Number`, `DateTime`, and `List<T>`. `DateTime` SHALL represent a timezone-aware instant; serialized inputs SHALL accept an RFC 3339 timestamp with an explicit offset and reject missing or invalid offsets. Comparisons of `DateTime` SHALL compare instants, not textual representations.

#### Scenario: Number literal typing
- **WHEN** source uses numeric literals such as `1`, `1000`, or `12.50`
- **THEN** validation treats those literals as `Number` values without requiring an integer type

#### Scenario: Typed list literal
- **WHEN** a value of type `List<Number>` is supplied as `[1, 2.5]`
- **THEN** validation accepts both elements as `Number` values

#### Scenario: List element type mismatch
- **WHEN** a value of type `List<Number>` is supplied as `[1, "two"]`
- **THEN** validation fails with a diagnostic identifying the incompatible element

#### Scenario: DateTime input
- **WHEN** a typed `DateTime` field receives `"2026-10-02T09:30:00+02:00"`
- **THEN** validation accepts it and comparison uses the corresponding instant

#### Scenario: Unsupported built-in type
- **WHEN** source uses an unsupported built-in type such as `Table<T>`, `Date`, `Time`, `Range`, `Any`, or `Optional<T>`
- **THEN** validation fails with a diagnostic identifying the unsupported type

### Requirement: Processes declare their task graph in `.bl`
The language SHALL support source-defined typed tasks and process graphs consisting of named start, task, business-rule, intermediate wait, gateway, join, and terminal nodes connected by explicit directed links. The compiler SHALL reject unknown or duplicate nodes, unreachable nodes or terminals, dead-end routes, invalid gateway split/join pairing, and links whose values do not match their targets' declared types. Process graphs SHALL permit backward links, provided that any graph containing a directed cycle declares a positive process deadline; decision requirement graphs SHALL remain acyclic. Backward links SHALL NOT enter a start node or cause unmatched gateway joins, and references used on each route SHALL be typed and available for that activation. Process-level single-body declarations and implicit sequential `run`/`return` syntax SHALL NOT be accepted; task bodies SHALL retain typed `return`.

#### Scenario: Valid source-defined process map
- **WHEN** a `.bl` process explicitly links named tasks and gateways in a finite, type-compatible graph from start to terminal nodes
- **THEN** validation accepts the graph without a separately authored Rust process map

#### Scenario: Invalid graph link
- **WHEN** a process graph references a missing node, strands an activated route, passes an incompatible value, or loops through a join without matching activation
- **THEN** validation fails with a diagnostic identifying the offending node or link

#### Scenario: Bounded cyclic graph
- **WHEN** a process has a backward edge and a declared positive process deadline, with a reachable exit and valid typed routes
- **THEN** validation accepts repeated visits to the same node

#### Scenario: Cycle without deadline
- **WHEN** a process graph has a directed cycle and no process deadline
- **THEN** validation rejects the graph before Rust is emitted

#### Scenario: Existing process stays valid
- **WHEN** an existing `.bl` process is migrated from a single body or implicit `run`/`join`/`return` sequence to named nodes and explicit links
- **THEN** its typed business behavior can still validate and execute; unmigrated process syntax is rejected with a diagnostic

## ADDED Requirements

### Requirement: Business-rule nodes invoke decision models
A `.bl` process SHALL be able to call a source-declared typed decision model through a named business-rule node. Its argument and returned value SHALL be type checked like a task node and made available to downstream expressions and links. An unknown model or incompatible type SHALL be rejected before generation.

#### Scenario: Decision-guided route
- **WHEN** a business-rule node evaluates a discount model and a downstream gateway checks its typed result
- **THEN** the process selects a route using the evaluated decision result

### Requirement: Intermediate waits and process deadlines are source-configurable
A process SHALL support `pause for` with a positive duration literal using the existing retry-duration units and `pause until` with a `DateTime` expression derived from available typed values. The deadline policy SHALL select a positive duration measured either since the instance was queued or since it was first claimed by a worker. The compiler SHALL reject waits with invalid types and policies with invalid or ambiguous origins. The reserved terminal name `timeout` SHALL be unavailable for user-declared nodes. Source declarations use `node hold = pause_for "1m"` or `node hold = pause_until input.closes_at`, and `deadline queued "10m"` or `deadline first_claimed "10m"` before graph nodes.

#### Scenario: Dynamic pause until
- **WHEN** `pause until` refers to a `DateTime` field in the process input
- **THEN** validation accepts it and generates a wait node that uses that instant

#### Scenario: Wrong wait type
- **WHEN** `pause until` receives a `Number` expression
- **THEN** validation rejects the wait before generation

### Requirement: Task iteration is explicit and bounded
A task node SHALL support a conditional loop checked before each iteration or after each iteration; it SHALL declare at least one positive maximum iteration count or elapsed iteration duration. A task node SHALL also support sequential or parallel multi-instance execution over a typed `List<T>`, passing one item per invocation and producing a `List<U>` result in input order. A task-loop condition SHALL be a typed `Bool` expression and all loop-result references SHALL have an unambiguous type and availability. Invalid bounds, mismatched element types, or ambiguous repeated-output references SHALL fail validation. Source syntax is `node repeated = task echo(repeated) repeat_pre(repeated < 3) max_iterations 3 initial input`, `node repeated = task echo(input) repeat_post(repeated < 3) max_duration "1m"`, or `node batch = task echo each input parallel` (also `sequential`); count and duration bounds MAY be combined.

#### Scenario: Post-check loop
- **WHEN** a bounded post-check loop has a condition that is false after the first invocation
- **THEN** the task runs once and its output is available downstream

#### Scenario: Pre-check loop
- **WHEN** a pre-check condition is false before the first invocation
- **THEN** no task invocation occurs and the node yields a declared type-correct no-iteration result

#### Scenario: Sequential and parallel instances
- **WHEN** a list of three items enters a sequential or parallel multi-instance task
- **THEN** the task is invoked once for each item and results are returned in input order
