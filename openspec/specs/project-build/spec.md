# project-build Specification

## Purpose

Defines a user-facing blkit project that builds `.bl` sources into a reusable Rust crate or packaged executables without requiring authors to maintain generated Rust by hand.

## Requirements

### Requirement: Projects discover sources and select one build target
A blkit project SHALL discover its `.bl` files under the project root without a source list in its manifest, and SHALL require exactly one `build_target` value from `crate`, `worker`, or `server`. Each target SHALL use the same validated generated process definitions from every discovered source. Building `crate` SHALL produce a reusable Rust library; building `worker` SHALL produce a runnable distributed worker; building `server` SHALL produce a runnable all-in-one local REST server that both accepts requests and executes processes. Users SHALL NOT need to author a Cargo project to build any of these targets.

#### Scenario: Reusable crate
- **WHEN** a project with valid `.bl` sources selects `crate`
- **THEN** its build produces a library exposing the compiled process definitions for integration with another Rust project

#### Scenario: Worker target
- **WHEN** a project selects `worker` and builds it
- **THEN** the binary contains that project's compiled process definitions, claims only matching process identities from PostgreSQL, and requires a separately operated distributed API for REST access

#### Scenario: All-in-one server target
- **WHEN** a project selects `server` and builds it
- **THEN** the binary serves the existing process-control REST endpoints and executes the project's compiled definitions using a durable local store without requiring PostgreSQL or a separate worker

#### Scenario: Exactly one target
- **WHEN** `build_target` is absent, names an unsupported target, or specifies multiple targets
- **THEN** the build fails with an actionable diagnostic rather than producing a successful artifact

#### Scenario: Discover every source
- **WHEN** a project contains two `.bl` files in different directories and declares no source paths
- **THEN** the build discovers and validates both, and packages their compiled processes into the one selected target

#### Scenario: No sources or invalid source
- **WHEN** the project has no discoverable `.bl` file or one discovered source is invalid
- **THEN** the build fails with an actionable diagnostic and does not report a successful artifact

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

### Requirement: Projects resolve toolchain and extension dependencies reproducibly
A blkit project SHALL declare the blkit version and any custom task crate dependencies with version requirements, without listing individual tasks or source paths in its manifest, with a project lockfile recording the versions actually built. A mismatched blkit CLI/compiler and declared blkit version SHALL fail early with an actionable diagnostic rather than be compared with the `.bl` process version. The build SHALL fail if an extension dependency cannot be resolved or linked; executing workers and servers SHALL NOT load extension crates or compile `.bl` at runtime.

#### Scenario: Rebuilding locked project
- **WHEN** a project with a lockfile is rebuilt with unchanged configuration and sources
- **THEN** the build uses the locked dependency versions unless the author explicitly updates them

#### Scenario: Compiler version is distinct from process identity
- **WHEN** `.bl` declares `version "1.0"` and the project declares a different blkit toolchain version
- **THEN** the `.bl` version is used for process identity, not toolchain compatibility

#### Scenario: Unsupported toolchain or extension
- **WHEN** the invoked compiler differs from the project-declared blkit version or a declared extension cannot be resolved
- **THEN** the build fails before packaging a runnable target and identifies the incompatible or unresolved dependency

### Requirement: Existing direct source transpilation remains available
The existing single-file CLI invocation SHALL continue to accept one `.bl` file and emit Rust without requiring a blkit project configuration, for sources using built-in tasks.

#### Scenario: Legacy compile command
- **WHEN** an existing built-in-only `.bl` file is compiled with `blkit SOURCE.bl OUTPUT.rs`
- **THEN** the command still emits Rust or reports its validation errors without requiring a project manifest
