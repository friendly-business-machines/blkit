# Spec Delta

## ADDED Requirements

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
