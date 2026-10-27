# Spec Delta

## ADDED Requirements

### Requirement: Every PostgreSQL connection uses verified TLS by default
PostgreSQL-backed runtimes and CLI administration SHALL require TLS with certificate-chain and server-hostname verification by default, without opportunistic plaintext fallback. The policy SHALL cover every pooled session, replacement session, dedicated notification listener, listener reconnect, and administrative session. Deployments SHALL be able to trust enterprise CAs through the platform trust store or an explicitly configured PEM CA bundle without editing or rebuilding generated code. Missing, unreadable, malformed, or untrusted certificate configuration and hostname mismatches SHALL fail closed with redacted, actionable diagnostics. A connection-string setting SHALL NOT weaken the policy. An explicit development/testing-only plaintext opt-out SHALL be supported and SHALL produce a visible warning; no default, build mode, loopback address, or environment detection SHALL implicitly enable it. There SHALL be no certificate-verification bypass mode.

#### Scenario: Verified default
- **WHEN** a runtime or administrative operation connects without a development override
- **THEN** it succeeds only over TLS to a server whose certificate chain and hostname validate, and a server without TLS support is rejected

#### Scenario: Corporate certificate authority
- **WHEN** the server certificate is signed by an enterprise CA supplied through the trust store or configured PEM bundle
- **THEN** API pools, worker pools, and notification listeners connect without regenerating the application, provided the certificate also matches the requested server hostname

#### Scenario: Invalid trust or weaker URL setting
- **WHEN** a CA bundle is missing, unreadable, or malformed, a certificate is expired/untrusted or names a different host, or a URL requests a weaker TLS mode without the explicit development opt-out
- **THEN** the connection is rejected without plaintext fallback or disclosure of credentials

#### Scenario: Explicit local test setup
- **WHEN** development/testing explicitly enables the plaintext override
- **THEN** connections can use a non-TLS PostgreSQL fixture and startup emits a warning identifying that database transport is insecure without printing credentials

#### Scenario: Pool and listener recovery
- **WHEN** a pooled session or notification listener reconnects after an outage
- **THEN** the replacement uses the same trust and transport policy as startup, and a newly invalid certificate cannot trigger a plaintext downgrade

## MODIFIED Requirements

### Requirement: Remote roles use bounded PostgreSQL connection pools
Each PostgreSQL-backed API and worker role SHALL serve concurrent store operations through a pool with a positive, bounded maximum rather than requiring all operations of that role to wait for one shared client. The default maximum SHALL be four connections per role; a Rust consumer SHALL be able to set an explicit maximum while retaining the existing `PostgresStore::connect` call shape. Generated remote executables SHALL accept independent optional `BLKIT_API_DB_POOL_MAX` and `BLKIT_WORKER_DB_POOL_MAX` positive-integer environment settings for their respective roles. PostgreSQL connection credentials SHALL be supplied only through the environment/file configuration contract, not positional arguments. API-only and split API binaries SHALL use the API budget and API login, worker-only and split worker binaries SHALL use the worker budget and worker login, and a combined remote API/worker binary SHALL use separate API and worker logins and budgets. A role SHALL NOT maintain more simultaneously managed PostgreSQL sessions than its maximum; it SHALL NOT require all allowed connections to be open at startup. Local-persistence and reusable-crate targets SHALL NOT gain a PostgreSQL dependency solely for pooling.

#### Scenario: Concurrent API calls
- **WHEN** an API role has a maximum of two and one store operation is waiting on PostgreSQL while another independent operation can complete on a different session
- **THEN** the second operation can complete without waiting for the first session, and the API role uses no more than two sessions

#### Scenario: Concurrent worker persistence
- **WHEN** a worker role has a maximum of two and one store operation occupies a PostgreSQL session while an independent worker store operation can complete on another
- **THEN** the independent operation can complete without waiting for the first session, and the worker role uses no more than two sessions

#### Scenario: Combined roles under load
- **WHEN** a generated combined remote binary runs concurrent API requests and worker persistence with distinct configured API and worker maxima
- **THEN** the roles draw from independent pools authenticated with their respective logins and bounded by their own maxima; exhausting the worker budget does not consume API connections

#### Scenario: Defaults and overrides
- **WHEN** a generated remote binary starts without pool settings, or starts with positive integer settings for its active roles
- **THEN** each active role uses a default maximum of four or its configured maximum, independently of the worker task limit and CPU count

### Requirement: PostgreSQL pool failures preserve distributed safety
Generated remote executables SHALL reject a nonpositive or malformed pool limit for any active role with a diagnostic that identifies the setting. PostgreSQL-backed store initialization SHALL establish a usable connection and validate an already initialized compatible schema before reporting startup success; it SHALL NOT create or alter schema objects, execute migrations, perform migration backfills, or repair permissions. Missing, older unsupported, newer unsupported, or inconsistent schemas SHALL fail startup with an instruction to use the matching CLI administration workflow. Connection checkout SHALL have a bounded wait and SHALL report an error if capacity or a working connection is unavailable; a failed or discarded connection SHALL NOT permanently consume pool capacity. Pooling SHALL preserve single-connection transaction ownership and the existing claim/drain serialization, lease renewal, checkpoint fencing, cancellation, and recovery semantics; it SHALL NOT acknowledge a database operation that was not committed.

#### Scenario: Invalid configuration or unavailable startup database
- **WHEN** an active remote role is configured with zero or an invalid maximum, or the database is unavailable during store initialization
- **THEN** startup fails visibly instead of running with an unlimited or uninitialized pool

#### Scenario: Saturated or disconnected pool
- **WHEN** all sessions in a role's pool remain checked out beyond its checkout wait or a session is lost
- **THEN** a timed-out checkout reports an error rather than waiting indefinitely or pretending it committed; a lost session is replaced when possible or the operation reports an error, without permanently losing pool capacity

#### Scenario: Claim and drain on different sessions
- **WHEN** a claim and drain overlap on different PostgreSQL sessions for the same worker
- **THEN** a committed drain does not permit a later claim, while already owned claims remain fenced and can renew and finish or release under the existing drain rules

#### Scenario: Contended active worker
- **WHEN** worker persistence and maintenance compete for the bounded worker pool while claims are active
- **THEN** the worker continues to enforce its task limit and existing lease/generation safety rather than double-claiming or committing a stale update

#### Scenario: Schema requires administration
- **WHEN** an application starts against an empty, inconsistent, or unsupported-version PostgreSQL schema
- **THEN** startup fails with a redacted diagnostic pointing to `blkit db init` or `blkit db migrate`, without mutating schema or persisted application data

#### Scenario: Prepared schema without DDL permission
- **WHEN** a remote binary starts with its provisioned operational login on a compatible prepared schema
- **THEN** startup succeeds without schema-owner privileges or any runtime schema mutation
