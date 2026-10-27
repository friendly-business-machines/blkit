//! Read-only compatibility identity; executable migrations belong to blkit-cli.

pub const SUPPORTED_SCHEMA_VERSION: i64 = 1;
pub const SUPPORTED_SCHEMA_IDENTITY: &str = "blkit-postgres-v1";
pub const SUPPORTED_SCHEMA_CHECKSUM: &str =
    "e7d49695bd5c35b92aaaf76e9de8572b12215c01258f1b193408351573f17a04";

const INCOMPATIBLE: &str = "postgres schema is missing, unsupported, or inconsistent; prepare the database with the matching CLI using blkit db init or blkit db migrate before startup";

// Preparation attests definitions and permissions; startup only checks the recorded
// compatibility identity and required runtime columns/objects, never repairs them.
pub(crate) async fn validate(client: &mut tokio_postgres::Client) -> Result<(), String> {
    // Metadata SELECTs can execute policy functions even on ordinary tables.
    let transaction = client
        .build_transaction()
        .read_only(true)
        .start()
        .await
        .map_err(|_| INCOMPATIBLE.to_owned())?;
    match validate_inner(&transaction).await {
        Ok(()) => transaction
            .commit()
            .await
            .map_err(|_| INCOMPATIBLE.to_owned()),
        Err(()) => {
            transaction
                .rollback()
                .await
                .map_err(|_| INCOMPATIBLE.to_owned())?;
            Err(INCOMPATIBLE.to_owned())
        }
    }
}

async fn validate_inner(client: &tokio_postgres::Transaction<'_>) -> Result<(), ()> {
    // Reject executable relations before evaluating either metadata record.
    let mut tables = Vec::new();
    let mut columns = Vec::new();
    let mut types = Vec::new();
    for (table, ty, names) in REQUIRED_COLUMNS {
        for column in names.split_whitespace() {
            tables.push(*table);
            columns.push(column);
            types.push(*ty);
        }
    }
    let row = client.query_one(
        "SELECT NOT EXISTS (
            SELECT FROM ROWS FROM (pg_catalog.unnest($1::text[]), pg_catalog.unnest($2::text[]), pg_catalog.unnest($3::text[])) AS expected(tab, col, typ)
            WHERE NOT EXISTS (
                SELECT FROM pg_catalog.pg_attribute a
                JOIN pg_catalog.pg_class c ON c.oid=a.attrelid
                JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
                WHERE n.nspname='public' AND c.relname=expected.tab AND c.relkind='r'
                  AND a.attname=expected.col AND a.attnum>0 AND NOT a.attisdropped
                  AND pg_catalog.format_type(a.atttypid,a.atttypmod)=expected.typ
            )
        )",
        &[&tables, &columns, &types],
    ).await.map_err(|_| ())?;
    if !row.try_get::<_, bool>(0).map_err(|_| ())? {
        return Err(());
    }

    let history = client.query(
        "SELECT version, identity, checksum, applied_at IS NOT NULL AND pg_catalog.isfinite(applied_at) AND applied_at <= pg_catalog.clock_timestamp() FROM public.blkit_schema_migrations LIMIT 2",
        &[],
    ).await.map_err(|_| ())?;
    if history.len() != 1 {
        return Err(());
    }
    let row = &history[0];
    if row.try_get::<_, i64>(0).map_err(|_| ())? != SUPPORTED_SCHEMA_VERSION
        || row.try_get::<_, &str>(1).map_err(|_| ())? != SUPPORTED_SCHEMA_IDENTITY
        || row.try_get::<_, &str>(2).map_err(|_| ())? != SUPPORTED_SCHEMA_CHECKSUM
        || !row.try_get::<_, bool>(3).map_err(|_| ())?
    {
        return Err(());
    }
    let deployment = client.query(
        "SELECT singleton, role_prefix, owner_role, api_role, worker_role FROM public.blkit_deployment LIMIT 2",
        &[],
    ).await.map_err(|_| ())?;
    if deployment.len() != 1 {
        return Err(());
    }
    let row = &deployment[0];
    let prefix: &str = row.try_get(1).map_err(|_| ())?;
    if !row.try_get::<_, bool>(0).map_err(|_| ())?
        || prefix.is_empty()
        || prefix.len() > 56
        || !prefix.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        || prefix.starts_with("pg_")
        || !prefix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        || row.try_get::<_, &str>(2).map_err(|_| ())? != format!("{prefix}_owner")
        || row.try_get::<_, &str>(3).map_err(|_| ())? != format!("{prefix}_api")
        || row.try_get::<_, &str>(4).map_err(|_| ())? != format!("{prefix}_worker")
    {
        return Err(());
    }

    let row = client.query_one(
        "SELECT NOT EXISTS (
            SELECT FROM (VALUES
                ('instances','instances_pkey',true),
                ('input_requests','input_requests_pkey',true),
                ('input_requests','input_requests_answered',false),
                ('instance_attachments','instance_attachments_start',true),
                ('instance_attachments','instance_attachments_reply',true),
                ('instance_interests','instance_interests_pkey',true),
                ('workers','workers_pkey',true),
                ('worker_capabilities','worker_capabilities_pkey',true),
                ('process_policies','process_policies_pkey',true),
                ('blkit_deployment','blkit_deployment_pkey',true),
                ('blkit_schema_migrations','blkit_schema_migrations_pkey',true)
            ) AS expected(tab, idx, uniq)
            WHERE NOT EXISTS (
                SELECT FROM pg_catalog.pg_index i
                JOIN pg_catalog.pg_class idx ON idx.oid=i.indexrelid
                JOIN pg_catalog.pg_class tab ON tab.oid=i.indrelid
                JOIN pg_catalog.pg_namespace n ON n.oid=tab.relnamespace
                WHERE n.nspname='public' AND tab.relname=expected.tab AND idx.relname=expected.idx
                  AND idx.relnamespace=n.oid AND i.indisvalid AND i.indisready AND i.indislive
                  AND i.indisunique=expected.uniq
            )
        ) AND NOT EXISTS (
            SELECT FROM (VALUES
                ('instances','instance_terminal_interests_notify'),
                ('input_requests','input_request_interests_notify')
            ) AS expected(tab, trig)
            WHERE NOT EXISTS (
                SELECT FROM pg_catalog.pg_trigger t
                JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid
                JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
                JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
                WHERE n.nspname='public' AND c.relname=expected.tab AND t.tgname=expected.trig
                  AND NOT t.tgisinternal AND t.tgenabled IN ('O','A')
                  AND p.pronamespace=n.oid AND p.proname='notify_instance_interests'
                  AND p.pronargs=0 AND p.prokind='f' AND p.prorettype='pg_catalog.trigger'::pg_catalog.regtype
            )
        )",
        &[],
    ).await.map_err(|_| ())?;
    if !row.try_get::<_, bool>(0).map_err(|_| ())? {
        return Err(());
    }
    Ok(())
}

const REQUIRED_COLUMNS: &[(&str, &str, &str)] = &[
    (
        "instances",
        "text",
        "id namespace version process input status result error checkpoint terminal_name owner_id retry_policy deadline_origin",
    ),
    (
        "instances",
        "bigint",
        "created_at updated_at attempt first_failure_at next_eligible_at lease_until generation queued_at_ms first_claim_at_ms wake_at_ms deadline_duration_ms deadline_at_ms",
    ),
    ("instances", "boolean", "input_resume"),
    (
        "input_requests",
        "text",
        "instance_id request_id prompt schema status response",
    ),
    (
        "instance_attachments",
        "text",
        "instance_id request_id object_key filename media_type",
    ),
    ("instance_attachments", "bigint", "ordinal byte_length"),
    ("instance_interests", "text", "instance_id channel"),
    (
        "instance_interests",
        "timestamp with time zone",
        "expires_at",
    ),
    ("workers", "text", "id"),
    ("workers", "bigint", "heartbeat_at"),
    ("workers", "boolean", "draining"),
    (
        "worker_capabilities",
        "text",
        "worker_id namespace version process",
    ),
    (
        "process_policies",
        "text",
        "namespace version process retry_policy deadline_origin",
    ),
    ("process_policies", "bigint", "deadline_duration_ms"),
    ("blkit_schema_migrations", "bigint", "version"),
    ("blkit_schema_migrations", "text", "identity checksum"),
    (
        "blkit_schema_migrations",
        "timestamp with time zone",
        "applied_at",
    ),
    ("blkit_deployment", "boolean", "singleton"),
    (
        "blkit_deployment",
        "text",
        "role_prefix owner_role api_role worker_role",
    ),
];
