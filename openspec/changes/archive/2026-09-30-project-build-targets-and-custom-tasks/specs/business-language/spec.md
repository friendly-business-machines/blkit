# Spec Delta

## MODIFIED Requirements

### Requirement: Domain types are declared in source
The language SHALL support business-domain record declarations, enum declarations, and typed list values using only `.bl` source-defined domain types and built-in types. In a project build, a declaration MAY reference domain types defined in another file with the same namespace and process version. A standalone single-file compilation SHALL continue to resolve declarations from that file alone.

#### Scenario: Valid domain declarations
- **WHEN** a source declares a record type, an enum type, and a `List<T>` field
- **THEN** validation accepts the declarations when all referenced types exist in its compilation scope

#### Scenario: Shared domain type
- **WHEN** one `.bl` file defines `Order` and another file in the same project namespace and version uses `Order` in a process signature
- **THEN** project validation accepts the type regardless of source-file order

#### Scenario: Unknown type reference
- **WHEN** a declaration references a type that is neither built in nor declared in its compilation scope
- **THEN** validation fails with a diagnostic identifying the unknown type

## ADDED Requirements

### Requirement: Source-defined tasks and decisions are shared within a project scope
A project build SHALL resolve source-defined tasks and decision models across `.bl` files with matching namespace and process version before validating process graphs, independently of source-file order. Declarations from another namespace or process version SHALL NOT become visible implicitly; duplicate declaration names within a scope SHALL be rejected. Standalone single-file compilation SHALL retain its existing resolution behavior.

#### Scenario: Cross-file task call
- **WHEN** one file defines a typed task and another file in the same namespace and version calls it from a process node
- **THEN** validation accepts the call and the generated graph invokes that task

#### Scenario: Cross-file decision call
- **WHEN** a process uses a business-rule node naming a decision defined in another file with the same namespace and version
- **THEN** validation accepts and emits the decision call

#### Scenario: Duplicate or out-of-scope declaration
- **WHEN** two files in one scope define the same declaration name, or a process references a task only defined in a different namespace or version
- **THEN** project validation rejects the duplicate or unknown reference before emitting Rust

### Requirement: Process graphs may invoke crate-qualified custom task nodes
A project-backed `.bl` process SHALL be able to call a configured extension crate's task using a crate-qualified task reference. Each extension crate SHALL supply discoverable task signatures identifying callable names and `.bl` input/output types; the project manifest SHALL NOT duplicate those task declarations. The compiler SHALL resolve the signature for the call's namespace/version scope and check the argument, result type, and downstream uses before emitting Rust. External tasks SHALL NOT introduce new split, join, terminal, or routing node kinds. Existing source-defined task declarations SHALL remain supported.

#### Scenario: Valid custom task call
- **WHEN** `payments` is a configured crate advertising `charge(Order) -> Receipt`, and a process links `node payment = task payments.charge(input)` to a compatible downstream node
- **THEN** validation succeeds and the compiled graph calls the linked crate's task when `payment` runs

#### Scenario: Missing or invalid provider metadata
- **WHEN** a task references an unconfigured crate, an unadvertised callable, missing provider signatures, or a signature referring to a type absent from the caller's namespace/version scope
- **THEN** validation fails before Rust generation with a diagnostic identifying the provider, task, or type

#### Scenario: Incompatible task argument or result use
- **WHEN** a process passes an incompatible argument to a crate task or uses its advertised output as an incompatible type
- **THEN** validation fails before Rust generation with a diagnostic identifying the task and mismatch

#### Scenario: Provider does not implement its advertised signature
- **WHEN** the configured crate advertises a task but does not expose a callable compatible with the external-task interface
- **THEN** the project build fails rather than producing a runnable graph

#### Scenario: Source-defined tasks remain valid
- **WHEN** a source-defined task is used in a project alongside crate-qualified task nodes
- **THEN** its existing graph syntax and validation behavior are unchanged
