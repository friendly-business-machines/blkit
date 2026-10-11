# Spec Delta

## ADDED Requirements

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
