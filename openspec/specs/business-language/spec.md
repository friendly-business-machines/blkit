# business-language Specification

## Purpose

Defines the initial `.bl` language capability for writing closed-world, type-safe business processes that can be validated and transpiled to Rust.

## Requirements

### Requirement: Source files declare namespace and version
A `.bl` source file SHALL declare a namespace and version before process declarations so generated processes can be identified for change management.

#### Scenario: Valid namespace and version
- **WHEN** a source file declares `namespace orders` and `version "1.0"`
- **THEN** the language front end accepts those declarations as the process identity context

#### Scenario: Missing namespace or version
- **WHEN** a source file omits either the namespace or the version declaration
- **THEN** validation fails with a diagnostic identifying the missing declaration

### Requirement: Domain types are declared in source
The language SHALL support business-domain record declarations, enum declarations, and typed list values using only `.bl` source-defined domain types and built-in types.

#### Scenario: Valid domain declarations
- **WHEN** a source declares a record type, an enum type, and a `List<T>` field
- **THEN** validation accepts the declarations when all referenced types exist

#### Scenario: Unknown type reference
- **WHEN** a declaration references a type that is neither built in nor declared in the input `.bl` file
- **THEN** validation fails with a diagnostic identifying the unknown type

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

### Requirement: Processes have typed input and output
A process declaration SHALL define a name, exactly one typed input parameter, and one output type for normal `end` nodes. Routes ending at `error`, `cancel`, or `terminate` SHALL NOT need to produce that normal output type.

#### Scenario: Valid process signature
- **WHEN** a process is declared with `process approve_order(input: Order) -> Decision`
- **THEN** validation accepts the signature when `Order` and `Decision` are valid types

#### Scenario: Invalid process signature type
- **WHEN** a process signature references an unknown input or output type
- **THEN** validation fails with a diagnostic identifying the invalid type reference

#### Scenario: Exceptional route
- **WHEN** one route supplies `Decision` to an `end` node and another route reaches an `error` node
- **THEN** validation accepts both routes without treating the error as a `Decision`

### Requirement: Process bodies support minimal deterministic control flow
Task bodies SHALL support field access, enum variant references, literals, comparisons, boolean operations, `if`/`else` branching, and `return` statements sufficient to express deterministic business decisions. A process SHALL express control flow through its named graph nodes and links rather than a single-body `return` statement.

#### Scenario: Branching process returns expected enum type
- **WHEN** a process graph links to a task returning `Decision` that returns `Decision.review` in one branch and `Decision.approved` in another branch
- **THEN** validation accepts the task body and process graph

#### Scenario: Return type mismatch
- **WHEN** a task returns a value that does not match its declared task output type
- **THEN** validation fails with a diagnostic identifying the return type mismatch

#### Scenario: Process-level return is not a terminal event
- **WHEN** a process body ends with `return value` rather than linking to an `end` node
- **THEN** validation rejects the process and identifies the required explicit terminal node

### Requirement: Rust generation follows successful validation
The compiler SHALL generate Rust output only after parsing, name validation, graph validation, and type validation have succeeded.

#### Scenario: Valid source generates Rust
- **WHEN** an input `.bl` file passes validation
- **THEN** the compiler produces Rust code representing the declared types, lists, tasks, process graphs, and retry policies

#### Scenario: Invalid source does not generate Rust
- **WHEN** an input `.bl` file fails parsing or validation
- **THEN** the compiler produces diagnostics and does not emit Rust code for that file

#### Scenario: Generated behavior is callable
- **WHEN** valid `.bl` declares a graph that branches on a numeric field and reaches a typed normal end node
- **THEN** the generated Rust compiles and the registered process can be invoked to return the expected result for each branch

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

### Requirement: Gateway conditions and joins are typed
The language SHALL support named, explicitly linked AND, OR, and XOR split and join gateways in a `.bl` process map. AND splits SHALL activate all outgoing branches; XOR splits SHALL activate the first matching branch in outgoing-link declaration order; OR splits SHALL activate every matching branch. Conditions SHALL be `Bool` expressions that can reference process input and upstream task outputs available on every route to that gateway. XOR and OR splits SHALL provide a fallback when no condition matches. Joins SHALL account for the branches activated for that instance: AND waits for all incoming branches and combines their results into a declared record, XOR accepts the selected branch's result of a common type, and OR waits for every selected branch and produces a `List<T>` of compatible branch results in split-link declaration order. Every route to a normal `end` SHALL produce its declared output type.

#### Scenario: Conditions use task output and input
- **WHEN** an XOR gateway uses a completed task's output and the process input to choose between two links
- **THEN** validation accepts its `Bool` conditions and exactly one branch is selected at execution

#### Scenario: Inclusive parallel routing
- **WHEN** two OR conditions match
- **THEN** both branches become active and the OR join waits for both, but not for inactive branches

#### Scenario: Invalid reference or route
- **WHEN** a condition references a task result unavailable on that route or a branch cannot produce the declared normal-end output type
- **THEN** validation fails before code is generated

#### Scenario: Multiple XOR conditions match
- **WHEN** an XOR gateway has more than one true condition
- **THEN** the first matching outgoing link in source order is selected

#### Scenario: Inclusive join output
- **WHEN** an OR gateway selects two branches returning values of the same declared type
- **THEN** its join produces a typed list of those values in split-link declaration order

### Requirement: Compiler emits the validated process graph
After successful validation, the compiler SHALL emit Rust representing `.bl`-defined tasks, named nodes, explicit typed links, gateway routes, terminal events, retry policy, and process identity for runtime registration. The runtime SHALL NOT need to reconstruct or infer a process map from individual compiled functions. The generated business logic SHALL be linked into a binary at build time rather than compiled by a running worker.

#### Scenario: Compiled graph is executable
- **WHEN** a valid `.bl` graph with a gateway, at least two task nodes, and a named terminal is compiled into a worker binary
- **THEN** the runtime can register that generated graph and execute the intended route

#### Scenario: Invalid graph produces no Rust
- **WHEN** graph validation fails
- **THEN** the compiler emits diagnostics rather than Rust output for that file

#### Scenario: Worker advertises compiled identities
- **WHEN** a worker starts with generated process definitions linked into its binary
- **THEN** it can advertise those definitions' namespace, version, and process names without compiling `.bl` at runtime

### Requirement: Processes declare bounded retry policy in source
A `.bl` process SHALL optionally declare `max retries` (additional attempts after the initial execution), `retry for` (a duration measured from the first execution failure), `retry delay` (minimum wait before the first retry), and exponential backoff. Absence of a retry declaration SHALL mean no retries. Both the attempt limit and time window SHALL bound retries; later delays SHALL grow exponentially from the declared minimum. The compiler SHALL reject invalid or unbounded declarations.

#### Scenario: Bounded policy is compiled
- **WHEN** a process declares three additional retries, a ten-minute retry window, a one-second minimum delay, and exponential backoff
- **THEN** validation accepts the policy and the compiled process definition carries all four parameters

#### Scenario: No retry declaration
- **WHEN** a process declares no retry policy
- **THEN** its compiled definition permits no additional attempts

#### Scenario: Invalid policy
- **WHEN** a retry declaration supplies a negative limit, nonpositive duration, or missing required parameter
- **THEN** validation fails before Rust is emitted

### Requirement: Process graphs end in named terminal nodes
A process graph SHALL connect via explicit links to named terminal nodes of kind `end`, `error`, `cancel`, or `terminate`. A normal `end` SHALL receive a value of the process's declared output type; a named `error` node SHALL itself identify the business error and SHALL require no payload. `cancel` and `terminate` SHALL require no output. These terminal events apply to the whole instance without boundary catches, subprocess scope, or transaction compensation.

#### Scenario: Normal typed end
- **WHEN** a valid route links a `Decision` value into a named `end` node in a process returning `Decision`
- **THEN** validation accepts the typed output

#### Scenario: Modeled business error
- **WHEN** a route links to a named `error` node
- **THEN** validation accepts the error node without requiring a process output value on that route

#### Scenario: Incompatible normal end
- **WHEN** a route sends `Bool` to an `end` node of a process returning `Decision`
- **THEN** validation rejects the link

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
