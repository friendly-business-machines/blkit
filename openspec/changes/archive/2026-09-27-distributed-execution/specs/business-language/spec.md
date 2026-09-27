# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
The language SHALL support source-defined typed tasks and process graphs consisting of named start, task, gateway, join, and terminal nodes connected by explicit directed links. The compiler SHALL reject unknown or duplicate nodes, cycles, unreachable nodes or terminals, dead-end routes, invalid gateway split/join pairing, and links whose values do not match their targets' declared types. Process-level single-body declarations and implicit sequential `run`/`return` syntax SHALL NOT be accepted; task bodies SHALL retain typed `return`.

#### Scenario: Valid source-defined process map
- **WHEN** a `.bl` process explicitly links named tasks and gateways in a finite, type-compatible graph from start to terminal nodes
- **THEN** validation accepts the graph without a separately authored Rust process map

#### Scenario: Invalid graph link
- **WHEN** a process graph references a missing node, forms a cycle, strands an activated route, or passes an incompatible value
- **THEN** validation fails with a diagnostic identifying the offending node or link

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
