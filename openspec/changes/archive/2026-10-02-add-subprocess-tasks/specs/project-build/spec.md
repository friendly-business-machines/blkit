# Spec Delta

## MODIFIED Requirements

### Requirement: Multi-file projects compile shared declaration scopes
All discovered `.bl` files in a project that declare the same namespace and process version SHALL form one declaration scope. Types, tasks, decisions, and processes declared in one such file SHALL be available to processes in another such file without depending on source-file order. Files with different namespaces or process versions SHALL have separate declaration scopes and SHALL NOT implicitly share declarations. The project build SHALL reject duplicate declarations within a scope and duplicate namespace/version/process identities, and SHALL NOT package a partial set of processes if any discovered source fails validation.

#### Scenario: Cross-file task and type
- **WHEN** one source defines `Order` and `charge(input: Order) -> Receipt`, and another source with the same namespace and version defines a process that calls `charge` and uses `Receipt`
- **THEN** validation succeeds regardless of file order, and the selected build target contains the executable process

#### Scenario: Cross-file decision model
- **WHEN** one source defines a typed decision model and another source with the same namespace and version calls it from a business-rule node
- **THEN** the project build resolves and validates the model as part of the shared scope

#### Scenario: Cross-file subprocess
- **WHEN** one source defines a process and another source in the same namespace and version calls it through a subprocess node
- **THEN** validation succeeds regardless of file order, and the selected build target executes the child within the caller's instance

#### Scenario: Distinct identities remain separate
- **WHEN** files declare different namespaces or process versions and use the same declaration name
- **THEN** each declaration belongs only to its own scope and unqualified use from the other scope fails validation

#### Scenario: Duplicate or invalid file stops whole build
- **WHEN** two discovered files in the same scope define the same declaration or one discovered file has a validation error
- **THEN** the project reports the offending file/declaration and produces no successful crate, worker, or server artifact for that build
