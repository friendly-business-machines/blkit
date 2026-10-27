# CLI Experience Spec Delta

## ADDED Requirements

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
