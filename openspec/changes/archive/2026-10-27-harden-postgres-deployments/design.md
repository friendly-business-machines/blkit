# Design

## Context

See `proposal.md` for motivation. Relevant observed implementation:

- `crates/blkit-core/src/postgres_store.rs` builds a `deadpool-postgres` pool with `NoTls`, checks out a session, then executes a large DDL/backfill batch. The batch includes seven application tables, indexes, notification functions/triggers, and a `queued_at_ms` backfill. There is no PostgreSQL migration ledger.
- `crates/blkit-core/src/distributed.rs::run_listener` creates another `NoTls` connection from a stored PostgreSQL configuration, outside the API request pool; reconnect repeats that path.
- `crates/blkit-transpiler/src/project.rs` generates three remote source templates. All take a positional URL; combined mode authenticates both pools with it. Split API generation reuses the API template.
- API and worker both reconcile deadlines, expired claims, and due waits. Both update `instances` and request status; table-wide read/write grants would erase meaningful separation. The notification trigger reads `instance_interests` under the caller's privileges.
- PostgreSQL tests across core, CLI generated-project tests, and `tools/dev-harness` rely on connect-time initialization and plaintext Testcontainers fixtures. Local `LocalStore::open` uses in-process Turso and is a separate lifecycle.
- Current distributed specs explicitly promise schema setup at startup and preservation of positional arguments. The deltas replace those promises rather than leaving conflicting requirements in place.

## Goals / Non-Goals

**Goals:**

- One secure connection policy shared by pools, dedicated listeners, and CLI administration.
- CLI-owned PostgreSQL DDL/migration assets; core retains only connectivity, runtime operations, and a read-only schema compatibility contract.
- Database-enforced separation between API and worker permissions, with no runtime schema ownership.
- Explicit, testable credential source precedence and safe diagnostics for both generated binaries and administration.
- A non-destructive upgrade path from this repository's currently emitted, unversioned PostgreSQL schema.

**Non-Goals:**

- Creating a PostgreSQL server or database, configuring `postgresql.conf`/`pg_hba.conf`, or replacing a DBA. Commands operate inside an existing dedicated application database.
- Tenant/user/instance-specific RLS, isolation between individual worker replicas, HTTP authentication/TLS, client-certificate authentication, new secret-manager integrations, automatic credential rotation, or automatic downgrade migrations.
- Moving local Turso initialization/migration out of core, or changing build targets, attachment storage, pool sizes/timeouts, task limits, or lease semantics.

The dedicated-database assumption, setting names, role naming, and exact-version compatibility below are proposed design defaults, not additional decisions attributed to the user. The user explicitly requested CLI creation of roles and grants.

## Decisions

### 1. Shared connection policy, verified TLS unless deliberately disabled

Add a small remote-persistence connection/configuration module in core. It resolves a sanitized PostgreSQL configuration plus immutable trust/transport options that both pooled connections and the dedicated listener consume. The CLI uses the same connector; migration SQL stays in the CLI. Keep `PostgresStore::connect(url)` and `connect_with_pool_size(url, max)` available to Rust consumers, with secure defaults and no implicit schema setup; add an explicit options path for CA trust and test plaintext. Avoid reading global environment variables inside low-level store constructors: generated entry points and the CLI resolve environment/file settings once and pass options down. Direct Rust consumers can still supply strings programmatically; the no-argument-credentials contract applies to executables.

Proposed settings:

| Setting | Contract |
| --- | --- |
| `BLKIT_POSTGRES_CA_FILE` | Optional PEM CA bundle added to platform roots; fail if explicitly configured but unreadable, empty, or malformed. |
| `BLKIT_POSTGRES_INSECURE_DEV` | Absent or `false`: verified TLS. Exactly `true`: deliberately disable transport TLS and emit an insecure-development warning. Other values fail. |

Use rustls with native/platform roots and a compatible PostgreSQL adapter, preferably `tokio-postgres-rustls`, rather than inventing a TLS transport. Select dependency versions/features against the installed tokio-postgres/rustls versions at implementation time. Require TLS in the driver and enforce certificate-chain, validity, and hostname checks in the connector. `tokio-postgres` exposes `Disable`, `Prefer`, and `Require`, not libpq's complete verification modes; `Require` alone is not the complete trust policy. Reject explicit URL `sslmode=disable` or `prefer` in secure mode rather than allowing a URL to weaken the policy. The connector supplies required TLS when the URL has no mode. In deliberate plaintext mode reject conflicting secure URL requirements or CA settings instead of pretending to apply them. Reject Unix-socket-only connection configurations in secure mode because there is no server-hostname-verified TLS transport; use a hostname or explicitly select the development override.

Store connector state alongside the pool so listener reconnect and recycled sessions cannot revert to `NoTls`. Do not add an accept-invalid-certificate switch. No runtime certificate reload is required: restart after changing CA files. Retain the dedicated listener's separately budgeted session and existing reconnect/resync ordering.

**Alternatives:** opportunistic TLS or encryption without hostname verification does not meet the security goal. An OS-only trust mechanism is insufficient for minimal containers; a PEM bundle permits enterprise deployment without rebuilding. A compile-time dev mode is not appropriate for testing the exact production binary.

### 2. Explicit environment and file configuration, no secret argv values

Use these role-specific settings, with no generic URL fallback:

| Setting | Consumer |
| --- | --- |
| `BLKIT_API_DATABASE_URL` or `BLKIT_API_DATABASE_URL_FILE` | API runtime; initial API credential provisioning by the CLI |
| `BLKIT_WORKER_DATABASE_URL` or `BLKIT_WORKER_DATABASE_URL_FILE` | Worker runtime; initial worker credential provisioning by the CLI |
| `BLKIT_ADMIN_DATABASE_URL` or `BLKIT_ADMIN_DATABASE_URL_FILE` | `blkit db` only |

A `_FILE` contains one UTF-8 PostgreSQL URL, allowing one trailing LF/CRLF from a mounted secret; reject an otherwise empty value. Passwords with special characters must be URL-encoded. Do not log URL contents. Only load settings required by the selected command/role; generated runtimes never load administrative credentials. Combined mode resolves two URLs and requires distinct users and the same parsed host/port/database endpoint (accept harmless URL spelling differences, reject differing endpoints rather than guessing whether aliases point to the same database). API and worker URLs used for initial provisioning must likewise identify the configured administrative database endpoint and expected managed login names.

All remote executables and `blkit db` accept an optional `--env-file PATH`. Parse a conventional dotenv file as data using an established parser, without shell execution, command substitution, automatic file search, or global environment mutation. Relative dotenv paths and paths contained in configuration are relative to the process working directory. Form the effective configuration by overlaying process environment on dotenv values by setting name; then reject any required direct-value/`_FILE` pair where both names are set, even if they came from different sources. Validate explicitly selected dotenv files before connecting, including when environment values would otherwise suffice. Treat empty required settings as invalid. CA and insecure-dev options use the same overlay. Existing pool and attachment configuration must see the same effective values when a dotenv file is selected; the loader must feed existing configuration entry points without silently executing `set_var` in the asynchronous runtime.

New generated usage preserves the order/defaults of non-secret operands after removing the URL:

- `api [--env-file PATH] [BIND_ADDRESS]`
- `worker [--env-file PATH] [MAX_TASKS] [LEASE_MS]`
- `api-worker [--env-file PATH] [MAX_TASKS] [LEASE_MS] [BIND_ADDRESS]`

Local Turso usage and local database-file operands remain unchanged. Validate all arguments and required configuration before database access, and detect legacy URL-shaped operands even when they could otherwise occupy a string-valued bind slot. Parser errors must not echo raw unexpected arguments, invalid bind values, URLs, SQL containing passwords, or driver details containing secrets. This includes help/usage errors, tracing, `Debug` output, and boxed-error termination paths. Use setting names and safe error categories for credential-bearing boundaries while retaining safe operation context.

**Alternatives:** shell-sourcing `.env` executes arbitrary code; implicit `.env` discovery risks selecting unintended credentials. Separate URL settings are smaller than adding a new host/user/password configuration language and prevent combined deployments from defaulting to one privileged login.

### 3. Three managed database roles with explicit object/column grants

`blkit db init [--env-file PATH] [--role-prefix NAME]` defaults the prefix to `blkit`. Persist the selected prefix and role names in CLI-managed metadata; later `migrate` reads them. Reject invalid/overlong names and any prefix mismatch. Create `${prefix}_owner` as `NOLOGIN` and `${prefix}_api`/`${prefix}_worker` as login roles. Set passwords from the supplied role URLs only when creating new logins. Do not generate or print passwords. Changing an existing password is a separate DBA operation, not a side effect of migration. Grant no runtime membership in the owner role, administrative identities, or the opposite runtime role. Verify all existing managed role attributes/memberships before reconciliation and fail on unexpected elevation rather than silently adopting it. Unexpected existing cluster role names are collisions unless the deployment metadata already records ownership; do not seize roles from another database.

The administrator needs authority to create these roles, assume/manage the owner, transfer current blkit object ownership during legacy adoption, and apply grants. Require these capabilities and report missing authority; do not require the application to run as a superuser. For the initial scope, retain existing objects in `public` rather than relocating persisted deployments into a new schema. Set a trusted search path, explicitly qualify security-sensitive object references, and remove `PUBLIC` CREATE access to `public` in the documented dedicated database. Revoke `PUBLIC` rights on blkit objects and broad function defaults for the managed owner; grant database CONNECT and schema USAGE to the two runtime logins, but not CREATE. Do not sweep grants, ownership, or default privileges belonging to unrelated object owners. Existing public-schema deployments must review this database-wide CREATE revocation before adoption.

Minimum grant matrix (all objects owned by the non-login owner; no runtime grant option):

| Managed object | API | Worker |
| --- | --- | --- |
| `instances` | SELECT; INSERT only admission fields; UPDATE only response-resume, cancellation, and reconciliation columns | SELECT; UPDATE only claim/lease, checkpoint, execution outcome, and reconciliation columns; no INSERT or DELETE |
| `input_requests` | SELECT; UPDATE `status`, `response`; no INSERT or DELETE | SELECT; INSERT request identity/prompt/schema fields and UPDATE `status`; no INSERT/UPDATE of response values, no DELETE |
| `instance_attachments` | SELECT and INSERT metadata fields; no UPDATE or DELETE | SELECT only |
| `instance_interests` | SELECT, INSERT, UPDATE expiry, DELETE | SELECT only, required by the existing invoker-security notification trigger |
| `workers` | SELECT only | SELECT, INSERT, UPDATE heartbeat/draining, DELETE for drained unregister |
| `worker_capabilities` | SELECT only | SELECT, INSERT; removal by existing worker-delete cascade |
| `process_policies` | SELECT only | SELECT, INSERT immutable policies; no UPDATE or DELETE |
| Version/deployment metadata | SELECT compatibility metadata only | SELECT compatibility metadata only |
| Notification function/triggers | No ownership or definition changes; retain trigger invocation | Same |

Pin exact column lists to the SQL already used by each role. API must not gain UPDATE of `checkpoint`, typed `input`, or `result`; worker must not gain INSERT on instances or write access to reply `response` values. Shared reconciliation legitimately updates owner/lease, status, attempts, error/retry/deadline bookkeeping and retires requests; do not move it into a new service. Column-level INSERT on input requests is necessary so worker INSERT cannot smuggle a client response. PostgreSQL row-locking clauses can require UPDATE permission even when the query only reads: inspect every lock against this matrix. In particular, `register_worker_with_policies` currently locks an immutable policy with `FOR UPDATE` after `INSERT ... ON CONFLICT DO NOTHING`; replace that unnecessary read lock with a normal immutable read after the conflict-serialized insertion, with concurrency coverage, rather than granting policy UPDATE. Retain locks that fence claims and drain.

The notification function can remain `SECURITY INVOKER`: worker SELECT on interest channels is the necessary read permission, avoiding a new security-definer surface. No RLS or per-worker row-ownership guarantee is added. Explicit role/column privileges reduce blast radius, but do not authorize end users or stop a compromised login from altering other rows within its permitted columns.

Each migration explicitly grants permissions for its new objects/columns. Owner default privileges deny implicit `PUBLIC` function access and do not blanket-grant future runtime writes. Reconcile the exact managed grant policy on reruns; detect and reject inherited elevation. Leave unrelated application objects alone.

**Alternatives:** one read/write login or `GRANT ALL` defeats role separation. A stored-procedure gateway or pervasive RLS would provide finer operation/row enforcement but requires rewriting the runtime data-access model, beyond the requested role permissions. Three roles with direct grants are sufficient; no extra generic group-role framework is needed.

### 4. CLI-owned ordered migrations and a runtime compatibility ledger

Move PostgreSQL DDL, trigger installation, and migration backfills from `PostgresStore` into versioned assets under `crates/blkit-cli`. Add a CLI administration module and a remote core dependency with default features disabled; the distributed CLI still builds without demos, HTTP execution, or local-store requirements for these commands. Do not place migrations behind a core feature that generated executables might accidentally enable. Core knows the supported schema version/identity and how to read compatibility metadata, not how to create it.

Add owner-controlled deployment metadata and migration history (version, immutable migration identity/checksum, applied timestamp). Initially require an exact known schema version/identity at runtime; missing history, mismatched identity, or a newer/older version fails with safe `db init`/`db migrate` guidance before serving HTTP, advertising a worker, or running recovery loops. Compatibility metadata reads are permitted to each login, mutation is not. Future rolling compatibility windows can be introduced explicitly; do not guess that a higher version is safe.

Serialize all schema preparation within a database using an advisory lock scoped to the blkit migration namespace, held on one administrative session. Set finite connection and lock-wait timeouts and report contention. Use transactional PostgreSQL DDL: each migration's changes, ownership/grants, and version record commit together. Initial creation of managed roles and deployment metadata is atomic so failure cannot strand unrecorded roles. On failure, leave the last successful version usable and report failure; do not auto-drop data or apply down migrations. For future multi-step upgrades, earlier committed migrations can remain applied and the next run resumes from recorded history. Validate the entire known history before mutation and validate it again under the lock.

`init` handles empty or already managed compatible databases; a recognized unversioned schema directs the user to `migrate`. `migrate` on an empty database directs the user to `init`. For legacy adoption, recognize the current pre-change schema structurally (all seven tables, expected columns/types/constraints/indexes/triggers), not by the presence of one table. Capture that fixture before changing runtime startup. Reject partial, unexpected, or older unrecognized shapes with guidance rather than treating them as empty. Baseline the recognized schema, transfer only known blkit object ownership, provision missing runtime roles with supplied secrets, and apply the new metadata/security migrations transactionally. Include the currently required `queued_at_ms` backfill in the CLI migration assets, never in runtime connection setup. Preserve all application rows and active checkpoints; schedule migration with writers quiesced.

Existing managed migrations normally need only the admin URL; runtime URLs are required when creating missing login roles, not for every upgrade. Initialization reruns and migrations never reset existing passwords. All grant updates are part of preparation success, not best-effort postprocessing. Avoid dynamic SQL interpolation of identifiers or passwords: validate and quote role identifiers and use correct driver/SQL literal escaping for role-password utility statements; errors must never expose those statements. PostgreSQL server-side statement auditing/redaction remains an administrator responsibility.

**Alternatives:** keeping a core auto-migration feature still ships administrative behavior into applications. A generic migration framework is not needed for a small transactional migration ledger and advisory lock. Inferring arbitrary legacy shapes or silently repairing drift is unsafe; support the observed baseline explicitly.

### 5. Reuse the same preparation path in tests and the development harness

Update all callers that assume `connect` creates tables. Integration fixtures must invoke the CLI administration path (or a test-only helper exercising its implementation), using a known disposable admin connection and explicit plaintext test options. Core tests cannot gain a production dependency on the CLI; prepare their database via the compiled CLI in integration orchestration or CLI-produced test SQL assets supplied by test tooling, not by restoring runtime DDL. Keep migration assets canonical and avoid copy-pasted test schemas. The same setup precedes generated-role and dev-harness startup. Update test command invocations to put URLs in child-process environment/secrets files, not argv.

Use TLS-enabled PostgreSQL fixtures with a test CA and matching/mismatching/expired certificates to test actual handshakes, not just configuration strings. Exercise pool replacement and listener reconnect after TLS failures. Positive permission tests must run as the real managed logins; separate negative SQL tests prove forbidden writes and DDL fail. Cover API/worker shared reconciliation, notification trigger reads, attachment flows, immutable policy publication concurrency, and existing lease/drain/recovery behavior.

Retain local Turso tests and dependency-tree checks. No full test run is needed for this planning-only change; implementation verification includes formatting, clippy, focused security/migration/generated-binary tests, and the serialized full workspace suite.

## Risks / Trade-offs

- [Breaking deployment contract] → Regenerate/rebuild binaries, migrate using the matching CLI, and update launch configuration before restart; no insecure URL-argument compatibility fallback.
- [Plaintext override used in production] → Explicitly named dev setting, no automatic activation, visible warning, and production examples omit it. This is an operator-controlled escape hatch, not a guarantee that production can be detected automatically.
- [Column grants miss an indirect read, lock, or trigger permission] → Test complete runtime paths under actual API/worker identities, including SSE and shared maintenance, not only isolated grant inspection.
- [Managed PostgreSQL limits role/ownership administration] → Report required authority and fail safely; provisioning needs a sufficiently authorized administrator, not runtime elevation.
- [Public schema access defaults affect other users] → Require/document a dedicated database, scope other changes to managed objects, and require review of legacy ownership/default changes before rollout.
- [Core tests need schema setup without depending on CLI] → Introduce test-only orchestration using canonical CLI assets; do not duplicate migration DDL or link it into core runtime features.
- [Exact schema version prevents arbitrary mixed-release rollout] → Use a maintenance window and document it. Roll back the application only if its schema compatibility is known; otherwise restore the database and matching objects from backup.
- [Secret files/environment still accessible to the process and privileged host users] → Use mounted secret permissions and deployment secret management; removing argv exposure does not make a compromised host safe.
- [Server statement logging may capture role-password DDL] → Document secure PostgreSQL logging/audit configuration and never echo provisioning SQL from the client.

## Migration Plan

1. Build/distribute the matching `blkit` CLI separately from the application. Prepare platform or PEM corporate trust and an admin credential secret with required role/ownership authority.
2. Back up PostgreSQL and its corresponding private attachment objects; inventory the known legacy schema, existing owners/grants, role-name availability, and dedicated-database assumptions. Schedule a maintenance window; stop admissions, drain workers, and stop API/worker writers before adoption.
3. Select a collision-free role prefix and supply strong API and worker URL secrets with the expected managed usernames, same endpoint/database, and encoded passwords. Run `blkit db init` for a new database or `blkit db migrate` for the recognized existing deployment. Verify successful version/grant installation; do not run ad-hoc README SQL as a second migration system.
4. Regenerate and build applications. Replace positional URL launch arguments with role-specific environment/secret-file inputs or an explicitly selected dotenv file; combined mode receives two distinct identities. Configure the same verified trust policy for admin, pools, and listener. Set plaintext only for disposable dev/test fixtures.
5. Start the API and workers with their operational logins; confirm version checks, advertisement, admission/execution, requests/replies, attachments, SSE, and recovery. Remove admin credentials from application environments and retire broad legacy runtime grants/logins only after confirming they are not shared or still used.
6. For a failed transaction, correct the administrative issue and rerun; the failed version is not recorded. After committed schema changes, roll forward where possible. A rollback requiring incompatible old binaries restores a matching PostgreSQL backup and consistent attachment snapshot; no destructive automated downgrade is offered.

## Source Notes

- PostgreSQL TLS verification rationale: https://www.postgresql.org/docs/17/libpq-ssl.html (`verify-full` verifies trust and server name; opportunistic `prefer` does not enforce security).
- Driver transport modes: https://docs.rs/tokio-postgres/latest/tokio_postgres/config/enum.SslMode.html (`Disable`, `Prefer`, `Require`; certificate/hostname policy belongs in the connector).
- Existing-compatible adapter approach: https://docs.rs/tokio-postgres-rustls/latest/tokio_postgres_rustls/ (rustls PostgreSQL connector, native roots support). Exact dependency versions are an implementation detail to verify before adding dependencies.
