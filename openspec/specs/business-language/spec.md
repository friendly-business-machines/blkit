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
