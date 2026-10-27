# Canonical PostgreSQL assets

`0001_initial.sql` is migration version **1**, identity **blkit-postgres-v1**.
It contains the current application schema, notification triggers, queued-time
backfill, and compatibility/deployment metadata tables. Ordered subsequent
migrations belong here, not in core or test fixtures. The initial application
DDL intentionally retains the constructor's definitions (including function
body whitespace, for catalog comparison). Only the standalone CLI's `blkit db init`
and `blkit db migrate` prepare production databases from these assets.
Runtime startup validates prepared schema compatibility read-only; it never
creates, migrates, backfills, provisions roles, or repairs grants.

`0001_initial.sha256` records SHA-256 of the exact SQL file bytes, including
its trailing newline. Verify from this directory with:

```sh
sha256sum --check 0001_initial.sha256
```

## Metadata interface

- `public.blkit_schema_migrations`: `version BIGINT PRIMARY KEY`, immutable
  `identity TEXT NOT NULL`, `checksum TEXT NOT NULL` (lowercase hexadecimal
  SHA-256 of the migration bytes), and `applied_at TIMESTAMPTZ NOT NULL`
  defaulting to `clock_timestamp()`. The runner records each applied migration
  in the same transaction as its DDL. CLI administration validates the entire
  history and checksums before modifying the deployment.
- `public.blkit_deployment`: one `singleton BOOLEAN PRIMARY KEY CHECK (singleton)`
  row, with non-null text `role_prefix`, `owner_role`, `api_role`, `worker_role`.
  CLI administration owns the metadata and installs runtime SELECT-only access.
- Core's remote-only `postgres_schema::{SUPPORTED_SCHEMA_VERSION,
  SUPPORTED_SCHEMA_IDENTITY, SUPPORTED_SCHEMA_CHECKSUM}` identifies exact runtime
  compatibility without importing migration assets or administrative implementation.

## Test-only preparation

`tests/support/postgres.rs::prepare_fresh_database(&mut tokio_postgres::Client)`
executes the canonical SQL and records version/deployment metadata atomically.
It requires a fresh **disposable** database and an administrative test connection;
callers explicitly use `NoTls` with Testcontainers. Core and dev-harness tests
can import the helper with `#[path]`; no production dependency on the CLI or
migration feature is necessary. `tests/postgres_preparation.rs` is a shared
contract exercised by all three packages.

The helper records fixed `blkit_test_*` names but does **not** provision roles,
apply grants, acquire administration locks, or validate existing deployments.
Its successful use is not evidence of managed-role readiness. Production CLI
administration performs provisioning, locking, validation, and grant reconciliation;
the helper is not a production init/migrate entry point. Frozen
`tests/fixtures/postgres-legacy` assets remain an independent historical oracle,
never an input to fresh preparation.
