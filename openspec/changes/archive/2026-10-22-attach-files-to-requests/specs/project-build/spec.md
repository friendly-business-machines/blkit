# Spec Delta

## ADDED Requirements

### Requirement: Generated API and worker projects support matching attachment stores
Generated local API+worker binaries SHALL initialize a persistent filesystem attachment store by default. Generated remote API and worker roles, including API-only, worker-only, split, and combined remote targets, SHALL require configuration for the same supported shared cloud storage backend at startup without adding a PostgreSQL dependency to local builds, an HTTP-server dependency to worker-only builds, or any attachment-storage dependency to a reusable `crate` target. Storage selection SHALL NOT require editing generated source or change the existing build-target vocabulary.

#### Scenario: Local generated binary
- **WHEN** a generated local `api-worker` or compatibility `server` target is built and started without attachment-specific settings
- **THEN** it uses a persistent directory alongside its database and can accept and subsequently read attachments without cloud credentials or PostgreSQL

#### Scenario: Split distributed project
- **WHEN** a generated split remote project's API and worker are configured to the same cloud store
- **THEN** the API can accept attachments and the separate worker can read them without either role linking the other's HTTP or execution dependencies

#### Scenario: Cloud storage is required for every distributed role
- **WHEN** a generated API-only, worker-only, split, or combined remote binary starts without valid cloud object-storage configuration
- **THEN** that binary fails startup even if it would otherwise serve JSON-only requests

#### Scenario: Reusable crate
- **WHEN** a generated `crate` target is built
- **THEN** it does not depend on the attachment storage integration merely because other build targets support attachments
