use blkit_cli::postgres_admin::{Preparation, prepare_database};
use blkit_core::{
    postgres_config::DatabaseRole, postgres_connection::PostgresConnectionOptions,
    postgres_store::PostgresStore,
};

mod support {
    pub mod postgres;
}

#[path = "support/managed_postgres.rs"]
mod managed_postgres;
use managed_postgres::Database;

impl Database {
    async fn snapshot(&self) -> Vec<String> {
        let mut snapshot: Vec<String> = self
            .client
            .query(include_str!("support/postgres_schema_snapshot.sql"), &[])
            .await
            .unwrap()
            .iter()
            .map(|row| row.get(0))
            .collect();
        snapshot.extend(self.client.query(
            "SELECT jsonb_build_object('table', c.relname, 'policy', p.polname, 'command', p.polcmd, 'roles', p.polroles::text, 'permissive', p.polpermissive, 'using', pg_catalog.pg_get_expr(p.polqual, p.polrelid), 'check', pg_catalog.pg_get_expr(p.polwithcheck, p.polrelid))::text FROM pg_catalog.pg_policy p JOIN pg_catalog.pg_class c ON c.oid=p.polrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' ORDER BY c.relname, p.polname", &[],
        ).await.unwrap().iter().map(|row| row.get::<_, String>(0)));
        for row in self.client.query("SELECT quote_ident(c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind='r' ORDER BY c.relname", &[]).await.unwrap() {
            let table: String = row.get(0);
            snapshot.push(self.client.query_one(&format!("SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]')::text FROM public.{table} t"), &[]).await.unwrap().get(0));
        }
        snapshot
    }

    async fn assert_refused_unchanged(&self) {
        let before = self.snapshot().await;
        let options = PostgresConnectionOptions::new(None, Some("true")).unwrap();
        let result = PostgresStore::connect_with_options(&self.url, 4, &options).await;
        assert!(
            before == self.snapshot().await,
            "startup changed schema or persisted rows"
        );
        let error = result
            .err()
            .expect("incompatible database must fail startup");
        assert!(
            error.contains("blkit db init") && error.contains("blkit db migrate"),
            "missing administration guidance: {error}"
        );
        assert!(
            !error.contains("startup-secret-sentinel"),
            "metadata/server value leaked"
        );
    }
}

#[test]
fn runtime_checksum_matches_immutable_cli_asset_and_sidecar() {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(include_bytes!("../postgres/0001_initial.sql"));
    let expected = blkit_core::postgres_schema::SUPPORTED_SCHEMA_CHECKSUM;
    let actual: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(actual, expected);
    assert_eq!(
        include_str!("../postgres/0001_initial.sha256")
            .split_whitespace()
            .next(),
        Some(expected)
    );
}

#[tokio::test]
async fn empty_database_startup_refuses_without_creating_schema() {
    Database::start().await.assert_refused_unchanged().await;
}

#[tokio::test]
async fn populated_unversioned_startup_refuses_without_backfilling_or_changing_checkpoints() {
    let db = Database::start().await;
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/schema.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute("UPDATE public.instances SET queued_at_ms=NULL WHERE id='running'")
        .await
        .unwrap();
    assert!(
        db.client
            .query_one(
                "SELECT queued_at_ms IS NULL FROM public.instances WHERE id='running'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    db.assert_refused_unchanged().await;
}

#[tokio::test]
async fn cli_prepared_api_and_worker_start_without_ownership_or_ddl_rights() {
    let db = Database::start().await;
    let settings = db.settings();
    prepare_database(&settings, Preparation::Init { role_prefix: None })
        .await
        .unwrap();
    let before = db.snapshot().await;
    let options = settings.connection_options().unwrap();
    for role in [DatabaseRole::Api, DatabaseRole::Worker] {
        let database = settings.resolve_role(role, &options).unwrap();
        let (client, driver) = database.connection().connect().await.unwrap();
        tokio::spawn(driver);
        let row = client.query_one("SELECT current_user <> pg_get_userbyid(c.relowner), NOT has_schema_privilege(current_user, 'public', 'CREATE'), NOT pg_has_role(current_user, 'blkit_owner', 'MEMBER') FROM pg_class c WHERE c.oid='public.instances'::regclass", &[]).await.unwrap();
        assert!(row.get::<_, bool>(0) && row.get::<_, bool>(1) && row.get::<_, bool>(2));
        assert_eq!(
            client
                .batch_execute("CREATE TABLE public.forbidden_startup_ddl(id int)")
                .await
                .unwrap_err()
                .code(),
            Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
        );
        let store = database
            .connect_store(4)
            .await
            .expect("prepared operational login must start without DDL");
        assert!(store.get_worker("absent").await.unwrap().is_none());
        if matches!(role, DatabaseRole::Worker) {
            assert!(
                !store.heartbeat("absent").await.unwrap(),
                "validation must return a writable pool session"
            );
        }
    }
    assert!(
        before == db.snapshot().await,
        "successful startup mutated prepared schema/data"
    );
}

#[tokio::test]
async fn incompatible_metadata_and_required_objects_refuse_without_mutation() {
    let mut db = Database::start().await;
    // Each case gets a fresh canonical fixture, never constructor preparation.
    for mutation in [
        "UPDATE public.blkit_schema_migrations SET version=0",
        "UPDATE public.blkit_schema_migrations SET version=2",
        "UPDATE public.blkit_schema_migrations SET identity='startup-secret-sentinel'",
        "UPDATE public.blkit_schema_migrations SET checksum=repeat('0',64)",
        "UPDATE public.blkit_schema_migrations SET applied_at='infinity'",
        "UPDATE public.blkit_schema_migrations SET applied_at=clock_timestamp()+interval '1 day'",
        "DELETE FROM public.blkit_schema_migrations",
        "DROP TABLE public.blkit_schema_migrations",
        "ALTER TABLE public.blkit_schema_migrations ALTER COLUMN identity DROP NOT NULL; UPDATE public.blkit_schema_migrations SET identity=NULL",
        "INSERT INTO public.blkit_schema_migrations(version,identity,checksum) VALUES(2,'unknown','unknown')",
        "DELETE FROM public.blkit_deployment",
        "DROP TABLE public.blkit_deployment",
        "ALTER TABLE public.blkit_deployment DROP CONSTRAINT blkit_deployment_singleton_check; UPDATE public.blkit_deployment SET singleton=false",
        "UPDATE public.blkit_deployment SET role_prefix='pg_bad'",
        "UPDATE public.blkit_deployment SET api_role='startup-secret-sentinel'",
        "ALTER TABLE public.blkit_deployment DROP CONSTRAINT blkit_deployment_pkey; INSERT INTO public.blkit_deployment SELECT * FROM public.blkit_deployment",
        "ALTER TABLE public.blkit_schema_migrations ALTER COLUMN version TYPE text",
        "ALTER TABLE public.blkit_deployment ALTER COLUMN role_prefix DROP NOT NULL; UPDATE public.blkit_deployment SET role_prefix=NULL",
        "DROP TABLE public.instance_attachments",
        "ALTER TABLE public.instances DROP COLUMN checkpoint",
        "ALTER TABLE public.instances ALTER COLUMN generation TYPE text",
        "DROP INDEX public.input_requests_answered",
        "DROP INDEX public.instance_attachments_reply; CREATE INDEX instance_attachments_reply ON public.workers(id)",
        "ALTER TABLE public.instances DISABLE TRIGGER instance_terminal_interests_notify",
        "DROP TRIGGER input_request_interests_notify ON public.input_requests",
        "DROP TRIGGER instance_terminal_interests_notify ON public.instances; CREATE TRIGGER instance_terminal_interests_notify AFTER UPDATE ON public.workers FOR EACH ROW EXECUTE FUNCTION public.notify_instance_interests()",
        "ALTER FUNCTION public.notify_instance_interests() RENAME TO missing_notification_function",
        "ALTER TABLE public.blkit_schema_migrations RENAME TO old_history; UPDATE public.old_history SET identity='startup-secret-sentinel'; CREATE VIEW public.blkit_schema_migrations AS SELECT identity::bigint AS version, identity, checksum, applied_at FROM public.old_history",
    ] {
        db.client
            .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public")
            .await
            .unwrap();
        support::postgres::prepare_fresh_database(&mut db.client)
            .await
            .unwrap();
        db.client.batch_execute(mutation).await.unwrap();
        db.assert_refused_unchanged().await;
    }
}

async fn assert_side_effecting_metadata_refused_unchanged(
    relation: &str,
    projection: Option<&str>,
) {
    let db = Database::start().await;
    let settings = db.settings();
    prepare_database(&settings, Preparation::Init { role_prefix: None })
        .await
        .unwrap();
    // Seed as admin, without worker registration or immutable-policy publication.
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    db.client.batch_execute(
        "INSERT INTO public.workers(id, heartbeat_at) VALUES ('startup-secret-sentinel', 100);
         CREATE FUNCTION public.startup_metadata_touch(recorded bigint) RETURNS bigint
         LANGUAGE plpgsql VOLATILE SECURITY INVOKER AS $$
         BEGIN
             IF current_user <> 'blkit_worker' THEN
                 RAISE EXCEPTION 'startup-secret-sentinel';
             END IF;
             UPDATE public.workers SET heartbeat_at=heartbeat_at+1 WHERE id='startup-secret-sentinel';
             IF NOT FOUND THEN RAISE EXCEPTION 'startup-secret-sentinel'; END IF;
             RETURN recorded;
         END $$;
         REVOKE ALL ON FUNCTION public.startup_metadata_touch(bigint) FROM PUBLIC;
         GRANT EXECUTE ON FUNCTION public.startup_metadata_touch(bigint) TO blkit_worker;",
    ).await.unwrap();
    let metadata_sql = format!("SELECT to_jsonb(t)::text FROM public.{relation} t");
    let expected: String = db
        .client
        .query_one(&metadata_sql, &[])
        .await
        .unwrap()
        .get(0);
    // Identifiers/projections come exclusively from the fixed test cases below.
    if let Some(projection) = projection {
        db.client.batch_execute(&format!(
            "ALTER TABLE public.{relation} RENAME TO {relation}_saved;
             CREATE VIEW public.{relation} WITH (security_invoker=true) AS SELECT {projection} FROM public.{relation}_saved;
             GRANT SELECT ON public.{relation} TO blkit_worker;"
        )).await.unwrap();
    } else {
        db.client.batch_execute(&format!(
            "ALTER TABLE public.{relation} ENABLE ROW LEVEL SECURITY;
             CREATE POLICY startup_metadata_side_effect ON public.{relation} FOR SELECT TO blkit_worker USING (public.startup_metadata_touch(1)=1);"
        )).await.unwrap();
    }
    let options = settings.connection_options().unwrap();
    let database = settings
        .resolve_role(DatabaseRole::Worker, &options)
        .unwrap();
    let (mut worker, driver) = database.connection().connect().await.unwrap();
    tokio::spawn(driver);
    let row = worker.query_one(
        "SELECT current_user='blkit_worker', has_column_privilege(current_user, 'public.workers', 'heartbeat_at', 'UPDATE'), NOT has_table_privilege(current_user, 'public.workers', 'UPDATE'), NOT pg_has_role(current_user, 'blkit_owner', 'MEMBER')", &[],
    ).await.unwrap();
    assert!((0..4).all(|index| row.get::<_, bool>(index)));
    let heartbeat_sql =
        "SELECT heartbeat_at FROM public.workers WHERE id='startup-secret-sentinel'";
    let before = db.snapshot().await;
    // Prove valid metadata evaluation can use the worker's existing column grant,
    // then roll the probe back before testing the actual constructor.
    let transaction = worker.transaction().await.unwrap();
    let metadata: String = transaction
        .query_one(&metadata_sql, &[])
        .await
        .unwrap()
        .get(0);
    assert!(
        metadata == expected,
        "fixture must return valid canonical metadata"
    );
    assert_eq!(
        transaction
            .query_one(heartbeat_sql, &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        101
    );
    transaction.rollback().await.unwrap();
    assert!(
        before == db.snapshot().await,
        "setup probe rollback changed persisted state"
    );
    assert_eq!(
        db.client
            .query_one(heartbeat_sql, &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        100
    );

    let result = database.connect_store(4).await;
    let after = db.snapshot().await;
    let heartbeat: i64 = db
        .client
        .query_one(heartbeat_sql, &[])
        .await
        .unwrap()
        .get(0);
    let error = result.err().unwrap_or_else(|| panic!("{relation} startup succeeded after unsafe metadata evaluation: heartbeat was 100, now {heartbeat}"));
    assert!(
        error.contains("blkit db init") && error.contains("blkit db migrate"),
        "missing administration guidance: {error}"
    );
    assert!(
        !error.contains("startup-secret-sentinel"),
        "metadata/server value leaked"
    );
    assert!(
        before == after,
        "{relation} refusal changed schema/data/checkpoints: seeded heartbeat was 100, now {heartbeat}"
    );
    assert_eq!(heartbeat, 100);
    assert!(db.client.query_one(
        "SELECT NOT EXISTS (SELECT FROM pg_stat_activity WHERE usename='blkit_worker' AND state LIKE 'idle in transaction%')", &[],
    ).await.unwrap().get::<_, bool>(0), "failed validation left a worker session in a transaction");
}

#[tokio::test]
async fn side_effecting_history_view_is_refused_before_worker_heartbeat_mutation() {
    assert_side_effecting_metadata_refused_unchanged(
        "blkit_schema_migrations",
        Some("public.startup_metadata_touch(version) AS version, identity, checksum, applied_at"),
    )
    .await;
}

#[tokio::test]
async fn side_effecting_deployment_view_is_refused_before_worker_heartbeat_mutation() {
    assert_side_effecting_metadata_refused_unchanged(
        "blkit_deployment",
        Some("public.startup_metadata_touch(CASE WHEN singleton THEN 1::bigint ELSE 0::bigint END)=1 AS singleton, role_prefix, owner_role, api_role, worker_role"),
    ).await;
}

#[tokio::test]
async fn side_effecting_metadata_rls_is_refused_without_worker_heartbeat_mutation() {
    assert_side_effecting_metadata_refused_unchanged("blkit_deployment", None).await;
}
