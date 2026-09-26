# Spec Delta

## ADDED Requirements

### Requirement: Processes declare their task graph in `.bl`
The language SHALL support `.bl` declarations for typed business-logic tasks and for a process map that names and links those tasks and gateways. The compiler SHALL reject unknown nodes, cyclic links, unreachable outputs, or task connections whose values do not match the declared input and output types. Existing single-body process declarations SHALL remain valid.

#### Scenario: Valid source-defined process map
- **WHEN** a `.bl` process links source-defined tasks in a finite graph with type-compatible inputs and outputs
- **THEN** validation accepts the graph as the process definition without a separate Rust-authored process map

#### Scenario: Invalid graph link
- **WHEN** a process map references a missing task, forms a cycle, or passes an incompatible task output to another task
- **THEN** validation fails with a diagnostic identifying the offending link

#### Scenario: Existing process stays valid
- **WHEN** an existing `.bl` process consists of one typed body with `if`/`else` and `return`
- **THEN** it still validates and generates callable Rust behavior

### Requirement: Gateway conditions and joins are typed
The language SHALL support AND, OR, and XOR split and join gateways in a `.bl` process map. AND splits SHALL activate all outgoing branches; XOR splits SHALL activate the first matching branch in source order; OR splits SHALL activate every matching branch. Conditions SHALL be `Bool` expressions that can reference process input and upstream task outputs available on every route to that gateway. XOR and OR splits SHALL provide a fallback when no condition matches. Joins SHALL account for the branches activated for that instance: AND waits for all incoming branches and combines their results into a declared record, XOR accepts the selected branch's result of a common type, and OR waits for every selected branch and produces a `List<T>` of compatible branch results in declaration order. Every route to a process output SHALL produce its declared type.

#### Scenario: Conditions use task output and input
- **WHEN** an XOR gateway uses a completed task's output and the process input to choose between two branches
- **THEN** validation accepts its `Bool` conditions and exactly one branch is selected at execution

#### Scenario: Inclusive parallel routing
- **WHEN** two OR conditions match
- **THEN** both branches become active and the OR join waits for both, but not for inactive branches

#### Scenario: Invalid reference or route
- **WHEN** a condition references a task result unavailable on that route or a branch cannot produce the declared process output type
- **THEN** validation fails before code is generated

#### Scenario: Multiple XOR conditions match
- **WHEN** an XOR gateway has more than one true condition
- **THEN** the first matching branch in source order is selected

#### Scenario: Inclusive join output
- **WHEN** an OR gateway selects two branches returning values of the same declared type
- **THEN** its join produces a typed list of those values in branch-declaration order

### Requirement: Compiler emits the validated process graph
After successful validation, the compiler SHALL emit Rust that represents the `.bl`-defined tasks, their typed connections, gateway routes, and process identity for runtime registration. The runtime SHALL NOT need to reconstruct or infer a process map from individual compiled functions.

#### Scenario: Compiled graph is executable
- **WHEN** a valid `.bl` graph with a gateway and at least two task nodes is compiled into a server binary
- **THEN** the runtime can register that generated graph and execute the intended route

#### Scenario: Invalid graph produces no Rust
- **WHEN** graph validation fails
- **THEN** the compiler emits diagnostics rather than Rust output for that file
