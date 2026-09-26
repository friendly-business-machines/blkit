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
The MVP type system SHALL provide exactly these built-in user-facing types: `Bool`, `String`, `Number`, and `List<T>`.

#### Scenario: Number literal typing
- **WHEN** source uses numeric literals such as `1`, `1000`, or `12.50`
- **THEN** validation treats those literals as `Number` values without requiring an integer type

#### Scenario: Typed list literal
- **WHEN** a value of type `List<Number>` is supplied as `[1, 2.5]`
- **THEN** validation accepts both elements as `Number` values

#### Scenario: List element type mismatch
- **WHEN** a value of type `List<Number>` is supplied as `[1, "two"]`
- **THEN** validation fails with a diagnostic identifying the incompatible element

#### Scenario: Unsupported built-in type
- **WHEN** source uses an unsupported built-in type such as `Table<T>`, `Date`, `Time`, `DateTime`, `Range`, `Any`, or `Optional<T>`
- **THEN** validation fails with a diagnostic identifying the unsupported type

### Requirement: Processes have typed input and output
A process declaration SHALL define a name, exactly one typed input parameter, and one output type.

#### Scenario: Valid process signature
- **WHEN** a process is declared with `process approve_order(input: Order) -> Decision`
- **THEN** validation accepts the signature when `Order` and `Decision` are valid types

#### Scenario: Invalid process signature type
- **WHEN** a process signature references an unknown input or output type
- **THEN** validation fails with a diagnostic identifying the invalid type reference

### Requirement: Process bodies support minimal deterministic control flow
A process body SHALL support field access, enum variant references, literals, comparisons, boolean operations, `if`/`else` branching, and `return` statements sufficient to express deterministic business decisions.

#### Scenario: Branching process returns expected enum type
- **WHEN** a process returning `Decision` returns `Decision.review` in one branch and `Decision.approved` in another branch
- **THEN** validation accepts the process body

#### Scenario: Return type mismatch
- **WHEN** a process returns a value that does not match its declared output type
- **THEN** validation fails with a diagnostic identifying the return type mismatch

### Requirement: Rust generation follows successful validation
The compiler SHALL generate Rust output only after parsing, name validation, and type validation have succeeded.

#### Scenario: Valid source generates Rust
- **WHEN** an input `.bl` file passes validation
- **THEN** the compiler produces Rust code representing the declared types, lists, and processes

#### Scenario: Invalid source does not generate Rust
- **WHEN** an input `.bl` file fails parsing or validation
- **THEN** the compiler produces diagnostics and does not emit Rust code for that file

#### Scenario: Generated behavior is callable
- **WHEN** valid `.bl` declares a process that branches on a numeric field and returns an enum value
- **THEN** the generated Rust compiles and the callable process returns the expected value for each branch

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
