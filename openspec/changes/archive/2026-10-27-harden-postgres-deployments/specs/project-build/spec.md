# Spec Delta

## ADDED Requirements

### Requirement: Generated remote binaries load database configuration only from environment or files
Every generated remote target, including compatibility `worker`, SHALL load PostgreSQL connection configuration from role-specific environment settings, an explicitly selected dotenv file, or mounted secret files. No generated executable SHALL accept a database URL, username, password, or equivalent credential-bearing connection string as a positional argument or option value. File-path options SHALL select files, never inline credentials. Ambient environment settings SHALL take precedence over dotenv values; supplying both a direct setting and its secret-file counterpart in the effective configuration SHALL fail rather than choose silently. Selected files that are missing, unreadable, malformed, or empty where a value is required SHALL fail startup. Configuration files SHALL be parsed as data, not executed as shell scripts, and SHALL NOT be discovered automatically from the working directory. Diagnostics and operational logs SHALL identify failing setting names or file operations without printing credential contents or connection strings.

#### Scenario: Environment-only start
- **WHEN** an API or worker starts with its role-specific database URL in the environment and no database arguments
- **THEN** it connects using that configuration and retains its documented non-secret task, lease, or bind settings

#### Scenario: Explicit configuration file
- **WHEN** a remote binary is given an explicit dotenv path and a role URL is also set in the process environment
- **THEN** the process environment wins, file contents are not executed, and no implicit `.env` search occurs

#### Scenario: Mounted secret
- **WHEN** the role's database URL is supplied through its configured secret-file path
- **THEN** the binary reads the URL from the file without requiring the secret value in process arguments

#### Scenario: Ambiguous or invalid configuration
- **WHEN** both a direct URL setting and its file counterpart are present, a required role URL is absent, or an explicitly selected configuration/secret file is invalid
- **THEN** startup fails with a redacted setting-specific diagnostic before opening database sessions

#### Scenario: Legacy database argument
- **WHEN** a caller supplies a legacy positional PostgreSQL URL or a database credential option value
- **THEN** the generated binary exits nonzero with safe usage guidance and does not connect, echo the supplied secret, or reinterpret it as a bind address

### Requirement: Combined remote targets retain role-separated authentication
API-only and split API executables SHALL require API database configuration only; worker-only, compatibility worker, and split worker executables SHALL require worker database configuration only. Combined remote API/worker executables SHALL require separate API and worker connection settings for the same database and distinct login identities, authenticate each pool with the corresponding credentials, and use the API identity for notification listeners. A missing role configuration or a combined configuration reusing one login for both roles SHALL fail startup without falling back to a shared administrative or generic URL. Unused role configuration SHALL NOT become mandatory for single-role executables.

#### Scenario: Combined executable
- **WHEN** a combined remote binary starts with distinct provisioned API and worker URLs for the same database
- **THEN** API operations and notification listening use the API login while execution uses the worker login and each retains its own pool limit

#### Scenario: Role credentials missing or collapsed
- **WHEN** a combined remote binary has only one role configured or both URLs identify the same login
- **THEN** startup rejects the configuration without reusing one identity for both pools

#### Scenario: Single-role deployment
- **WHEN** an API-only or worker-only executable has valid configuration for its own role
- **THEN** it does not require credentials for the other role

### Requirement: PostgreSQL administration is excluded from generated application dependencies
Generated remote executables SHALL depend only on PostgreSQL runtime connectivity, configuration, and schema-compatibility capabilities, not executable PostgreSQL initialization/migration code or a CLI dependency. Local all-in-one `server` and local `api-worker` targets SHALL retain automatic Turso initialization and migration through core. Local and reusable-crate targets SHALL NOT gain PostgreSQL, PostgreSQL TLS, or administrative dependencies from this change. Worker-only outputs SHALL remain free of HTTP-server dependencies, and API-only outputs SHALL remain free of compiled process definitions and worker execution dependencies.

#### Scenario: Remote startup cannot administer PostgreSQL
- **WHEN** a generated remote binary is built and invoked
- **THEN** it has no PostgreSQL init/migrate command or linked migration assets and can start against a prepared schema without administrative privileges

#### Scenario: Local Turso compatibility
- **WHEN** a local all-in-one executable opens a new or existing compatible Turso database
- **THEN** core initializes or migrates it as before, without a `blkit db` step, PostgreSQL credentials, or PostgreSQL dependencies

#### Scenario: Build-role isolation
- **WHEN** reusable, local, API-only, worker-only, and independently built split outputs are inspected and built
- **THEN** they retain their existing backend and role dependency exclusions, and only remote outputs acquire PostgreSQL TLS dependencies
