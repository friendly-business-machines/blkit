# cli-experience Specification

## Purpose

Defines a usable command-line interface for compiling and building blkit projects, with discoverable usage, contextual errors, and output appropriate for humans and scripts.

## Requirements

### Requirement: Existing CLI invocations remain compatible
The blkit CLI SHALL continue to accept `blkit SOURCE.bl OUTPUT.rs`, `blkit build [PROJECT_DIR]`, and `blkit update [PROJECT_DIR]`, with `.` as the default project directory. It SHALL preserve the existing source output, selected project build target, and dependency update behavior. The CLI SHALL NOT introduce another subcommand as part of this change.

#### Scenario: Direct source compilation
- **WHEN** a user invokes `blkit SOURCE.bl OUTPUT.rs` with a valid built-in-only source
- **THEN** the CLI writes generated Rust at `OUTPUT.rs` without a project manifest

#### Scenario: Project commands
- **WHEN** a user invokes `blkit build` or `blkit update` with or without `PROJECT_DIR`
- **THEN** the CLI runs the selected operation for the given directory or the current directory by default

### Requirement: Usage and version are discoverable
The CLI SHALL provide generated top-level and subcommand help, and a version display. Missing or invalid arguments SHALL produce actionable usage text and a nonzero exit status without running a project operation or writing an output file.

#### Scenario: Asking for help
- **WHEN** a user invokes `blkit --help` or `blkit build --help`
- **THEN** the CLI succeeds and displays usage for the requested invocation

#### Scenario: Invalid invocation
- **WHEN** a user omits required operands, supplies unexpected extra arguments, or selects an unknown command
- **THEN** the CLI reports usage to stderr, exits nonzero, and performs no compilation or build

### Requirement: Shell completion is available without another command
The CLI SHALL provide a top-level `--completions SHELL` option that writes a completion script for a supported shell to stdout and then exits successfully without reading project files or building. Unsupported shells SHALL fail with usage information.

#### Scenario: Requesting completion
- **WHEN** a user invokes `blkit --completions bash`
- **THEN** stdout contains a completion script for the existing commands and options, and no project output is created

#### Scenario: Unsupported shell
- **WHEN** a user supplies an unsupported shell to `--completions`
- **THEN** the CLI exits nonzero with an actionable diagnostic and no completion script

### Requirement: Diagnostics are contextual and automation-safe
Operational errors SHALL identify the failing source, project, or underlying operation where available and exit nonzero. CLI-generated failures SHALL go to stderr; completion scripts SHALL go to stdout. Terminal styling and transient progress SHALL appear only when stderr is an interactive terminal and styling is enabled; non-interactive or color-disabled output SHALL contain no ANSI control sequences or progress animation. Compiler diagnostics SHALL NOT claim a source span unless one is known.

#### Scenario: Invalid source
- **WHEN** direct transpilation fails parsing or validation
- **THEN** the CLI reports the source path and underlying error on stderr, returns nonzero, and does not write the output file

#### Scenario: Project build failure
- **WHEN** a project build fails during manifest validation, compilation, or Cargo execution
- **THEN** the CLI reports the operation and original failure details on stderr, returns nonzero, and does not display a success message

#### Scenario: Redirected output
- **WHEN** stderr is captured by a script or styling is disabled
- **THEN** errors contain no ANSI escape sequences and no transient progress frames

### Requirement: Interactive project feedback
During project `build` and `update`, the CLI SHALL show transient status on an interactive stderr terminal, clear it when the operation ends, and report failure details without obscuring them. Direct source transpilation and machine-readable completion output SHALL remain free of progress animation.

#### Scenario: Interactive build
- **WHEN** a user starts a project build in an interactive terminal
- **THEN** the CLI displays progress while waiting and removes transient status when the build finishes or fails

#### Scenario: Piped build
- **WHEN** a user runs the same build with stderr redirected
- **THEN** the build still runs with the same result and no progress frames are emitted
