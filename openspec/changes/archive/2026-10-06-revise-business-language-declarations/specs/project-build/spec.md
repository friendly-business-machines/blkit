# Spec Delta

## MODIFIED Requirements

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
