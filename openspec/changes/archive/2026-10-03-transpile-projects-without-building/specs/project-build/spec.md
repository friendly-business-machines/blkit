# Spec Delta

## MODIFIED Requirements

### Requirement: Projects discover sources and select one build target
A blkit project SHALL discover its `.bl` files under the project root without a source list in its manifest, and SHALL require exactly one `build_target` value from `crate`, `worker`, or `server`. Each target SHALL use the same validated generated process definitions from every discovered source. `blkit transpile [PROJECT_DIR]` SHALL generate a Cargo project in `PROJECT_DIR/.blkit` for the selected target, defaulting `PROJECT_DIR` to the current directory, without invoking `cargo build` or claiming a compiled artifact. A `crate` target SHALL generate a reusable Rust library; a `worker` target SHALL generate a distributed worker binary entry point; a `server` target SHALL generate a local REST server binary entry point. Users SHALL NOT need to author the generated Cargo project, but SHALL invoke Cargo themselves to compile it. `blkit build` SHALL NOT remain an alias for project transpilation.

#### Scenario: Reusable crate
- **WHEN** a project with valid `.bl` sources selects `crate` and the user invokes `blkit transpile`
- **THEN** `.blkit` contains a Cargo manifest and library source exposing the process definitions, and the CLI does not compile the library

#### Scenario: Worker target
- **WHEN** a project selects `worker` and the user invokes `blkit transpile`
- **THEN** `.blkit` contains a Cargo manifest and worker source using the project's process definitions, which the user can compile with Cargo into a distributed worker that claims only matching process identities from PostgreSQL and requires a separate distributed API for REST access

#### Scenario: All-in-one server target
- **WHEN** a project selects `server` and the user invokes `blkit transpile`
- **THEN** `.blkit` contains a Cargo manifest and server source using the project's process definitions, which the user can compile with Cargo into a local REST server that executes processes using a durable local store without PostgreSQL or a separate worker

#### Scenario: User builds locally or elsewhere
- **WHEN** transpilation succeeds for any target
- **THEN** the user can invoke `cargo build --manifest-path PROJECT_DIR/.blkit/Cargo.toml` locally or on another machine with the generated project and its required inputs, and blkit has not run `cargo build`

#### Scenario: Old project command
- **WHEN** the user invokes `blkit build`
- **THEN** the CLI rejects the removed command rather than compiling a binary or silently transpiling

#### Scenario: Exactly one target
- **WHEN** `build_target` is absent, names an unsupported target, or specifies multiple targets
- **THEN** transpilation fails with an actionable diagnostic rather than reporting a successful generated project

#### Scenario: Discover every source
- **WHEN** a project contains two `.bl` files in different directories and declares no source paths
- **THEN** transpilation discovers and validates both, and generates one Cargo project for the selected target with all compiled process definitions

#### Scenario: No sources or invalid source
- **WHEN** the project has no discoverable `.bl` file or one discovered source is invalid
- **THEN** transpilation fails with an actionable diagnostic and does not report a successful generated project

### Requirement: Multi-file projects compile shared declaration scopes
All discovered `.bl` files in a project that declare the same namespace and process version SHALL form one declaration scope. Types, tasks, decisions, and processes declared in one such file SHALL be available to processes in another such file without depending on source-file order. Files with different namespaces or process versions SHALL have separate declaration scopes and SHALL NOT implicitly share declarations. Project transpilation SHALL reject duplicate declarations within a scope and duplicate namespace/version/process identities, and SHALL NOT report successful generation of a partial set of processes if any discovered source fails validation.

#### Scenario: Cross-file task and type
- **WHEN** one source defines `Order` and `charge(input: Order) -> Receipt`, and another source with the same namespace and version defines a process that calls `charge` and uses `Receipt`
- **THEN** validation succeeds regardless of file order, and the generated Rust project contains the process definitions for the selected target

#### Scenario: Cross-file decision model
- **WHEN** one source defines a typed decision model and another source in the same namespace and version calls it from a business-rule node
- **THEN** transpilation resolves and validates the model as part of the shared scope

#### Scenario: Cross-file subprocess
- **WHEN** one source defines a process and another source in the same namespace and version calls it through a subprocess node
- **THEN** validation succeeds regardless of file order, and the generated project contains both processes so the compiled target executes the child within the caller's instance

#### Scenario: Distinct identities remain separate
- **WHEN** files declare different namespaces or process versions and use the same declaration name
- **THEN** each declaration belongs only to its own scope and unqualified use from the other scope fails validation

#### Scenario: Duplicate or invalid file stops whole build
- **WHEN** two discovered files in the same scope define the same declaration or one discovered file has a validation error
- **THEN** the project reports the offending file/declaration and does not report successful generation of a crate, worker, or server project

### Requirement: Projects resolve toolchain and extension dependencies reproducibly
A blkit project SHALL declare the blkit version and any custom task crate dependencies with version requirements, without listing individual tasks or source paths in its manifest. A mismatched blkit CLI/compiler and declared blkit version SHALL fail early with an actionable diagnostic rather than be compared with the `.bl` process version. Project transpilation SHALL continue using installed Cargo to resolve referenced custom task crates and their task descriptors, respecting an existing project lockfile; it SHALL report a missing Cargo executable or an unresolved dependency rather than claim successful generation. `blkit update` SHALL remain an explicit operation to refresh dependency locks. Cargo compilation and linker/type errors SHALL be reported by the user's subsequent Cargo build, not by `blkit transpile`; executing workers and servers SHALL NOT load extension crates or compile `.bl` at runtime.

#### Scenario: Rebuilding locked project
- **WHEN** a project with a lockfile is transpiled with unchanged configuration and sources
- **THEN** resolution of referenced custom task crates uses the locked dependency versions unless the author explicitly updates them

#### Scenario: Compiler version is distinct from process identity
- **WHEN** `.bl` declares `version "1.0"` and the project declares a different blkit toolchain version
- **THEN** the `.bl` version is used for process identity, not toolchain compatibility

#### Scenario: Unsupported toolchain or extension
- **WHEN** the invoked compiler differs from the project-declared blkit version or a referenced custom task crate cannot be resolved
- **THEN** transpilation fails and identifies the incompatible or unresolved dependency

#### Scenario: Missing Cargo when resolving extension tasks
- **WHEN** a project references custom task crates and Cargo is unavailable at transpilation time
- **THEN** the CLI fails with an actionable Cargo diagnostic, even if compilation would occur on another machine

#### Scenario: Built-in-only project on a machine without Cargo
- **WHEN** a project uses only built-in tasks and Cargo is unavailable
- **THEN** transpilation succeeds without running Cargo; the user can compile the generated project later on a machine with a Rust toolchain

#### Scenario: Explicit dependency update
- **WHEN** the user invokes `blkit update` for a project with changed dependency requirements
- **THEN** installed Cargo refreshes the generated project's lockfile without building its crate or executable

#### Scenario: Dependency fails Rust compilation
- **WHEN** a custom task crate resolves but its Rust callable cannot compile or link with the generated project
- **THEN** `blkit transpile` does not promise a runnable artifact, and the user's Cargo build reports the compilation failure
