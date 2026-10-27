# Spec Delta

## Purpose

Defines CLI-owned PostgreSQL initialization, schema upgrades, and least-privilege runtime role provisioning, keeping administrative credentials and database mutation out of deployed applications.

## ADDED Requirements

### Requirement: PostgreSQL administration belongs to the CLI
`blkit db init` SHALL prepare an existing, dedicated PostgreSQL database for runtime use, create the managed owner, API, and worker roles, set supplied credentials for newly created runtime logins, and grant their required permissions. `blkit db migrate` SHALL upgrade an initialized database or adopt a recognized pre-versioned blkit schema, provisioning missing managed roles and applying current grants. Both operations SHALL use separately supplied administrative credentials, the same verified-TLS policy as runtime connections, and no credentials supplied as command-line values. Neither operation SHALL require generated binaries, Cargo, `.bl` sources, cloud attachment credentials, or a project manifest. Neither operation SHALL create a PostgreSQL server or database. Generated runtime executables SHALL NOT contain or expose PostgreSQL initialization or migration operations.

#### Scenario: Fresh deployment
- **WHEN** an administrator runs `blkit db init` with valid administrative and runtime credential configuration against an empty database
- **THEN** the CLI installs the current schema and managed roles and grants, and API and worker logins can start their respective generated binaries without administrative credentials

#### Scenario: Insufficient administrative authority
- **WHEN** an initialization or upgrade connection cannot create the required roles or manage the required objects and grants
- **THEN** the CLI exits nonzero with an actionable, redacted diagnostic and does not claim the database is prepared

#### Scenario: Standalone administration
- **WHEN** a matching CLI is run on a deployment host without a project or generated application
- **THEN** it can initialize or migrate the configured PostgreSQL database without transpilation, compilation, or object-store access

### Requirement: Schema upgrades are versioned, serialized, and data preserving
The CLI SHALL record ordered schema versions and migration identities, serialize concurrent initialization and migration attempts for the same database, and apply each migration atomically with its version record. Repeating either command on a compatible prepared database SHALL preserve data and existing login credentials and SHALL converge managed grants to the required policy. Unknown schemas, inconsistent migration history, or schemas newer than the CLI understands SHALL be rejected without mutation. Adoption of a recognized existing blkit schema SHALL retain instance identities, statuses, retry/deadline state, checkpoints, requests, replies, attachments, and worker metadata. A failed migration SHALL NOT record its version as successfully applied. Only the CLI SHALL write migration history.

#### Scenario: Existing unversioned deployment
- **WHEN** `blkit db migrate` encounters a recognized legacy blkit schema with persisted instances and attachment metadata
- **THEN** it upgrades that schema and records its migration baseline without resetting existing application data

#### Scenario: Repeat and concurrent upgrade
- **WHEN** two CLI processes migrate the same database or initialization is repeated after success
- **THEN** their schema changes are serialized, each migration is applied at most once, and existing passwords and data remain unchanged

#### Scenario: Migration fails or schema is unknown
- **WHEN** a migration fails before commit, or the database has an unknown or newer schema/history
- **THEN** no failed migration is marked applied, an unknown/newer schema is not altered, and the CLI exits nonzero

### Requirement: Runtime roles receive only their operational permissions
The CLI SHALL provision distinct API and worker login roles and a non-login schema-owner role, with runtime logins unable to inherit or assume ownership or administrative privileges. Runtime roles SHALL NOT be superusers, create databases or roles, bypass row security, replicate, grant their own privileges, change schema objects, or write migration history. API permissions SHALL cover admission, status, cancellation, response submission, attachment metadata writes, SSE interests, and required reconciliation; they SHALL NOT permit worker registration, capability/policy publication, or checkpoint/result commits. Worker permissions SHALL cover advertisement, heartbeat/drain lifecycle, claiming, lease renewal, checkpoint/results, requests, recovery/reconciliation, and reading attachment metadata; they SHALL NOT permit admission of new instances, submission of client responses, insertion of attachment metadata, or mutation of API interests. Required read access for notification triggers SHALL remain available to both roles. Shared recovery operations SHALL retain existing lease/generation fencing and transaction safety. Grants SHALL be explicit for managed objects and required columns, not blanket write access to all tables. New schema objects SHALL NOT implicitly become writable by runtime roles or `PUBLIC`.

#### Scenario: Real runtime identities
- **WHEN** an API and worker use their separately provisioned logins on a prepared database
- **THEN** admission, execution, cancellation, requests/replies, attachments, SSE notification/reconnect, recovery, and draining succeed under their operational grants

#### Scenario: Forbidden cross-role access
- **WHEN** the API login attempts to publish worker capabilities or commit a checkpoint, or the worker login attempts admission, a client-response write, an attachment-metadata insert, or an API-interest mutation
- **THEN** PostgreSQL denies the unauthorized write, independently of application-side checks

#### Scenario: Administrative and future-object access
- **WHEN** either runtime login attempts schema DDL, a privilege grant, owner-role assumption, or migration-history mutation, or accesses an ungranted newly created object
- **THEN** PostgreSQL denies the operation

### Requirement: Provisioning is bounded to the managed deployment
Role names SHALL have a documented configurable prefix so independent deployments can coexist on a PostgreSQL cluster. The CLI SHALL persist the deployment's managed role identities, reject conflicting or privileged existing role names instead of silently adopting them, and change only blkit-managed object ownership and grants plus the documented dedicated-database access defaults. Existing runtime passwords SHALL NOT be reset by initialization reruns or migrations. Required ownership transfer or broad access-default changes during legacy adoption SHALL be documented before rollout; unrelated application objects SHALL NOT be dropped, re-owned, or granted to blkit runtime roles.

#### Scenario: Role-name collision
- **WHEN** initialization encounters a matching role name that is not recorded as belonging to the deployment
- **THEN** it refuses to reuse or modify that role and identifies the naming conflict without revealing credentials

#### Scenario: Upgrade does not rotate credentials
- **WHEN** an administrator repeats initialization or applies migrations with already provisioned roles
- **THEN** existing login passwords remain valid and only the intended managed schema and grant changes are applied
