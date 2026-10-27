use super::{ManagedRoles, ProvisionError, failure, quote_identifier, verify_managed_role};
use tokio_postgres::Transaction;

// These are the columns written by the role-specific SQL in PostgresStore, not
// its legacy general-purpose insert. New migrations must extend grants explicitly.
// (table, API INSERT, API UPDATE, worker INSERT, worker UPDATE)
const TABLES: &[(&str, &str, &str, &str, &str)] = &[
    (
        "instances",
        "id, namespace, version, process, input, status, created_at, updated_at, retry_policy, queued_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms",
        "status, input_resume, updated_at, owner_id, lease_until, terminal_name, wake_at_ms, error, first_failure_at, next_eligible_at",
        "",
        "status, owner_id, lease_until, generation, attempt, input_resume, next_eligible_at, wake_at_ms, first_claim_at_ms, deadline_at_ms, updated_at, input, checkpoint, error, first_failure_at, result, terminal_name",
    ),
    (
        "input_requests",
        "",
        "status, response",
        "instance_id, request_id, prompt, schema",
        "status",
    ),
    (
        "instance_attachments",
        "instance_id, request_id, ordinal, object_key, filename, media_type, byte_length",
        "",
        "",
        "",
    ),
    (
        "instance_interests",
        "instance_id, channel, expires_at",
        "expires_at",
        "",
        "",
    ),
    (
        "workers",
        "",
        "",
        "id, heartbeat_at",
        "heartbeat_at, draining",
    ),
    (
        "worker_capabilities",
        "",
        "",
        "worker_id, namespace, version, process",
        "",
    ),
    (
        "process_policies",
        "",
        "",
        "namespace, version, process, retry_policy, deadline_origin, deadline_duration_ms",
        "",
    ),
    ("blkit_deployment", "", "", "", ""),
    ("blkit_schema_migrations", "", "", "", ""),
];

/// Transfers only known blkit objects to the NOLOGIN owner and reconciles explicit
/// runtime grants plus dedicated-database security defaults. No connection/commit.
///
/// Call after `provision_managed_roles` in the SAME serialized administrative
/// transaction as canonical schema installation and history recording. The caller
/// validates schema/history and deployment identity before mutation, then rolls
/// back on ANY error and commits only when all preparation succeeds. Runtime SQL
/// still requiring DDL or an immutable-policy UPDATE lock is not supported here.
///
/// The administrator must own/manage the database, public schema and known objects,
/// and be able to assume the managed owner (SUPERUSER is not required). Database
/// PUBLIC CREATE/TEMP and public-schema PUBLIC CREATE are revoked; unrelated object
/// ACLs/owners and unrelated owners' default ACLs are untouched. Only the managed
/// owner's defaults are tightened globally in this database, in public and in any
/// other schema with existing defaults for that managed owner.
///
/// Fails closed on remaining runtime CREATE/TEMP, schema CREATE/ownership, or
/// ownership of any unrelated object in this database; never repairs unrelated
/// owners/ACLs. Also rejects forbidden managed-object rights/grant options surviving
/// through another grantor; a DBA must resolve them, not a cascading ACL sweep.
/// These are current-database checks, not a cluster-wide audit.
///
/// Restores caller identity; leaves a transaction-local trusted search path. Later
/// migrations/history writes must assume the owner when the administrator does not
/// inherit it. Runtime/owner search-path defaults are scoped to this database.
///
/// # Errors
/// Rejects changed/elevated managed roles, absent known objects or insufficient
/// administrative authority. Errors discard raw SQL/driver details and sources.
/// The CLI must retain pre-sink tokio_postgres diagnostic suppression, as required
/// by role provisioning; this helper never logs SQL or credentials.
pub async fn reconcile_managed_privileges(
    transaction: &Transaction<'_>,
    roles: &ManagedRoles,
) -> Result<(), ProvisionError> {
    transaction
        .batch_execute("SET LOCAL search_path = pg_catalog, pg_temp")
        .await
        .map_err(|_| failure("cannot establish trusted grant search path"))?;
    for (role, login) in [
        (roles.owner(), false),
        (roles.api(), true),
        (roles.worker(), true),
    ] {
        if !verify_managed_role(transaction, role, login, true).await? {
            return Err(failure(
                "managed role missing; provision roles before reconciling grants",
            ));
        }
    }
    let row = transaction
        .query_one(
            "SELECT current_user::pg_catalog.text, pg_catalog.current_database()::pg_catalog.text",
            &[],
        )
        .await
        .map_err(|_| failure("cannot inspect administrative grant session"))?;
    let administrator = quote_identifier(row.get(0));
    let database = quote_identifier(row.get(1));
    let owner = quote_identifier(roles.owner());
    let api = quote_identifier(roles.api());
    let worker = quote_identifier(roles.worker());
    let runtimes = format!("{api}, {worker}");
    let grantees = format!("PUBLIC, {runtimes}");
    // A non-superuser transferring table ownership needs CREATE for the new owner.
    transaction.batch_execute(&format!(
        "REVOKE CREATE, TEMPORARY ON DATABASE {database} FROM PUBLIC;
         REVOKE ALL ON DATABASE {database} FROM {runtimes};
         GRANT CONNECT ON DATABASE {database} TO {runtimes};
         REVOKE CREATE ON SCHEMA public FROM PUBLIC;
         REVOKE ALL ON SCHEMA public FROM {runtimes};
         GRANT USAGE ON SCHEMA public TO {runtimes};
         GRANT USAGE, CREATE ON SCHEMA public TO {owner};"
    )).await.map_err(|_| failure("cannot apply dedicated-database access defaults; check database and public-schema authority"))?;
    for role in [&owner, &api, &worker] {
        transaction.batch_execute(&format!(
            "ALTER ROLE {role} IN DATABASE {database} SET search_path = pg_catalog, public, pg_temp"
        )).await.map_err(|_| failure("cannot set database-scoped managed role search path; check administrative authority"))?;
    }
    for (table, ..) in TABLES {
        let qualified = format!("public.{}", quote_identifier(table));
        let already_owned: bool = transaction.query_one(
            "SELECT pg_catalog.pg_get_userbyid(relowner)=$2 FROM pg_catalog.pg_class WHERE oid=$1::pg_catalog.text::pg_catalog.regclass",
            &[&qualified, &roles.owner()],
        ).await.map_err(|_| failure("cannot inspect known managed table ownership"))?.get(0);
        // NOINHERIT administrators can assume the owner, but cannot ALTER its tables.
        if !already_owned {
            transaction.batch_execute(&format!(
                "ALTER TABLE {qualified} OWNER TO {owner}"
            )).await.map_err(|_| failure("cannot transfer known managed table ownership; check schema and object authority"))?;
        }
    }
    let already_owned: bool = transaction.query_one(
        "SELECT pg_catalog.pg_get_userbyid(proowner)=$1 FROM pg_catalog.pg_proc WHERE oid='public.notify_instance_interests()'::pg_catalog.regprocedure",
        &[&roles.owner()],
    ).await.map_err(|_| failure("cannot inspect managed function ownership"))?.get(0);
    if !already_owned {
        transaction
            .batch_execute(&format!(
                "ALTER FUNCTION public.notify_instance_interests() OWNER TO {owner}"
            ))
            .await
            .map_err(|_| {
                failure(
                    "cannot transfer managed function ownership; check administrative authority",
                )
            })?;
    }
    transaction
        .batch_execute(&format!("SET LOCAL ROLE {owner}"))
        .await
        .map_err(|_| failure("cannot assume managed owner; check administrative authority"))?;
    for (table, api_insert, api_update, worker_insert, worker_update) in TABLES {
        let qualified = format!("public.{}", quote_identifier(table));
        // Table REVOKE does not clear column ACLs, including old grant options.
        let columns = transaction.query(
            "SELECT attname::pg_catalog.text FROM pg_catalog.pg_attribute WHERE attrelid=$1::pg_catalog.text::pg_catalog.regclass AND attnum>0 AND NOT attisdropped ORDER BY attnum",
            &[&qualified],
        ).await.map_err(|_| failure("cannot inspect managed table column privileges"))?;
        let columns = columns
            .iter()
            .map(|row| quote_identifier(row.get(0)))
            .collect::<Vec<_>>()
            .join(", ");
        transaction
            .batch_execute(&format!(
                "REVOKE ALL ON TABLE {qualified} FROM {grantees};
             REVOKE ALL ({columns}) ON TABLE {qualified} FROM {grantees};
             GRANT SELECT ON TABLE {qualified} TO {runtimes};"
            ))
            .await
            .map_err(|_| failure("cannot reconcile managed table and column privileges"))?;
        for (role, insert, update) in [
            (&api, api_insert, api_update),
            (&worker, worker_insert, worker_update),
        ] {
            for (privilege, columns) in [("INSERT", insert), ("UPDATE", update)] {
                if !columns.is_empty() {
                    transaction
                        .batch_execute(&format!(
                            "GRANT {privilege} ({columns}) ON TABLE {qualified} TO {role}"
                        ))
                        .await
                        .map_err(|_| failure("cannot grant explicit managed runtime columns"))?;
                }
            }
        }
    }
    transaction
        .batch_execute(&format!(
            "GRANT DELETE ON TABLE public.instance_interests TO {api};
         GRANT DELETE ON TABLE public.workers TO {worker};
         REVOKE ALL ON FUNCTION public.notify_instance_interests() FROM {grantees};"
        ))
        .await
        .map_err(|_| failure("cannot reconcile managed deletion or trigger-function privileges"))?;
    // Per-schema REVOKE only removes additions; it cannot cancel global PUBLIC
    // EXECUTE on functions (or USAGE on types). Revoke both scopes explicitly.
    let schemas = transaction
        .query(
            "SELECT DISTINCT n.nspname::pg_catalog.text FROM pg_catalog.pg_default_acl d
         JOIN pg_catalog.pg_namespace n ON n.oid=d.defaclnamespace
         JOIN pg_catalog.pg_roles r ON r.oid=d.defaclrole
         WHERE r.rolname=$1 AND n.nspname<>'public'",
            &[&roles.owner()],
        )
        .await
        .map_err(|_| failure("cannot inspect schema-specific managed-owner defaults"))?;
    let mut scopes = vec![String::new(), "IN SCHEMA public".into()];
    scopes.extend(
        schemas
            .iter()
            .map(|row| format!("IN SCHEMA {}", quote_identifier(row.get(0)))),
    );
    for scope in scopes {
        for kind in ["TABLES", "SEQUENCES", "FUNCTIONS", "TYPES"] {
            transaction.batch_execute(&format!(
                "ALTER DEFAULT PRIVILEGES FOR ROLE {owner} {scope} REVOKE ALL ON {kind} FROM {grantees}"
            )).await.map_err(|_| failure("cannot deny implicit future managed-object privileges"))?;
        }
    }
    transaction
        .batch_execute(&format!(
            "ALTER DEFAULT PRIVILEGES FOR ROLE {owner} REVOKE ALL ON SCHEMAS FROM {grantees};
         SET LOCAL ROLE {administrator};"
        ))
        .await
        .map_err(|_| {
            failure("cannot reconcile managed schema defaults or restore administrative authority")
        })?;
    let elevated: bool = transaction.query_one(
        "WITH runtime AS (
             SELECT oid FROM pg_catalog.pg_roles WHERE rolname::pg_catalog.text = ANY($1)
         ), deployment AS (
             SELECT oid, datdba FROM pg_catalog.pg_database WHERE datname=pg_catalog.current_database()
         )
         SELECT EXISTS (
             SELECT FROM runtime r, deployment d
             WHERE d.datdba=r.oid
                OR pg_catalog.has_database_privilege(r.oid, d.oid, 'CREATE')
                OR pg_catalog.has_database_privilege(r.oid, d.oid, 'TEMP')
         ) OR EXISTS (
             SELECT FROM runtime r, pg_catalog.pg_namespace n
             WHERE n.nspowner=r.oid OR pg_catalog.has_schema_privilege(r.oid, n.oid, 'CREATE')
         ) OR EXISTS (
             SELECT FROM runtime r, deployment d, pg_catalog.pg_shdepend s
             WHERE s.dbid=d.oid AND s.refclassid='pg_catalog.pg_authid'::pg_catalog.regclass
               AND s.refobjid=r.oid AND s.deptype='o'
         )",
        &[&vec![roles.api(), roles.worker()]],
    ).await.map_err(|_| failure("cannot verify managed runtime ownership and effective DDL privileges"))?.get(0);
    if elevated {
        return Err(failure(
            "unexpected managed runtime ownership or effective DDL privileges; DBA review required",
        ));
    }
    verify_managed_acl(transaction, roles).await
}

// No outward memberships or runtime ownership remain, so effective access is the
// union of direct and PUBLIC ACLs. REVOKE as owner cannot clear other grantors' ACLs.
async fn verify_managed_acl(
    transaction: &Transaction<'_>,
    roles: &ManagedRoles,
) -> Result<(), ProvisionError> {
    let runtimes = vec![roles.api(), roles.worker()];
    for (table, api_insert, api_update, worker_insert, worker_update) in TABLES {
        let qualified = format!("public.{}", quote_identifier(table));
        let rows = transaction.query(
            "WITH target AS (SELECT oid, relacl FROM pg_catalog.pg_class WHERE oid=$1::pg_catalog.text::pg_catalog.regclass),
             acl AS (
                 SELECT a.*, NULL::pg_catalog.text AS column_name FROM target t,
                     LATERAL pg_catalog.aclexplode(t.relacl) a
                 UNION ALL
                 SELECT a.*, c.attname::pg_catalog.text FROM target t
                     JOIN pg_catalog.pg_attribute c ON c.attrelid=t.oid AND c.attnum>0 AND NOT c.attisdropped,
                     LATERAL pg_catalog.aclexplode(c.attacl) a
             )
             SELECT pg_catalog.pg_get_userbyid(a.grantee)::pg_catalog.text, a.privilege_type, a.is_grantable, a.column_name
             FROM acl a WHERE a.grantee=0 OR a.grantee IN (
                 SELECT oid FROM pg_catalog.pg_roles WHERE rolname::pg_catalog.text = ANY($2)
             )",
            &[&qualified, &runtimes],
        ).await.map_err(|_| failure("cannot verify effective managed table and column privileges"))?;
        for row in rows {
            let role: &str = row.get(0);
            let privilege: &str = row.get(1);
            let grantable: bool = row.get(2);
            let column: Option<&str> = row.get(3);
            let (insert, update) = if role == roles.api() {
                (api_insert, api_update)
            } else if role == roles.worker() {
                (worker_insert, worker_update)
            } else {
                return Err(unexpected_acl()); // PUBLIC must have no managed-object rights.
            };
            let allowed = match (privilege, column) {
                ("SELECT", _) => true,
                ("DELETE", None) => {
                    (role == roles.api() && *table == "instance_interests")
                        || (role == roles.worker() && *table == "workers")
                }
                ("INSERT", Some(column)) => insert.split(", ").any(|c| c == column),
                ("UPDATE", Some(column)) => update.split(", ").any(|c| c == column),
                _ => false,
            };
            if grantable || !allowed {
                return Err(unexpected_acl());
            }
        }
    }
    let invalid: bool = transaction.query_one(
        "WITH runtime AS (SELECT oid FROM pg_catalog.pg_roles WHERE rolname::pg_catalog.text = ANY($1))
         SELECT EXISTS (
             SELECT FROM pg_catalog.pg_proc p, LATERAL pg_catalog.aclexplode(p.proacl) a
             WHERE p.oid='public.notify_instance_interests()'::pg_catalog.regprocedure
               AND (a.grantee=0 OR a.grantee IN (SELECT oid FROM runtime))
         ) OR EXISTS (
             SELECT FROM pg_catalog.pg_database d, LATERAL pg_catalog.aclexplode(d.datacl) a
             WHERE d.datname=pg_catalog.current_database() AND a.grantee IN (SELECT oid FROM runtime)
               AND (a.is_grantable OR a.privilege_type<>'CONNECT')
         ) OR EXISTS (
             SELECT FROM pg_catalog.pg_namespace n, LATERAL pg_catalog.aclexplode(n.nspacl) a
             WHERE n.nspname='public' AND a.grantee IN (SELECT oid FROM runtime)
               AND (a.is_grantable OR a.privilege_type<>'USAGE')
         )", &[&runtimes],
    ).await.map_err(|_| failure("cannot verify managed function and access-default privileges"))?.get(0);
    if invalid {
        return Err(unexpected_acl());
    }
    Ok(())
}

fn unexpected_acl() -> ProvisionError {
    failure("unexpected managed object privileges or grant options; DBA review required")
}
