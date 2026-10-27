//! One administrative session owns preparation, its namespace lock and transactions.
use super::{
    ManagedRoles, ProvisionError, failure, provision_managed_roles, quote_identifier,
    read_deployment, reconcile_managed_privileges,
};
use blkit_core::postgres_config::{DatabaseRole, EffectivePostgresConfig, RoleDatabase};
use blkit_core::postgres_connection::PostgresConnectionOptions;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tokio_postgres::{Client, Transaction};

// PostgreSQL session advisory locks are database-local. ASCII "blkit-db" namespace.
const PREPARATION_LOCK: i64 = 0x626c6b69742d6462;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const MANAGED_TABLES: &[&str] = &[
    "instances",
    "input_requests",
    "instance_attachments",
    "instance_interests",
    "workers",
    "worker_capabilities",
    "process_policies",
    "blkit_deployment",
    "blkit_schema_migrations",
];

struct Migration {
    version: i64,
    identity: &'static str,
    sql: &'static str,
    checksum: &'static str,
}

// Extend in order with explicit transactional steps, never arbitrary supplied SQL.
const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    identity: "blkit-postgres-v1",
    sql: include_str!("../../postgres/0001_initial.sql"),
    checksum: include_str!("../../postgres/0001_initial.sha256"),
}];

/// Init checks its explicit/default `blkit` prefix. Migrate uses persisted identity
/// on managed reruns (an explicit prefix must match); legacy adoption selects the
/// supplied prefix or `blkit`. Operators must quiesce legacy writers first.
#[derive(Clone, Copy, Debug)]
pub enum Preparation<'a> {
    Init { role_prefix: Option<&'a str> },
    Migrate { role_prefix: Option<&'a str> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationOutcome {
    /// Committed fresh empty-database initialization.
    Initialized,
    /// Committed recognized unversioned baseline to v1 adoption.
    Migrated,
    /// Validated managed v1; reconciled grants without rewriting history.
    AlreadyCurrent,
}

/// CLI-owned preparation using only the supplied configuration snapshot.
///
/// Resolves BLKIT_ADMIN_DATABASE_URL[_FILE] and shared TLS options; runtime secrets
/// are resolved only for missing logins. Prints the shared insecure-dev warning once
/// to stderr after admin/transport validation; callers retain pre-sink driver-log
/// suppression. No raw driver errors,
/// SQL, endpoints or credentials are returned or logged by this runner.
///
/// A 10s whole-connect deadline includes handshake/authentication; a database-local
/// session advisory lock ("blkit-db") has a 5s wait. Individual PostgreSQL lock
/// waits are capped at 1s so blocked backends can observe a disconnected client.
/// The lock spans validation and every preparation commit. History is checked
/// before locking and again under it.
/// One transaction commits fresh schema or recognized legacy adoption, roles,
/// ownership/grants, deployment and history together; reruns reconcile policy
/// without rotating existing passwords.
/// The driver is polled here, NOT spawned: returning, error or cancellation drops
/// the socket, releasing the session lock and rolling back any open transaction.
///
/// Administrator needs database/public-schema and known-object management, role
/// creation, and SET ROLE authority for the managed owner. NOINHERIT is supported;
/// metadata reads/writes explicitly assume its owner, without granting admin ACLs.
///
/// # Errors
/// Fails closed before policy repair on unknown/partial/unsupported legacy schemas,
/// inconsistent identity/history/checksums/shape, contention, or insufficient actual
/// authority. Migrate on empty databases directs to init; init on recognized legacy
/// directs to migrate. No destructive repair or downgrade is performed.
pub async fn prepare_database(
    settings: &EffectivePostgresConfig,
    preparation: Preparation<'_>,
) -> Result<PreparationOutcome, ProvisionError> {
    validate_assets(MIGRATIONS)?;
    let role_prefix = match preparation {
        Preparation::Init { role_prefix } => Some(role_prefix.unwrap_or("blkit")),
        Preparation::Migrate { role_prefix } => role_prefix,
    };
    if let Some(prefix) = role_prefix {
        ManagedRoles::from_prefix(prefix)?;
    }
    let options = settings
        .connection_options()
        .map_err(|e| failure(&e.to_string()))?;
    let admin = settings
        .resolve_role(DatabaseRole::Admin, &options)
        .map_err(|e| failure(&e.to_string()))?;
    if let Some(warning) = options.warning() {
        eprintln!("{warning}");
    }
    let (mut client, driver) = timeout(CONNECT_TIMEOUT, admin.connection().connect()).await
        .map_err(|_| failure("BLKIT_ADMIN_DATABASE_URL: administrative connection deadline exceeded"))?
        .map_err(|_| failure("BLKIT_ADMIN_DATABASE_URL: administrative connection failed; check credentials, endpoint and TLS trust"))?;
    tokio::pin!(driver);
    // Keeping the driver in this scope makes cleanup cancellation-safe, without a task guard.
    tokio::select! {
        result = prepare_session(&mut client, settings, &admin, &options, preparation) => result,
        _ = &mut driver => Err(failure("administrative session failed; check database availability and transport")),
    }
}

fn validate_assets(migrations: &[Migration]) -> Result<(), ProvisionError> {
    for (index, migration) in migrations.iter().enumerate() {
        if migration.version != index as i64 + 1
            || migration.checksum.split_whitespace().next() != Some(digest(migration.sql).as_str())
        {
            return Err(failure(
                "embedded migration checksum/order mismatch; use an intact matching CLI build",
            ));
        }
    }
    Ok(())
}

fn digest(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn prepare_session(
    client: &mut Client,
    settings: &EffectivePostgresConfig,
    admin: &RoleDatabase,
    options: &PostgresConnectionOptions,
    preparation: Preparation<'_>,
) -> Result<PreparationOutcome, ProvisionError> {
    // A blocked backend may not observe a closed socket until its lock wait ends.
    client
        .batch_execute("SET lock_timeout = '1s'")
        .await
        .map_err(|_| failure("cannot establish bounded administrative SQL lock waits"))?;
    let tx = client
        .transaction()
        .await
        .map_err(|_| failure("cannot begin preparation validation"))?;
    let validation = validate_database(&tx, preparation).await;
    tx.rollback()
        .await
        .map_err(|_| failure("cannot close preparation validation transaction"))?;
    validation?;
    timeout(LOCK_TIMEOUT, async {
        loop {
            let acquired: bool = client.query_one("SELECT pg_catalog.pg_try_advisory_lock($1)", &[&PREPARATION_LOCK])
                .await.map_err(|_| failure("cannot acquire preparation advisory lock"))?.get(0);
            if acquired { return Ok::<_, ProvisionError>(()); }
            sleep(Duration::from_millis(50)).await;
        }
    }).await.map_err(|_| failure("database preparation lock contention; another administrator is preparing this database; retry after it finishes"))??;

    let tx = client
        .transaction()
        .await
        .map_err(|_| failure("cannot begin preparation transaction"))?;
    let result = apply_preparation(&tx, settings, admin, options, preparation).await;
    let outcome = match result {
        Ok(outcome) => {
            tx.commit().await.map_err(|_| failure("cannot commit database preparation; check administrative authority and database availability"))?;
            outcome
        }
        Err(error) => {
            // Queryable validation errors require rollback too, not just SQL errors.
            let _ = tx.rollback().await;
            return Err(error);
        }
    };
    client
        .execute(
            "SELECT pg_catalog.pg_advisory_unlock($1)",
            &[&PREPARATION_LOCK],
        )
        .await
        .map_err(|_| {
            failure("cannot release preparation advisory lock; administrative session closed")
        })?;
    Ok(outcome)
}

async fn apply_preparation(
    tx: &Transaction<'_>,
    settings: &EffectivePostgresConfig,
    admin: &RoleDatabase,
    options: &PostgresConnectionOptions,
    preparation: Preparation<'_>,
) -> Result<PreparationOutcome, ProvisionError> {
    let state = validate_database(tx, preparation).await?;
    if state != DatabaseState::Managed {
        // Only empty or fully recognized legacy can run the immutable v1 asset.
        // Implicit pg_catalog stays first for lookup, while CREATE targets public.
        tx.batch_execute("SET LOCAL search_path = public, pg_temp")
            .await
            .map_err(|_| failure("cannot select initial schema creation namespace"))?;
        tx.batch_execute(MIGRATIONS[0].sql).await.map_err(|_| {
            failure("cannot apply schema migration; check administrative DDL authority")
        })?;
        validate_shape(tx).await?;
    }
    let prefix = match preparation {
        Preparation::Init { role_prefix } => Some(role_prefix.unwrap_or("blkit")),
        Preparation::Migrate { role_prefix } => role_prefix,
    };
    let roles = provision_managed_roles(tx, prefix, settings, admin, options).await?;
    reconcile_managed_privileges(tx, &roles).await?;
    if state != DatabaseState::Managed {
        tx.batch_execute(&format!(
            "SET LOCAL ROLE {}",
            quote_identifier(roles.owner())
        ))
        .await
        .map_err(|_| failure("cannot assume managed owner to record migration history"))?;
        let migration = &MIGRATIONS[0];
        tx.execute("INSERT INTO public.blkit_schema_migrations(version, identity, checksum) VALUES ($1,$2,$3)",
            &[&migration.version, &migration.identity, &migration.checksum.split_whitespace().next().expect("validated asset")])
            .await.map_err(|_| failure("cannot record successful migration history; transaction rolled back"))?;
    }
    Ok(match state {
        DatabaseState::Empty => PreparationOutcome::Initialized,
        DatabaseState::Legacy => PreparationOutcome::Migrated,
        DatabaseState::Managed => PreparationOutcome::AlreadyCurrent,
    })
}

async fn shape_digest(tx: &Transaction<'_>) -> Result<String, ProvisionError> {
    tx.batch_execute("SET LOCAL search_path = pg_catalog, public, pg_temp")
        .await
        .map_err(|_| failure("cannot establish trusted preparation search path"))?;
    // Internal constraint-trigger names contain OIDs, so check state separately.
    let noncanonical_triggers: bool = tx.query_one(
        "SELECT EXISTS(SELECT FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname=ANY($1) AND t.tgisinternal AND t.tgconstraint<>0 AND t.tgenabled<>'O')",
        &[&MANAGED_TABLES],
    ).await.map_err(|_| failure("cannot inspect internal constraint-trigger state; DBA review required"))?.get(0);
    if noncanonical_triggers {
        return Err(failure(
            "noncanonical internal constraint-trigger state in managed schema; DBA review required before db migrate",
        ));
    }
    let shape: String = tx
        .query_one(include_str!("../../postgres/managed_shape.sql"), &[])
        .await
        .map_err(|_| failure("cannot inspect managed schema; DBA review required"))?
        .get(0);
    Ok(digest(&shape))
}

async fn validate_shape(tx: &Transaction<'_>) -> Result<(), ProvisionError> {
    // Pinned logical definitions from the canonical asset on PostgreSQL 17, not ACLs/OIDs.
    if shape_digest(tx).await? != include_str!("../../postgres/managed_shape.sha256").trim() {
        return Err(failure(
            "unknown or partial managed schema; use the matching CLI and obtain DBA review before db migrate",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DatabaseState {
    Empty,
    Legacy,
    Managed,
}

// Empty means no known/reserved objects; legacy must match the entire frozen shape.
async fn validate_database(
    tx: &Transaction<'_>,
    preparation: Preparation<'_>,
) -> Result<DatabaseState, ProvisionError> {
    tx.batch_execute("SET LOCAL search_path = pg_catalog, public, pg_temp")
        .await
        .map_err(|_| failure("cannot establish trusted preparation search path"))?;
    let present: bool = tx.query_one(
        "SELECT EXISTS(SELECT FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND (c.relname=ANY($1) OR c.relname LIKE 'blkit\\_%' ESCAPE '\\' OR c.relname IN ('input_requests_answered','instance_attachments_start','instance_attachments_reply'))) OR EXISTS(SELECT FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public' AND p.proname='notify_instance_interests')",
        &[&MANAGED_TABLES],
    ).await.map_err(|_| failure("cannot inspect database preparation state"))?.get(0);
    if !present {
        return match preparation {
            Preparation::Init { .. } => Ok(DatabaseState::Empty),
            Preparation::Migrate { .. } => Err(failure("database is not initialized; run db init")),
        };
    }
    let unknown_reserved: bool = tx.query_one(
        "SELECT EXISTS(SELECT FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname LIKE 'blkit\\_%' ESCAPE '\\' AND NOT (c.relname=ANY($1) OR c.relname IN ('blkit_deployment_pkey','blkit_schema_migrations_pkey')))",
        &[&MANAGED_TABLES],
    ).await.map_err(|_| failure("cannot inspect reserved deployment objects"))?.get(0);
    if unknown_reserved {
        return Err(failure(
            "unknown or newer blkit objects; use the matching CLI and obtain DBA review",
        ));
    }
    let shape = shape_digest(tx).await?;
    // Pinned from tests/fixtures/postgres-legacy/schema.sql on PG17, not v1 DDL.
    if shape == include_str!("../../postgres/legacy_shape.sha256").trim() {
        return match preparation {
            Preparation::Init { .. } => Err(failure(
                "recognized unversioned schema; quiesce application writers and run db migrate",
            )),
            Preparation::Migrate { .. } => Ok(DatabaseState::Legacy),
        };
    }
    if shape != include_str!("../../postgres/managed_shape.sha256").trim() {
        return Err(failure(
            "unknown, partial or unsupported blkit schema; use a matching CLI for db migrate or obtain DBA review",
        ));
    }
    let metadata = read_deployment(tx).await?;
    if metadata.len() != 1 {
        return Err(failure(
            "inconsistent deployment metadata; DBA review required",
        ));
    }
    let row = &metadata[0];
    let invalid = || failure("inconsistent deployment metadata; DBA review required");
    let roles =
        ManagedRoles::from_prefix(row.try_get(1).map_err(|_| invalid())?).map_err(|_| invalid())?;
    if !row.try_get::<_, bool>(0).map_err(|_| invalid())?
        || row.try_get::<_, &str>(2).map_err(|_| invalid())? != roles.owner()
        || row.try_get::<_, &str>(3).map_err(|_| invalid())? != roles.api()
        || row.try_get::<_, &str>(4).map_err(|_| invalid())? != roles.worker()
    {
        return Err(invalid());
    }
    let requested_prefix = match preparation {
        Preparation::Init { role_prefix } => Some(role_prefix.unwrap_or("blkit")),
        Preparation::Migrate { role_prefix } => role_prefix,
    };
    if requested_prefix.is_some_and(|prefix| roles.prefix() != prefix) {
        return Err(failure(
            "deployment role prefix mismatch; use the persisted prefix",
        ));
    }
    let owners: bool = tx.query_one(
        "SELECT NOT EXISTS(SELECT FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname=ANY($1) AND pg_catalog.pg_get_userbyid(c.relowner)<>$2) AND NOT EXISTS(SELECT FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public' AND p.proname='notify_instance_interests' AND pg_catalog.pg_get_userbyid(p.proowner)<>$2)",
        &[&MANAGED_TABLES, &roles.owner()],
    ).await.map_err(|_| failure("cannot verify managed schema ownership"))?.get(0);
    if !owners {
        return Err(failure(
            "inconsistent managed ownership; DBA review required",
        ));
    }
    tx.batch_execute(&format!(
        "SET LOCAL ROLE {}",
        quote_identifier(roles.owner())
    ))
    .await
    .map_err(|_| {
        failure("administrator must be able to assume managed owner to validate history")
    })?;
    let history = tx.query("SELECT version, identity, checksum, applied_at IS NOT NULL AND pg_catalog.isfinite(applied_at) AND applied_at <= pg_catalog.clock_timestamp() FROM public.blkit_schema_migrations ORDER BY version", &[]).await
        .map_err(|_| failure("cannot read migration history; check owner authority"))?;
    let invalid = || {
        failure(
            "unknown, newer or inconsistent migration history/checksum; use the matching CLI and obtain DBA review",
        )
    };
    if history.len() != MIGRATIONS.len() {
        return Err(invalid());
    }
    for (row, migration) in history.iter().zip(MIGRATIONS) {
        if row.try_get::<_, i64>(0).map_err(|_| invalid())? != migration.version
            || row.try_get::<_, &str>(1).map_err(|_| invalid())? != migration.identity
            || Some(row.try_get::<_, &str>(2).map_err(|_| invalid())?)
                != migration.checksum.split_whitespace().next()
            || !row.try_get::<_, bool>(3).map_err(|_| invalid())?
        {
            return Err(invalid());
        }
    }
    tx.batch_execute("RESET ROLE")
        .await
        .map_err(|_| failure("cannot restore administrative identity after history validation"))?;
    Ok(DatabaseState::Managed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_asset_bytes_are_checked_not_just_the_sidecar() {
        assert!(validate_assets(MIGRATIONS).is_ok());
        let changed = [Migration {
            sql: "SELECT 'changed';",
            ..MIGRATIONS[0]
        }];
        assert!(validate_assets(&changed).is_err());
        let unordered = [Migration {
            version: 2,
            ..MIGRATIONS[0]
        }];
        assert!(validate_assets(&unordered).is_err());
    }
}
