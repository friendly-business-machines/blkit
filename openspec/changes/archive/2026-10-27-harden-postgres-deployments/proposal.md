# Proposal

## Why

Remote binaries currently receive PostgreSQL URLs on the command line, connect without TLS, and create or alter shared tables during startup. This exposes credentials and forces application processes to hold administrative privileges rather than permissions limited to their API or worker responsibilities.

## What Changes

- **BREAKING**: Require certificate- and hostname-verified PostgreSQL TLS by default for runtime pools, notification listeners, and administrative connections. Support deployment-supplied corporate CA certificates and an explicit, warned plaintext override for development/testing only; never silently downgrade.
- **BREAKING**: Remove PostgreSQL URLs, usernames, and passwords from generated-binary arguments. Load database configuration from environment variables, an explicitly selected dotenv file, or mounted secret files, with deterministic precedence and redacted diagnostics.
- Introduce separate API and worker database identities with narrowly scoped grants. Combined remote executables still use separate credentials and pools for their two roles.
- Add `blkit db init` and `blkit db migrate` to `blkit-cli`. The CLI creates managed roles, provisions supplied runtime credentials, applies grants, and owns ordered, versioned PostgreSQL schema migrations using separate administrative credentials.
- **BREAKING**: Remote runtime startup validates an already prepared, compatible schema without creating, altering, migrating, or backfilling it. Existing PostgreSQL deployments must migrate and provision roles before deploying regenerated binaries.
- Preserve automatic local Turso initialization/migration in `blkit-core` for local all-in-one deployments and preserve backend/role dependency isolation.

## Capabilities

### New Capabilities

- `postgres-administration`: CLI-owned PostgreSQL bootstrap, managed role/credential provisioning, grants, and safe, repeatable schema upgrades.

### Modified Capabilities

- `distributed-execution`: Verified TLS on every connection, shared TLS policy on reconnect, and connect-time compatibility checks instead of runtime schema mutation; remove the conflicting positional-argument preservation promise while retaining pool limits.
- `project-build`: Generated remote binaries use environment/file-only database configuration, role-separated identities, and no PostgreSQL migration implementation, while local Turso behavior is unchanged.
- `cli-experience`: Discoverable database administration subcommands and safe configuration/error handling, without changing existing transpilation/update commands.

## Impact

- Runtime: `crates/blkit-core/src/postgres_store.rs`, the dedicated listener in `distributed.rs`, feature/dependency declarations, and PostgreSQL fixtures throughout core and integration tests.
- Generation: remote source templates and generated manifests in `crates/blkit-transpiler/src/project.rs`; API-only, worker-only/legacy worker, split, and combined-remote output contracts.
- Administration: `crates/blkit-cli/src/main.rs`, new CLI-owned schema/migration assets and tests, and a reusable remote connection/configuration path that does not expose migration code to generated executables.
- Deployment examples and dev harness callers under `tools/dev-harness`, generated-binary tests under `crates/blkit-cli/tests`, and README deployment, manual SQL rollout, credential, and connection-budget guidance.
- TLS requires a PostgreSQL-compatible TLS adapter; prefer the rustls ecosystem already present in the workspace dependency lock. Dotenv parsing may require a small established parser rather than executing shell files. No new backend, secret-manager service, tenant model, or HTTP authentication/TLS feature is proposed.
