# project-build Specification

## Purpose

Defines a user-facing blkit project that builds `.bl` sources into a reusable Rust crate or packaged executables without requiring authors to maintain generated Rust by hand.

## Requirements

### Requirement: Projects discover sources and select one build target
A blkit project SHALL discover its `.bl` files under the project root without a source list in its manifest and SHALL require exactly one `build_target` value from `crate`, `api-only`, `api-worker`, `api-worker-split`, `worker-only`, or the compatibility values `server` and `worker`. `api-worker` SHALL additionally require `persistence = "local"` or `persistence = "remote"` in `blkit.toml`; the other new targets SHALL select their single supported backend without an extra persistence field. The `server` and `worker` values SHALL remain equivalent to local `api-worker` and remote `worker-only`, respectively, without requiring a new field. `blkit transpile [PROJECT_DIR]` SHALL generate a Cargo project in `PROJECT_DIR/.blkit` for the selected target, defaulting `PROJECT_DIR` to the current directory, without invoking `cargo build` or claiming a compiled artifact. `crate` SHALL generate a reusable Rust library. `api-only` SHALL generate only a PostgreSQL-backed HTTP binary, shall succeed without `.bl` sources, and SHALL admit identities advertised by live workers rather than linking executable process definitions. `api-worker` SHALL generate one executable serving HTTP and running a worker using either a local durable store or PostgreSQL as selected. `api-worker-split` SHALL generate independently buildable remote API and worker binaries; `worker-only` SHALL generate a remote worker binary. Any discovered `.bl` source SHALL still validate, but targets with a worker SHALL require at least one source and SHALL derive compiled definitions from all valid discovered sources. Users SHALL NOT need to author the generated Cargo project, but SHALL invoke Cargo themselves to compile it. `blkit build` SHALL NOT remain an alias for project transpilation.

#### Scenario: Reusable crate
- **WHEN** a project with valid `.bl` sources selects `crate` and the user invokes `blkit transpile`
- **THEN** `.blkit` contains a Cargo manifest and library source exposing the process definitions, and the CLI does not compile the library

#### Scenario: Worker target
- **WHEN** a project selects the legacy `worker` or new `worker-only` target and invokes `blkit transpile`
- **THEN** `.blkit` contains a Cargo manifest and worker source using the project's process definitions, which the user can compile with Cargo into a remote worker that claims only matching process identities from PostgreSQL and requires a separate distributed API for REST access

#### Scenario: All-in-one server target
- **WHEN** a project selects the legacy `server` target or `api-worker` with local persistence and invokes `blkit transpile`
- **THEN** `.blkit` contains a Cargo manifest and source for one local HTTP executable whose in-process worker polls persisted work and executes compiled definitions with a concurrency limit, without PostgreSQL or a separate worker process

#### Scenario: One remote executable
- **WHEN** a project selects `api-worker` with remote persistence and invokes `blkit transpile`
- **THEN** `.blkit` contains a project buildable into one PostgreSQL-backed executable that serves the API and runs an advertised worker with the project's compiled definitions

#### Scenario: Separate remote executables
- **WHEN** a project selects `api-worker-split` and invokes `blkit transpile`
- **THEN** `.blkit` contains independently buildable API and worker binaries; only the worker binary links the project's compiled process definitions

#### Scenario: Source-free remote API
- **WHEN** a project with no `.bl` sources selects `api-only` and invokes `blkit transpile`
- **THEN** `.blkit` contains a buildable PostgreSQL-backed HTTP binary with no compiled process definitions, able to admit requests for any identity advertised by a live matching worker in its PostgreSQL store

#### Scenario: User builds locally or elsewhere
- **WHEN** transpilation succeeds for any target
- **THEN** the user can invoke Cargo with the generated project manifest locally or on another machine with its required inputs, and blkit has not run `cargo build`

#### Scenario: Old project command
- **WHEN** the user invokes `blkit build`
- **THEN** the CLI rejects the removed command rather than compiling a binary or silently transpiling

#### Scenario: Exactly one target
- **WHEN** `build_target` is absent, names an unsupported target, or specifies multiple targets
- **THEN** transpilation fails with an actionable diagnostic rather than reporting a successful generated project

#### Scenario: Discover every source
- **WHEN** a project contains two `.bl` files in different directories and declares no source paths
- **THEN** transpilation discovers and validates both, and generates the selected Cargo project with all compiled process definitions in each executable that needs them

#### Scenario: No sources or invalid source
- **WHEN** a target other than `api-only` has no discoverable `.bl` file, or any discovered source in any target is invalid
- **THEN** transpilation fails with an actionable diagnostic and does not report a successful generated project

### Requirement: Generated projects link only the runtime crate required for their target
Project transpilation SHALL emit Rust that imports `blkit_core` and manifests that depend on `blkit-core`, not `blkit-transpiler`. The generated project SHALL select only dependencies and runtime capabilities needed by each independently built output. Core SHALL offer additive `api-server` (HTTP), `worker` (execution with either storage backend), `local-persistence`, and `remote-persistence` capabilities; enabling both persistence capabilities SHALL remain supported. A `crate` build SHALL NOT include compiler/CLI, local-store, HTTP, PostgreSQL, or operational logging stacks in its normal dependency graph. `api-only` SHALL select `api-server` with `remote-persistence`, without generated process definitions or worker-execution dependencies. `worker-only` SHALL select `worker` with `remote-persistence`, without HTTP or local-store dependencies. Local and remote `api-worker` SHALL select `api-server` with `worker` and their corresponding persistence capability, excluding the unused backend; `api-worker-split` SHALL build its API and worker independently with their respective role features and `remote-persistence`. Selected targets SHALL retain existing process behavior and Rust consumer interfaces except for the documented local queuing, import, and distributed-API-admission changes.

#### Scenario: Small reusable crate
- **WHEN** a valid project selects `build_target = "crate"` and runs `blkit transpile`
- **THEN** its generated Cargo project builds with `blkit-core` and none of the compiler/CLI, local-store, HTTP, PostgreSQL, or logging dependencies in its normal dependency graph

#### Scenario: Worker dependencies
- **WHEN** a valid project selects `build_target = "worker-only"` and runs `blkit transpile`
- **THEN** the resulting worker builds and executes using remote persistence and worker execution without pulling in local-store or HTTP-server dependencies

#### Scenario: Local server dependencies
- **WHEN** a valid project selects `build_target = "api-worker"` with local persistence and runs `blkit transpile`
- **THEN** the resulting single binary builds HTTP and local queued worker execution without pulling in PostgreSQL dependencies

#### Scenario: API-only dependencies
- **WHEN** a project selects `api-only` and builds the generated API project
- **THEN** that build excludes compiler, local-store, worker-execution dependencies, and compiled process definitions

#### Scenario: Split API dependencies
- **WHEN** a valid project selects `api-worker-split` and builds only the generated API package
- **THEN** that build excludes compiler, local-store, and worker-execution dependencies and does not link compiled process definitions, while the separately built worker excludes HTTP dependencies

#### Scenario: Combined role dependencies
- **WHEN** a valid project selects `api-worker` with remote persistence and builds its executable
- **THEN** it contains HTTP and worker execution with remote persistence in one binary

#### Scenario: Additive persistence features
- **WHEN** a Rust consumer enables both `local-persistence` and `remote-persistence` in `blkit-core`
- **THEN** the feature combination compiles rather than being rejected as mutually exclusive

### Requirement: Generated runtime matches the transpiler release
`blkit.toml` SHALL continue to declare the `blkit` toolchain version and be checked against the invoked `blkit` CLI. The generated manifest SHALL request the compatible `blkit-core` release with an exact version requirement matching that CLI release; when using a local path it SHALL still require the matching version. A direct `blkit SOURCE.bl OUTPUT.rs` invocation SHALL emit embeddable Rust that requires `blkit-core` rather than a project manifest or transpiler dependency. Version-incompatible runtime dependencies SHALL be reported at the user's Cargo build or dependency resolution, not silently accepted.

#### Scenario: Matched generated project
- **WHEN** a project declares the installed CLI version and is transpiled
- **THEN** its generated manifest pins the matching `blkit-core` version and the generated Rust builds against that core release

#### Scenario: Version mismatch
- **WHEN** a project declares a different `blkit` toolchain version than the invoked CLI
- **THEN** transpilation rejects the project before reporting a successful generated project

#### Scenario: Direct source output
- **WHEN** a valid `.bl` file is transpiled with `blkit SOURCE.bl OUTPUT.rs`
- **THEN** the emitted Rust can compile in a consumer crate with `blkit-core` and its other required direct dependencies, without a generated project manifest or a dependency on `blkit-transpiler`

### Requirement: Multi-file projects compile shared declaration scopes
All discovered `.bl` files with the same namespace and process version SHALL form one declaration scope. Types, `start_event`, `end_event`, `decision_task`, other kind-specific graph-node peers, and processes declared in one such file SHALL be available to processes in another such file without depending on source-file order. Files with different namespaces or process versions SHALL have separate scopes and SHALL NOT implicitly share declarations. Project transpilation SHALL reject duplicate peer declarations within a scope and duplicate namespace/version/process identities; it SHALL NOT report successful partial generation when any discovered file fails validation. Generic `task` and `decision` declarations SHALL be rejected rather than treated as shared peers.

#### Scenario: Cross-file task and type
- **WHEN** one source defines `Order` and a `decision_task`, and a second source in the same scope declares a process with `flow` and `bind` referencing that decision task and `Order`
- **THEN** validation succeeds regardless of file order and the generated project includes the process

#### Scenario: Cross-file decision model
- **WHEN** a decision task is declared in another file in the same scope from its referencing process
- **THEN** validation resolves its named ports and decision nodes independent of file order

#### Scenario: Cross-file event
- **WHEN** a start or end event is declared in a different file in the same scope from its referencing process
- **THEN** validation resolves its named ports and event type independent of file order

#### Scenario: Cross-file subprocess
- **WHEN** one source defines a process and another in the same scope references it through a typed subprocess peer
- **THEN** validation succeeds regardless of file order and the generated project contains both processes

#### Scenario: Distinct identities remain separate
- **WHEN** files declare different namespaces or versions and use the same peer name
- **THEN** each declaration remains in its own scope, and unqualified use across scopes fails validation

#### Scenario: Duplicate or invalid file stops whole build
- **WHEN** two discovered files in one scope define the same peer or one discovered file has a syntax/type error
- **THEN** the project reports the offending file/declaration and does not report successful generation

### Requirement: Projects resolve toolchain and extension dependencies reproducibly
A blkit project SHALL declare the blkit version and MAY retain custom crate dependencies with version requirements without listing source paths in its manifest. A mismatched blkit CLI/compiler and declared blkit version SHALL fail early rather than be compared with `.bl` process version. `blkit update` SHALL remain an explicit operation to refresh dependency locks. New `.bl` sources SHALL NOT call extension-crate tasks until a kind-specific task form is supported; crate-backed legacy `task` nodes SHALL fail source validation rather than be resolved or linked. For supported sources, `blkit transpile` SHALL not require Cargo solely to parse or compile a `decision_task`. Cargo compilation and linker/type errors SHALL be reported by the user's subsequent Cargo build, not by transpilation; running workers and servers SHALL NOT compile `.bl`.

#### Scenario: Rebuilding locked project
- **WHEN** a project's dependency lockfile is used with unchanged configuration and sources
- **THEN** an explicit `blkit update` is still required to refresh locked dependencies; decision-task transpilation does not need Cargo metadata

#### Scenario: Compiler version is distinct from process identity
- **WHEN** `.bl` declares `version "1.0";` and the project declares a different blkit toolchain version
- **THEN** the `.bl` version is used for process identity, not toolchain compatibility

#### Scenario: Unsupported toolchain or extension
- **WHEN** the invoked compiler differs from the project-declared blkit version, or the source tries to call an extension task through a legacy node
- **THEN** transpilation fails and identifies the incompatible toolchain or unsupported task syntax

#### Scenario: Missing Cargo when resolving extension tasks
- **WHEN** a project source attempts a legacy extension-task call and Cargo is unavailable
- **THEN** transpilation rejects the unsupported source syntax rather than resolving a task crate

#### Scenario: Built-in-only project on a machine without Cargo
- **WHEN** a project uses only decision tasks and Cargo is unavailable
- **THEN** transpilation succeeds without running Cargo; the user can build the generated project later with a Rust toolchain

#### Scenario: Explicit dependency update
- **WHEN** the user invokes `blkit update` for a project with changed dependency requirements
- **THEN** installed Cargo refreshes the generated project's lockfile without building its crate or executable

#### Scenario: Dependency fails Rust compilation
- **WHEN** an unused configured dependency fails Rust compilation during a later Cargo build
- **THEN** `blkit transpile` does not promise a runnable artifact and the user's Cargo build reports the failure

#### Scenario: Legacy extension task
- **WHEN** `.bl` uses `node payment = task payments.charge(input)` with a configured crate
- **THEN** transpilation rejects the old syntax before claiming a successful project build

### Requirement: Existing direct source transpilation remains available
The existing single-file CLI invocation SHALL continue to accept one valid `.bl` file in the new syntax and emit Rust without requiring a project manifest. It SHALL reject the old generic `task` and `decision` syntax; source transpilation without a project SHALL resolve only peers in that one file.

#### Scenario: Legacy compile command
- **WHEN** `blkit SOURCE.bl OUTPUT.rs` receives a valid peer-declaration `.bl` source
- **THEN** the command emits Rust without requiring a project manifest

#### Scenario: Old source in direct compile
- **WHEN** the command receives a legacy colon/indentation process with generic `task` nodes
- **THEN** it reports validation errors and emits no successful Rust output
