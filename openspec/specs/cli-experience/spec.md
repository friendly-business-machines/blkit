# cli-experience Specification

## Purpose

Defines a usable command-line interface for compiling and building blkit projects, with discoverable usage, contextual errors, and output appropriate for humans and scripts.

## Requirements

### Requirement: Existing CLI invocations remain compatible
The blkit CLI SHALL accept `blkit SOURCE.bl OUTPUT.rs`, `blkit transpile [PROJECT_DIR]`, and `blkit update [PROJECT_DIR]`, with `.` as the default project directory. It SHALL preserve direct source output, selected project target, and dependency update behavior. `blkit build` SHALL be rejected rather than accepted as an alias or invoked to compile the generated project.

#### Scenario: Direct source compilation
- **WHEN** a user invokes `blkit SOURCE.bl OUTPUT.rs` with a valid built-in-only source
- **THEN** the CLI writes generated Rust at `OUTPUT.rs` without a project manifest

#### Scenario: Project commands
- **WHEN** a user invokes `blkit transpile` or `blkit update` with or without `PROJECT_DIR`
- **THEN** the CLI runs the selected operation for the given directory or the current directory by default

#### Scenario: Removed build command
- **WHEN** a user invokes `blkit build`
- **THEN** the CLI reports invalid usage and does not transpile or compile a project

### Requirement: Usage and version are discoverable
The CLI SHALL provide generated top-level and subcommand help, and a version display. Missing or invalid arguments SHALL produce actionable usage text and a nonzero exit status without running a project operation or writing an output file.

#### Scenario: Asking for help
- **WHEN** a user invokes `blkit --help` or `blkit transpile --help`
- **THEN** the CLI succeeds and displays usage for the requested invocation

#### Scenario: Invalid invocation
- **WHEN** a user omits required operands, supplies unexpected extra arguments, or selects an unknown command
- **THEN** the CLI reports usage to stderr, exits nonzero, and performs no compilation or project transpilation

### Requirement: Shell completion is available without another command
The CLI SHALL provide a top-level `--completions SHELL` option that writes a completion script for a supported shell to stdout and then exits successfully without reading project files or building. Unsupported shells SHALL fail with usage information.

#### Scenario: Requesting completion
- **WHEN** a user invokes `blkit --completions bash`
- **THEN** stdout contains a completion script for the existing commands and options, and no project output is created

#### Scenario: Unsupported shell
- **WHEN** a user supplies an unsupported shell to `--completions`
- **THEN** the CLI exits nonzero with an actionable diagnostic and no completion script

### Requirement: Diagnostics are contextual and automation-safe
Operational errors SHALL identify the failing source, project, or underlying operation where available and exit nonzero. CLI-generated failures SHALL go to stderr; completion scripts SHALL go to stdout. For `.bl` lexical and syntax errors, and source-bound validation errors with a known location, direct and project transpilation SHALL display the correct file path, source excerpt, location, and a readable explanation. If no reliable location is known, the CLI SHALL show the path and explanation without a fabricated span. Terminal styling and transient progress SHALL appear only when stderr is an interactive terminal and styling is enabled; non-interactive or color-disabled output SHALL contain no ANSI control sequences or progress animation. The CLI SHALL preserve accessible plain-text diagnostic content without requiring terminal color or graphics.

#### Scenario: Invalid source
- **WHEN** direct transpilation fails parsing or validation
- **THEN** the CLI reports the source path and underlying error on stderr, includes a source excerpt and location when known, returns nonzero, and does not write the output file

#### Scenario: Project build failure
- **WHEN** project transpilation fails during manifest validation, source validation, or Cargo dependency resolution
- **THEN** the CLI reports the operation and original failure details on stderr, returns nonzero, and does not display a success message; a located `.bl` error uses the offending project's source file rather than just its project root

#### Scenario: Redirected output
- **WHEN** stderr is captured by a script or styling is disabled
- **THEN** errors contain no ANSI escape sequences and no transient progress frames; any available file path, location, and explanatory text remain readable

#### Scenario: Error without a source location
- **WHEN** compilation reports a global validation failure without a reliable source span
- **THEN** the CLI reports the error with its source or project context but does not underline an unrelated source token

### Requirement: Interactive project feedback
During project `transpile` and `update`, the CLI SHALL show transient status on an interactive stderr terminal, clear it when the operation ends, and report failure details without obscuring them. Direct source transpilation and machine-readable completion output SHALL remain free of progress animation.

#### Scenario: Interactive build
- **WHEN** a user starts project transpilation in an interactive terminal
- **THEN** the CLI displays progress while waiting and removes transient status when transpilation finishes or fails

#### Scenario: Piped build
- **WHEN** a user runs the same transpilation with stderr redirected
- **THEN** transpilation still runs with the same result and no progress frames are emitted

### Requirement: The distributable CLI builds independently of development demos
The repository SHALL build a distributable `blkit` CLI executable without building its development API, server, or worker executables or compiling the repository's example process as part of that CLI build. The CLI SHALL continue to support the existing source-transpilation, project-transpilation, update, help, and completion invocations. Development executables SHALL remain buildable and testable separately, and the full workspace test suite SHALL continue to cover them.

#### Scenario: Build the CLI for distribution
- **WHEN** an operator selects the `blkit` CLI build from the workspace
- **THEN** Cargo produces the CLI without building the repository's development executables or running their example-process build script

#### Scenario: Preserve the installed CLI interface
- **WHEN** an operator runs the separately built `blkit` executable on a valid source or project
- **THEN** its supported commands and generated project behavior remain the same as before the package separation

#### Scenario: Run complete repository checks
- **WHEN** a developer runs the full workspace test suite
- **THEN** the development API and worker binaries remain available to their integration tests

### Requirement: Database administration is discoverable and credential-safe
The CLI SHALL expose `blkit db init` and `blkit db migrate` in top-level help, subcommand help, and shell completion. Their help SHALL describe administrative versus runtime credentials, environment/dotenv/secret-file inputs, the corporate CA configuration, the explicit insecure development/testing override, managed role naming, and the existing dedicated-database prerequisite. Configuration file paths and non-secret role-prefix options SHALL be permitted as arguments; database connection strings, usernames, passwords, and other database credential values SHALL NOT be accepted as arguments. Invalid usage and help/completion requests SHALL NOT connect to or mutate a database. Operational failures SHALL exit nonzero, report redacted context to stderr, and never claim success. Existing source transpilation, project transpilation, update, version, and completion behavior SHALL be retained.

#### Scenario: Discover administration
- **WHEN** a user requests top-level help, `blkit db --help`, `blkit db init --help`, `blkit db migrate --help`, or supported shell completion
- **THEN** the CLI describes the available database commands and relevant configuration contract without reading credential files or opening database sessions

#### Scenario: No credentials in arguments
- **WHEN** a user attempts to supply an administrative or runtime database URL, username, or password as a command-line value
- **THEN** the CLI rejects the invocation without connecting and without echoing the secret-bearing value in diagnostics

#### Scenario: Administrative connection fails
- **WHEN** database configuration, TLS validation, authentication, permissions, or a migration fails
- **THEN** the CLI exits nonzero and identifies the failed operation or setting on stderr without exposing credentials or claiming successful preparation

#### Scenario: Existing compiler commands
- **WHEN** a user invokes a previously supported source/transpile/update/version/completion command
- **THEN** it retains its behavior without requiring database configuration, runtime credentials, or network access to PostgreSQL

### Requirement: Initialize a standalone blkit project
The CLI SHALL accept `blkit init [PROJECT_DIR]`, defaulting to the current directory and creating a missing destination directory. Successful initialization of an otherwise empty destination SHALL create `blkit.toml` and one valid starter `main.bl`. The manifest SHALL contain the destination directory's name, the invoked CLI version as `project.blkit`, `build_target = "api-worker"`, and `persistence = "local"`. The starter SHALL define a complete executable process using supported syntax and no external task dependencies. The project SHALL be accepted by the existing transpile operation without requiring PostgreSQL, cloud credentials, Cargo, or rustc. Initialization SHALL NOT generate a root Cargo package, transpile sources, resolve dependencies, build or start an application, prepare a database, or request tool installation.

#### Scenario: Initialize the current directory
- **WHEN** a user runs `blkit init --no-vcs` in an otherwise empty directory with a valid project name
- **THEN** that directory contains the matching manifest and starter source, without a root `Cargo.toml`, generated `.blkit` project, or runtime database

#### Scenario: Initialize a named directory
- **WHEN** a user runs `blkit init my-app --no-vcs` and `my-app` does not exist
- **THEN** the CLI creates the directory and a standalone manifest named `my-app`, with a starter source that does not require deriving a language identifier from the hyphenated package name

#### Scenario: Transpile the initialized project without Rust tools
- **WHEN** a project initialized in an otherwise empty destination is subsequently transpiled with Cargo and rustc unavailable
- **THEN** transpilation succeeds and generates the existing local API/worker Cargo project under `.blkit`, without compiling it

### Requirement: Initialization preserves existing files
Initialization SHALL refuse to replace an existing `blkit.toml` or `main.bl`, including a directory or symbolic link at either path. Such a conflict SHALL be reported before creating either scaffold file or initializing Git. Initialization SHALL preserve unrelated files, existing repository metadata, and existing ignore rules. A destination that is not a directory or whose directory-derived name cannot be used by the generated project SHALL fail with an actionable diagnostic instead of producing a knowingly unusable scaffold. Filesystem or Git failures SHALL exit nonzero with project/operation context and SHALL NOT print a successful initialization message.

#### Scenario: Existing manifest or starter
- **WHEN** either scaffold path already exists, including a dangling symbolic link
- **THEN** initialization exits nonzero, identifies the conflict, leaves that path unchanged, and creates neither the other scaffold file nor a new repository

#### Scenario: Existing unrelated files
- **WHEN** initialization succeeds in a directory containing an unrelated file and an existing `.gitignore`
- **THEN** both existing files retain their original contents

#### Scenario: Invalid destination or package name
- **WHEN** the destination is a regular file or its directory-derived name is not usable as the generated Cargo package and crate name
- **THEN** initialization fails before writing scaffold files and explains how to select a usable destination

#### Scenario: Repeated initialization
- **WHEN** a user initializes a project that was already initialized
- **THEN** the CLI refuses the existing scaffold rather than replacing user edits or reporting another successful initialization

### Requirement: Initialization provides opt-out Git setup
Initialization SHALL accept the boolean `--no-vcs` flag and enable Git setup by default. Git SHALL be the only supported VCS; initialization SHALL NOT accept a value-taking `--vcs` selector. Without `--no-vcs`, it SHALL initialize a repository only when the destination is not already in a Git working tree. It SHALL recognize both ordinary repositories and linked worktrees, preserve existing Git configuration and metadata, and never stage files or make a commit. Git SHALL NOT be required or invoked when `--no-vcs` is supplied or an enclosing working tree is already recognized. When a new repository is required but Git cannot be run, initialization SHALL fail with guidance to install Git or use `--no-vcs`, without silently falling back to unversioned initialization.

#### Scenario: Default Git initialization
- **WHEN** a user runs `blkit init` outside a Git working tree with Git available
- **THEN** the destination becomes a Git repository with the new scaffold files uncommitted

#### Scenario: Existing parent repository or linked worktree
- **WHEN** the destination is inside an ordinary Git working tree or linked worktree
- **THEN** initialization creates no nested repository, preserves the existing metadata, and does not require invoking Git

#### Scenario: Explicit VCS opt-out
- **WHEN** a user runs `blkit init --no-vcs` with Git unavailable
- **THEN** initialization succeeds without invoking Git or creating repository metadata

#### Scenario: Git unavailable outside a repository
- **WHEN** default Git initialization requires a new repository and Git cannot be executed
- **THEN** the CLI exits nonzero, recommends Git installation or `--no-vcs`, and does not report successful initialization

### Requirement: Initialization checks Rust tools without requiring them
Initialization SHALL attempt advisory availability checks for Cargo and rustc using their version commands. A tool that cannot be started, exits unsuccessfully, or fails to respond within a bounded wait SHALL produce a named warning on stderr and SHALL NOT by itself prevent successful project creation. A successful probe SHALL NOT be described as proof of compiler-version compatibility or build readiness. These checks SHALL NOT run Cargo initialization, metadata, update, compilation, or rustc compilation, and SHALL NOT request automatic toolchain installation. Init SHALL NOT require other build, cloud, or database tools merely to scaffold the local project.

#### Scenario: Cargo and rustc unavailable
- **WHEN** Cargo and rustc cannot be executed but filesystem and selected VCS operations succeed
- **THEN** initialization exits zero, creates the project, and emits separate warnings explaining that the missing tools are needed to build rather than author or transpile the starter

#### Scenario: Installed command is unusable
- **WHEN** a Cargo or rustc executable exists but its version probe exits unsuccessfully or times out
- **THEN** initialization warns about that tool and still succeeds if the required initialization operations succeed

#### Scenario: Tools respond successfully
- **WHEN** both version probes succeed
- **THEN** initialization does not emit missing-tool warnings or claim that the generated application's build has been verified

### Requirement: Initialization is discoverable and explains the next steps
Top-level help, `blkit init --help`, and shell completion SHALL expose project initialization and the boolean `--no-vcs` flag in every supported shell; the flag SHALL NOT take a value or require value completion. Help, completion, and invalid argument handling SHALL NOT create files, run dependency probes, or initialize a repository. Successful initialization SHALL print next steps for entering the destination when needed, transpiling, building with the generated Cargo manifest, and running the local API/worker on its default loopback bind. Instructions SHALL distinguish successful scaffolding from an application that has been transpiled, built, or started. Existing direct transpilation, project transpilation, update, database administration, version, and completion invocations SHALL retain their behavior.

#### Scenario: Help or completion without a toolchain
- **WHEN** a user requests initialization help or shell completion with Git, Cargo, and rustc unavailable
- **THEN** the CLI succeeds with the relevant command/options and performs no probes or filesystem mutations

#### Scenario: Reject value-taking or legacy VCS syntax
- **WHEN** a user supplies the legacy `--vcs` selector, a value attached to `--no-vcs`, or extra positional arguments
- **THEN** the CLI exits nonzero with usage guidance without creating the destination or invoking external tools

#### Scenario: Successful initialization guidance
- **WHEN** initialization succeeds for a named destination
- **THEN** output identifies that destination, explains the separate transpile/build/run steps with its generated local executable name, and does not claim an application is already built or running
