//! CLI-owned managed-role provisioning. No runtime schema initialization lives here.

use blkit_core::{
    postgres_config::{DatabaseRole, EffectivePostgresConfig, RoleDatabase},
    postgres_connection::PostgresConnectionOptions,
};
use tokio_postgres::Transaction;

mod migrations;
mod privileges;
pub use migrations::{Preparation, PreparationOutcome, prepare_database};
pub use privileges::reconcile_managed_privileges;

/// Validated deployment identity; names are ASCII and never exceed PostgreSQL's
/// 63-byte identifier limit. This contains no credentials.
#[derive(Debug)]
pub struct ManagedRoles {
    prefix: String,
    owner: String,
    api: String,
    worker: String,
}

impl ManagedRoles {
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn api(&self) -> &str {
        &self.api
    }
    pub fn worker(&self) -> &str {
        &self.worker
    }

    /// Validates a prefix and derives names without IO. Construction does not provision
    /// or adopt roles, or certify persisted deployment ownership; preparation still
    /// checks database identity, collisions, memberships and privileges.
    ///
    /// # Errors
    /// Rejects prefixes outside the managed lowercase ASCII identifier policy.
    pub fn from_prefix(prefix: &str) -> Result<Self, ProvisionError> {
        // _worker is the longest suffix (7 bytes); never rely on server truncation.
        if prefix.is_empty()
            || prefix.len() > 56
            || !prefix.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || prefix.starts_with("pg_")
        {
            return Err(failure(
                "role prefix must be 1-56 lowercase ASCII letters, digits or underscores, start with a letter or underscore, and not start with pg_",
            ));
        }
        Ok(Self {
            prefix: prefix.into(),
            owner: format!("{prefix}_owner"),
            api: format!("{prefix}_api"),
            worker: format!("{prefix}_worker"),
        })
    }
}

/// Redacted boundary error. Raw driver errors, SQL and secrets are deliberately
/// discarded, including their source chains; Debug is safe without a logger.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ProvisionError(String);

fn failure(message: &str) -> ProvisionError {
    ProvisionError(message.into())
}

/// Provisions only managed roles and their singleton deployment identity.
///
/// Composition contract for grants/migrations:
/// - The caller serializes preparation, validates schema/history, and installs the
///   canonical `postgres/0001_initial.sql` metadata tables before calling this.
/// - Use the same administrative session/transaction for schema, this helper,
///   ownership/grants and history. Commit only after ALL succeed; roll back on ANY
///   error (including validation errors). This helper never commits or starts a
///   separate connection. Search path is transaction-locally trusted afterwards.
/// - `Some(prefix)` requires that identity (init supplies its default `blkit`);
///   `None` uses persisted identity for migrate, or `blkit` on first preparation.
/// - Pass one resolved admin and the shared options from the same configuration.
///   Only a missing login's URL/FILE is resolved. Supplied plaintext is converted
///   with postgres-protocol's SCRAM helper; existing passwords are untouched.
/// - A newly created owner is granted to a non-superuser administrator so it can
///   assume that owner. Existing membership is not repaired; later object-management
///   operations must also succeed under the caller's administrative authority.
/// - The CLI must retain core logging's pre-sink tokio_postgres diagnostic suppression;
///   this helper logs nothing. Server-side statement auditing is a DBA responsibility.
///
/// # Errors
/// Rejects prefix/identity drift, role collisions, unexpected attributes or ANY
/// outward membership of a managed role (even NOINHERIT permits SET ROLE), invalid
/// required credentials, or missing actual SQL authority. Does not adopt or repair
/// elevated roles. Ownership transfer and final object grants belong to the caller.
pub async fn provision_managed_roles(
    transaction: &Transaction<'_>,
    requested_prefix: Option<&str>,
    settings: &EffectivePostgresConfig,
    admin: &RoleDatabase,
    options: &PostgresConnectionOptions,
) -> Result<ManagedRoles, ProvisionError> {
    let requested = requested_prefix
        .map(ManagedRoles::from_prefix)
        .transpose()?;
    transaction
        .batch_execute("SET LOCAL search_path = pg_catalog, pg_temp")
        .await
        .map_err(|_| failure("cannot establish trusted provisioning search path"))?;
    let metadata = read_deployment(transaction).await?;
    let roles = if metadata.is_empty() {
        requested.unwrap_or(ManagedRoles::from_prefix("blkit")?)
    } else {
        let mismatch = || {
            failure("deployment identity mismatch; use the persisted role prefix and derived names")
        };
        if metadata.len() != 1 {
            return Err(mismatch());
        }
        let row = &metadata[0];
        if !row.try_get::<_, bool>(0).map_err(|_| mismatch())? {
            return Err(mismatch());
        }
        let prefix: &str = row.try_get(1).map_err(|_| mismatch())?;
        let stored = ManagedRoles::from_prefix(prefix).map_err(|_| mismatch())?;
        if requested.is_some_and(|roles| roles.prefix != stored.prefix)
            || row.try_get::<_, &str>(2).map_err(|_| mismatch())? != stored.owner
            || row.try_get::<_, &str>(3).map_err(|_| mismatch())? != stored.api
            || row.try_get::<_, &str>(4).map_err(|_| mismatch())? != stored.worker
        {
            return Err(mismatch());
        }
        stored
    };
    let names = [roles.owner(), roles.api(), roles.worker()];
    let identity = transaction
        .query_one("SELECT current_user::pg_catalog.text, session_user::pg_catalog.text, r.rolsuper FROM pg_catalog.pg_roles r WHERE r.rolname = current_user", &[])
        .await
        .map_err(|_| failure("cannot verify administrative session authority"))?;
    let current: &str = identity.get(0);
    if current != admin.username()
        || identity.get::<_, &str>(1) != current
        || names.contains(&current)
    {
        return Err(failure(
            "provisioning requires the resolved administrative session identity",
        ));
    }
    let superuser: bool = identity.get(2);
    let mut exists = [false; 3];
    // Validate every existing role before creating anything or reading new secrets.
    for (index, name) in names.iter().enumerate() {
        exists[index] =
            verify_managed_role(transaction, name, index != 0, !metadata.is_empty()).await?;
    }
    let api_verifier = if exists[1] {
        None
    } else {
        Some(new_login_verifier(
            settings,
            DatabaseRole::Api,
            roles.api(),
            admin,
            options,
        )?)
    };
    let worker_verifier = if exists[2] {
        None
    } else {
        Some(new_login_verifier(
            settings,
            DatabaseRole::Worker,
            roles.worker(),
            admin,
            options,
        )?)
    };
    for (index, verifier) in [None, api_verifier.as_deref(), worker_verifier.as_deref()]
        .into_iter()
        .enumerate()
    {
        if exists[index] {
            continue;
        }
        let login = verifier.map_or_else(
            || "NOLOGIN".into(),
            |verifier| format!("LOGIN PASSWORD {}", password_literal(verifier)),
        );
        let sql = format!(
            "CREATE ROLE {} {login} NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS",
            quote_identifier(names[index]),
        );
        transaction.batch_execute(&sql).await.map_err(|_| failure(
            "cannot create managed role; administrator needs role-creation authority and valid supplied credentials",
        ))?;
    }
    if !exists[0] && !superuser {
        transaction
            .batch_execute(&format!(
                "GRANT {} TO {}",
                quote_identifier(roles.owner()),
                quote_identifier(current),
            ))
            .await
            .map_err(|_| failure("administrator lacks authority to manage the new owner role"))?;
    }
    // Exercise authority, not just CREATEROLE/superuser attribute heuristics.
    transaction.batch_execute(&format!("SET LOCAL ROLE {}", quote_identifier(roles.owner())))
        .await.map_err(|_| failure("administrator lacks authority to assume the managed owner; DBA membership review required"))?;
    transaction
        .batch_execute(&format!("SET LOCAL ROLE {}", quote_identifier(current)))
        .await
        .map_err(|_| failure("cannot restore administrative session authority"))?;
    if metadata.is_empty() {
        transaction.execute(
            "INSERT INTO public.blkit_deployment (singleton, role_prefix, owner_role, api_role, worker_role) VALUES (TRUE, $1, $2, $3, $4)",
            &[&roles.prefix(), &roles.owner(), &roles.api(), &roles.worker()],
        ).await.map_err(|_| failure("cannot persist deployment identity; check administrative metadata authority"))?;
    }
    Ok(roles)
}

// Ownership transfer removes a NOINHERIT administrator's implicit metadata access.
// Read as its catalog owner, then restore the caller before checking admin identity.
async fn read_deployment(
    transaction: &Transaction<'_>,
) -> Result<Vec<tokio_postgres::Row>, ProvisionError> {
    let identity = transaction.query_one(
        "SELECT current_user::pg_catalog.text, pg_catalog.pg_get_userbyid(c.relowner)::pg_catalog.text, pg_catalog.has_table_privilege(c.oid, 'SELECT') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname='blkit_deployment' AND c.relkind='r'",
        &[],
    ).await.map_err(|_| failure("cannot inspect deployment metadata owner; validate CLI-managed schema"))?;
    let caller = quote_identifier(identity.get(0));
    let owner = quote_identifier(identity.get(1));
    let assume_owner = !identity.get::<_, bool>(2);
    if assume_owner {
        transaction
            .batch_execute(&format!("SET LOCAL ROLE {owner}"))
            .await
            .map_err(|_| {
                failure("administrator lacks authority to assume deployment metadata owner")
            })?;
    }
    let metadata = transaction
        .query("SELECT singleton, role_prefix, owner_role, api_role, worker_role FROM public.blkit_deployment", &[])
        .await
        .map_err(|_| failure("cannot read deployment identity; validate CLI-managed metadata and administrative authority"))?;
    if assume_owner {
        transaction
            .batch_execute(&format!("SET LOCAL ROLE {caller}"))
            .await
            .map_err(|_| failure("cannot restore administrative identity after metadata read"))?;
    }
    Ok(metadata)
}

// Shared with grants so a stale ManagedRoles cannot silently adopt role elevation.
async fn verify_managed_role(
    transaction: &Transaction<'_>,
    name: &str,
    login: bool,
    recorded: bool,
) -> Result<bool, ProvisionError> {
    let row = transaction.query_opt(
        "SELECT rolcanlogin, rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls OR rolinherit FROM pg_catalog.pg_roles WHERE rolname = $1",
        &[&name],
    ).await.map_err(|_| failure("cannot inspect managed role attributes"))?;
    let Some(row) = row else { return Ok(false) };
    if !recorded {
        return Err(failure(&format!(
            "cluster role name collision: {name}; choose another role prefix"
        )));
    }
    if row.get::<_, bool>(0) != login || row.get::<_, bool>(1) {
        return Err(failure(&format!(
            "unexpected managed role attributes: {name}; DBA review required"
        )));
    }
    let membership: bool = transaction.query_one(
        "SELECT EXISTS (SELECT FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles r ON r.oid = m.member WHERE r.rolname = $1)",
        &[&name],
    ).await.map_err(|_| failure("cannot inspect managed role memberships"))?.get(0);
    if membership {
        return Err(failure(&format!(
            "unexpected managed role membership: {name}; DBA review required"
        )));
    }
    Ok(true)
}

fn new_login_verifier(
    settings: &EffectivePostgresConfig,
    role: DatabaseRole,
    expected_user: &str,
    admin: &RoleDatabase,
    options: &PostgresConnectionOptions,
) -> Result<String, ProvisionError> {
    let database = settings
        .resolve_role(role, options)
        .map_err(|error| failure(&error.to_string()))?;
    let setting = match role {
        DatabaseRole::Api => "BLKIT_API_DATABASE_URL",
        DatabaseRole::Worker => "BLKIT_WORKER_DATABASE_URL",
        DatabaseRole::Admin => unreachable!("only runtime logins are created"),
    };
    if database.username() != expected_user {
        return Err(failure(&format!(
            "{setting} must use the expected managed login username"
        )));
    }
    admin
        .ensure_same_database(&database)
        .map_err(|error| failure(&error.to_string()))?;
    database
        .password()
        .filter(|password| !password.is_empty() && !password.contains(&0))
        // Treat even verifier-shaped URL secrets as literal passwords, never as
        // PostgreSQL's pre-encrypted PASSWORD input. Existing logins never reach here.
        .map(postgres_protocol::password::scram_sha_256)
        .ok_or_else(|| {
            failure(&format!(
                "{setting} must supply a nonempty password without NUL for new login creation"
            ))
        })
}

// Role names are validated above; the administrative identifier comes from PostgreSQL.
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

// Explicit escape literals work with either standard_conforming_strings setting.
fn password_literal(password: &str) -> String {
    format!("E'{}'", password.replace('\\', "\\\\").replace('\'', "''"))
}
