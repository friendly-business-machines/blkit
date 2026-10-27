use blkit_cli::postgres_admin::{Preparation, PreparationOutcome, prepare_database};
use blkit_core::postgres_config::EffectivePostgresConfig;
use std::time::{Duration, Instant};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio_postgres::{Client, Config, NoTls};

const LOCK: i64 = 0x626c6b69742d6462;
const SENTINEL: &str = "migration-secret-sentinel";
struct Database {
    _container: ContainerAsync<Postgres>,
    client: Client,
    url: String,
    config: Config,
}
impl Database {
    async fn start() -> Self {
        let container = Postgres::default()
            .with_tag("17.6-alpine")
            .with_env_var("POSTGRES_HOST_AUTH_METHOD", "scram-sha-256")
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(container.get_host().await.unwrap().to_string());
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let url = format!("postgresql://postgres:postgres@{host}:{port}/postgres");
        let config: Config = url.parse().unwrap();
        let client = connect(&config).await;
        Self {
            _container: container,
            client,
            url,
            config,
        }
    }
    fn settings(&self, runtime: bool) -> EffectivePostgresConfig {
        let mut values = vec![
            ("BLKIT_ADMIN_DATABASE_URL", self.url.clone()),
            ("BLKIT_POSTGRES_INSECURE_DEV", "true".into()),
        ];
        if runtime {
            for (key, role) in [
                ("BLKIT_API_DATABASE_URL", "api"),
                ("BLKIT_WORKER_DATABASE_URL", "worker"),
            ] {
                values.push((
                    key,
                    format!(
                        "postgresql://blkit_{role}:{SENTINEL}@{}",
                        self.url.rsplit_once('@').unwrap().1
                    ),
                ));
            }
        }
        EffectivePostgresConfig::from_iter(None, values).unwrap()
    }
    async fn init(&self) -> Result<PreparationOutcome, blkit_cli::postgres_admin::ProvisionError> {
        prepare_database(
            &self.settings(true),
            Preparation::Init { role_prefix: None },
        )
        .await
    }
    async fn snapshot(&self) -> Vec<String> {
        // Includes schema, data, owner/ACL state and password verifiers; never printed.
        let sql = "SELECT jsonb_build_object('class', (SELECT jsonb_agg(to_jsonb(c) - 'oid' - 'relfilenode' - 'relpages' - 'reltuples' - 'relallvisible' - 'relfrozenxid' - 'relminmxid' ORDER BY c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public'), 'roles', (SELECT jsonb_agg(to_jsonb(r)) FROM pg_authid r WHERE rolname !~ '^pg_'), 'defaults', (SELECT jsonb_agg(to_jsonb(d)) FROM pg_default_acl d), 'database_acl', (SELECT datacl::text FROM pg_database WHERE datname=current_database()), 'schema_acl', (SELECT jsonb_agg(a::text ORDER BY a::text) FROM pg_namespace n, LATERAL unnest(n.nspacl) a WHERE n.nspname='public'))::text";
        let mut result = vec![self.client.query_one(sql, &[]).await.unwrap().get(0)];
        result.extend(
            self.client
                .query(include_str!("support/postgres_schema_snapshot.sql"), &[])
                .await
                .unwrap()
                .iter()
                .map(|row| row.get::<_, String>(0)),
        );
        result.push(self.client.query_one("SELECT coalesce(jsonb_agg(jsonb_build_array(c.relname,a.attname,a.attacl) ORDER BY c.relname,a.attnum),'[]')::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND a.attnum>0", &[]).await.unwrap().get(0));
        result.push(self.client.query_one("SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY r.oid),'[]')::text FROM pg_rewrite r JOIN pg_class c ON c.oid=r.ev_class JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public'", &[]).await.unwrap().get(0));
        result.push(self.client.query_one("SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY i.inhrelid,i.inhparent),'[]')::text FROM pg_inherits i WHERE i.inhrelid IN (SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public') OR i.inhparent IN (SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public')", &[]).await.unwrap().get(0));
        result.push(self.client.query_one("SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY t.oid),'[]')::text FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND t.tgisinternal AND t.tgconstraint<>0", &[]).await.unwrap().get(0));
        for table in [
            "blkit_schema_migrations",
            "blkit_deployment",
            "instances",
            "input_requests",
            "instance_attachments",
            "instance_interests",
            "workers",
            "worker_capabilities",
            "process_policies",
        ] {
            let exists: bool = self
                .client
                .query_one(
                    "SELECT to_regclass($1) IS NOT NULL",
                    &[&format!("public.{table}")],
                )
                .await
                .unwrap()
                .get(0);
            if exists {
                result.push(self.client.query_one(&format!("SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]')::text FROM public.{table} t"), &[]).await.unwrap().get(0));
            }
        }
        result
    }
    async fn assert_refused_unchanged(
        &self,
        mode: Preparation<'_>,
    ) -> blkit_cli::postgres_admin::ProvisionError {
        let before = self.snapshot().await;
        let error = prepare_database(&self.settings(false), mode)
            .await
            .unwrap_err();
        assert!(!format!("{error:?} {error}").contains(SENTINEL));
        assert!(std::error::Error::source(&error).is_none());
        assert!(
            before == self.snapshot().await,
            "refusal mutated database state"
        );
        error
    }
}
async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

// Missing history, split commits, password rotation, or absent grants break real SQL.
#[tokio::test]
async fn fresh_preparation_is_usable_and_admin_only_reruns_preserve_passwords() {
    let db = Database::start().await;
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::Initialized);
    let history = db.client.query("SELECT version, identity, checksum FROM public.blkit_schema_migrations ORDER BY version", &[]).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].get::<_, i64>(0), 1);
    assert_eq!(history[0].get::<_, &str>(1), "blkit-postgres-v1");
    assert_eq!(
        history[0].get::<_, &str>(2),
        "e7d49695bd5c35b92aaaf76e9de8572b12215c01258f1b193408351573f17a04"
    );
    let mut api = db.config.clone();
    api.user("blkit_api").password(SENTINEL);
    let api = connect(&api).await;
    api.batch_execute("INSERT INTO public.instances (id,namespace,version,process,input,status,created_at,updated_at) VALUES ('saved','n','v','p','{}','queued',1,1)").await.unwrap();
    let mut worker = db.config.clone();
    worker.user("blkit_worker").password(SENTINEL);
    let worker = connect(&worker).await;
    worker.batch_execute("INSERT INTO public.workers(id,heartbeat_at) VALUES ('w',1); UPDATE public.instances SET owner_id='w',checkpoint='persisted' WHERE id='saved'; INSERT INTO public.input_requests(instance_id,request_id,prompt,schema) VALUES ('saved','r','prompt','{}')").await.unwrap();
    api.batch_execute("UPDATE public.input_requests SET status='answered', response='reply' WHERE instance_id='saved'").await.unwrap();
    assert!(
        api.batch_execute("UPDATE public.instances SET checkpoint='forbidden'")
            .await
            .is_err()
    );
    assert!(
        worker
            .batch_execute("UPDATE public.input_requests SET response='forbidden'")
            .await
            .is_err()
    );
    assert!(
        api.batch_execute("DELETE FROM public.blkit_schema_migrations")
            .await
            .is_err()
    );
    let passwords: Vec<String> = db.client.query("SELECT rolpassword FROM pg_authid WHERE rolname IN ('blkit_api','blkit_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    for mode in [
        Preparation::Init { role_prefix: None },
        Preparation::Migrate { role_prefix: None },
    ] {
        assert_eq!(
            prepare_database(&db.settings(false), mode).await.unwrap(),
            PreparationOutcome::AlreadyCurrent
        );
    }
    let after: Vec<String> = db.client.query("SELECT rolpassword FROM pg_authid WHERE rolname IN ('blkit_api','blkit_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(passwords == after, "rerun rotated passwords");
    assert_eq!(
        db.client
            .query_one(
                "SELECT checkpoint FROM public.instances WHERE id='saved'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, &str>(0),
        "persisted"
    );
}

#[tokio::test]
async fn noninheriting_database_owner_can_initialize_and_rerun() {
    let mut db = Database::start().await;
    db.client.batch_execute("CREATE ROLE deployer LOGIN NOINHERIT CREATEROLE PASSWORD 'migration-secret-sentinel'; ALTER DATABASE postgres OWNER TO deployer; GRANT USAGE, CREATE ON SCHEMA public TO deployer").await.unwrap();
    db.url = db
        .url
        .replace("postgres:postgres@", &format!("deployer:{SENTINEL}@"));
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::Initialized);
    assert_eq!(
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
}

#[tokio::test]
async fn concurrent_initializations_install_exactly_once() {
    let db = Database::start().await;
    let (left, right) = tokio::join!(db.init(), db.init());
    let mut results = [left.unwrap(), right.unwrap()];
    results.sort_by_key(|outcome| matches!(outcome, PreparationOutcome::AlreadyCurrent));
    assert_eq!(
        results,
        [
            PreparationOutcome::Initialized,
            PreparationOutcome::AlreadyCurrent
        ]
    );
    assert_eq!(
        db.client
            .query_one("SELECT count(*) FROM public.blkit_schema_migrations", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
}

#[tokio::test]
async fn held_lock_has_finite_wait_and_no_lingering_session() {
    let db = Database::start().await;
    db.client
        .execute("SELECT pg_advisory_lock($1)", &[&LOCK])
        .await
        .unwrap();
    let start = Instant::now();
    let error = db.init().await.unwrap_err();
    assert!(error.to_string().contains("contention"));
    assert!(start.elapsed() < Duration::from_secs(12));
    db.client
        .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .await
        .unwrap();
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::Initialized);
    assert_eq!(db.client.query_one("SELECT count(*) FROM pg_locks WHERE locktype='advisory' AND database=(SELECT oid FROM pg_database WHERE datname=current_database())", &[]).await.unwrap().get::<_, i64>(0), 0);
}

#[tokio::test]
async fn unknown_and_unversioned_schemas_are_not_adopted_by_init() {
    let db = Database::start().await;
    let error = db
        .assert_refused_unchanged(Preparation::Migrate { role_prefix: None })
        .await;
    assert!(error.to_string().contains("db init"));
    db.client
        .batch_execute(
            "CREATE TABLE public.instances (id text); INSERT INTO public.instances VALUES ('keep')",
        )
        .await
        .unwrap();
    db.assert_refused_unchanged(Preparation::Init { role_prefix: None })
        .await;
    db.assert_refused_unchanged(Preparation::Migrate { role_prefix: None })
        .await;
    db.client
        .batch_execute("DROP TABLE public.instances")
        .await
        .unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/schema.sql"))
        .await
        .unwrap();
    let error = db
        .assert_refused_unchanged(Preparation::Init { role_prefix: None })
        .await;
    assert!(error.to_string().contains("migrate"));
}

#[tokio::test]
async fn tampered_history_metadata_and_managed_shapes_fail_before_grant_repair() {
    let db = Database::start().await;
    db.init().await.unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    assert!(
        db.client
            .query_one(
                "SELECT EXISTS(SELECT FROM public.instances WHERE checkpoint IS NOT NULL)",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    for sql in [
        "CREATE TABLE public.blkit_future (id text)",
        "UPDATE public.blkit_schema_migrations SET applied_at='infinity'",
        "ALTER FUNCTION public.notify_instance_interests() SECURITY DEFINER",
        "UPDATE public.blkit_schema_migrations SET checksum='bad'",
        "UPDATE public.blkit_schema_migrations SET identity='unknown'",
        "UPDATE public.blkit_schema_migrations SET version=2",
        "INSERT INTO public.blkit_schema_migrations(version,identity,checksum) VALUES (3,'newer','bad')",
        "DELETE FROM public.blkit_schema_migrations",
        "UPDATE public.blkit_deployment SET api_role='other'",
        "DELETE FROM public.blkit_deployment",
        "ALTER TABLE public.instances ADD COLUMN surprise text",
        "DROP INDEX public.input_requests_answered",
        "ALTER TABLE public.instances DISABLE TRIGGER instance_terminal_interests_notify",
        "ALTER TABLE public.blkit_deployment ALTER COLUMN role_prefix DROP NOT NULL",
    ] {
        assert_eq!(
            prepare_database(
                &db.settings(false),
                Preparation::Migrate { role_prefix: None }
            )
            .await
            .unwrap(),
            PreparationOutcome::AlreadyCurrent
        );
        // Restore each mutation with its explicit inverse, never preparation.
        db.client.batch_execute(sql).await.unwrap();
        db.assert_refused_unchanged(Preparation::Migrate { role_prefix: None })
            .await;
        let inverse = match sql {
            s if s.starts_with("CREATE TABLE") => "DROP TABLE public.blkit_future",
            s if s.contains("applied_at=") => {
                "UPDATE public.blkit_schema_migrations SET applied_at=clock_timestamp()"
            }
            s if s.contains("SECURITY DEFINER") => {
                "ALTER FUNCTION public.notify_instance_interests() SECURITY INVOKER"
            }
            s if s.contains("checksum='bad'") => {
                "UPDATE public.blkit_schema_migrations SET checksum='e7d49695bd5c35b92aaaf76e9de8572b12215c01258f1b193408351573f17a04'"
            }
            s if s.contains("identity='unknown'") => {
                "UPDATE public.blkit_schema_migrations SET identity='blkit-postgres-v1'"
            }
            s if s.contains("version=2") => "UPDATE public.blkit_schema_migrations SET version=1",
            s if s.starts_with("INSERT") => {
                "DELETE FROM public.blkit_schema_migrations WHERE version=3"
            }
            "DELETE FROM public.blkit_schema_migrations" => {
                "INSERT INTO public.blkit_schema_migrations(version,identity,checksum) VALUES(1,'blkit-postgres-v1','e7d49695bd5c35b92aaaf76e9de8572b12215c01258f1b193408351573f17a04')"
            }
            s if s.contains("api_role='other'") => {
                "UPDATE public.blkit_deployment SET api_role='blkit_api'"
            }
            "DELETE FROM public.blkit_deployment" => {
                "INSERT INTO public.blkit_deployment VALUES(true,'blkit','blkit_owner','blkit_api','blkit_worker')"
            }
            s if s.contains("ADD COLUMN") => "ALTER TABLE public.instances DROP COLUMN surprise",
            s if s.contains("DROP INDEX") => {
                "CREATE INDEX input_requests_answered ON public.input_requests(instance_id) WHERE status='answered'"
            }
            s if s.contains("DISABLE TRIGGER") => {
                "ALTER TABLE public.instances ENABLE TRIGGER instance_terminal_interests_notify"
            }
            _ => "ALTER TABLE public.blkit_deployment ALTER COLUMN role_prefix SET NOT NULL",
        };
        db.client.batch_execute(inverse).await.unwrap();
    }
    db.client
        .batch_execute("ALTER TABLE public.instances DROP COLUMN checkpoint")
        .await
        .unwrap();
    db.assert_refused_unchanged(Preparation::Migrate { role_prefix: None })
        .await;
}

// Real event-trigger failure after role creation, ownership and grants, before commit.
#[tokio::test]
async fn late_failure_rolls_back_schema_roles_acl_history_and_retry_succeeds() {
    let db = Database::start().await;
    db.client.batch_execute("CREATE FUNCTION public.fail_preparation() RETURNS event_trigger LANGUAGE plpgsql AS $$ BEGIN IF current_user='blkit_owner' AND tg_tag='GRANT' THEN RAISE EXCEPTION 'migration-secret-sentinel'; END IF; END $$; CREATE EVENT TRIGGER fail_preparation ON ddl_command_end EXECUTE FUNCTION public.fail_preparation()").await.unwrap();
    let before = db.snapshot().await;
    let error = db.init().await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot reconcile managed table and column privileges")
    );
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
    assert!(
        before == db.snapshot().await,
        "failed preparation stranded schema, roles or ACLs"
    );
    db.client
        .batch_execute(
            "DROP EVENT TRIGGER fail_preparation; DROP FUNCTION public.fail_preparation()",
        )
        .await
        .unwrap();
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::Initialized);
}

#[tokio::test]
async fn explicit_prefix_mismatch_is_refused_without_mutation() {
    let db = Database::start().await;
    db.init().await.unwrap();
    db.assert_refused_unchanged(Preparation::Init {
        role_prefix: Some("other"),
    })
    .await;
}

#[tokio::test]
async fn history_insert_failure_rolls_back_every_fresh_change() {
    let db = Database::start().await;
    db.client.batch_execute("CREATE FUNCTION public.reject_history() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'migration-secret-sentinel'; END $$; CREATE FUNCTION public.install_history_fault() RETURNS event_trigger LANGUAGE plpgsql AS $$ BEGIN IF tg_tag='ALTER DEFAULT PRIVILEGES' AND current_user='blkit_owner' AND NOT EXISTS(SELECT FROM pg_trigger WHERE tgrelid=to_regclass('public.blkit_schema_migrations') AND tgname='reject_history') THEN CREATE TRIGGER reject_history BEFORE INSERT ON public.blkit_schema_migrations FOR EACH ROW EXECUTE FUNCTION public.reject_history(); END IF; END $$; CREATE EVENT TRIGGER install_history_fault ON ddl_command_end EXECUTE FUNCTION public.install_history_fault()").await.unwrap();
    let before = db.snapshot().await;
    let error = db.init().await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot record successful migration history"),
        "unexpected safe failure category: {error}"
    );
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
    assert!(
        before == db.snapshot().await,
        "history failure stranded preparation changes"
    );
    db.client.batch_execute("DROP EVENT TRIGGER install_history_fault; DROP FUNCTION public.install_history_fault(); DROP FUNCTION public.reject_history()").await.unwrap();
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::Initialized);
}

async fn wait_for_admin_query(client: &Client, query: &str) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let found: bool = client.query_one("SELECT EXISTS(SELECT FROM pg_stat_activity WHERE application_name='blkit_migration_test' AND query LIKE $1)", &[&query]).await.unwrap().get(0);
            if found { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("runner did not reach expected database operation");
}

#[tokio::test]
async fn validates_history_again_after_acquiring_lock() {
    let mut db = Database::start().await;
    db.init().await.unwrap();
    db.url.push_str("?application_name=blkit_migration_test");
    db.client
        .execute("SELECT pg_advisory_lock($1)", &[&LOCK])
        .await
        .unwrap();
    let settings = db.settings(false);
    let runner = tokio::spawn(async move {
        prepare_database(&settings, Preparation::Migrate { role_prefix: None }).await
    });
    wait_for_admin_query(&db.client, "%pg_try_advisory_lock%").await;
    db.client
        .batch_execute("UPDATE public.blkit_schema_migrations SET checksum='tampered-under-lock'")
        .await
        .unwrap();
    let before = db.snapshot().await;
    db.client
        .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .await
        .unwrap();
    let error = runner.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("history/checksum"));
    assert!(
        before == db.snapshot().await,
        "under-lock refusal changed state"
    );
}

#[tokio::test]
async fn cancelling_a_lock_wait_closes_the_session_without_background_work() {
    let mut db = Database::start().await;
    db.url.push_str("?application_name=blkit_migration_test");
    db.client
        .execute("SELECT pg_advisory_lock($1)", &[&LOCK])
        .await
        .unwrap();
    let settings = db.settings(true);
    let runner = tokio::spawn(async move {
        prepare_database(&settings, Preparation::Init { role_prefix: None }).await
    });
    wait_for_admin_query(&db.client, "%pg_try_advisory_lock%").await;
    runner.abort();
    assert!(runner.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let count: i64 = db.client.query_one("SELECT count(*) FROM pg_stat_activity WHERE application_name='blkit_migration_test'", &[]).await.unwrap().get(0);
            if count == 0 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("cancelled preparation retained a server session");
    db.client
        .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .await
        .unwrap();
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::Initialized);
}

#[tokio::test]
async fn whole_connection_deadline_bounds_stalled_tls_and_authentication() {
    for plaintext in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let settings = EffectivePostgresConfig::from_iter(
            None,
            [
                (
                    "BLKIT_ADMIN_DATABASE_URL",
                    format!("postgresql://admin:{SENTINEL}@{address}/postgres"),
                ),
                ("BLKIT_POSTGRES_INSECURE_DEV", plaintext.to_string()),
            ],
        )
        .unwrap();
        let stall = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let start = Instant::now();
        let error = prepare_database(&settings, Preparation::Init { role_prefix: None })
            .await
            .unwrap_err();
        stall.abort();
        let _ = stall.await;
        assert!(error.to_string().contains("connection deadline"));
        assert!(!format!("{error:?} {error}").contains(SENTINEL));
        assert!(start.elapsed() >= Duration::from_secs(9));
        assert!(start.elapsed() < Duration::from_secs(15));
    }
}

#[tokio::test]
async fn cancelling_in_flight_preparation_releases_lock_and_rolls_back() {
    let mut db = Database::start().await;
    db.url.push_str("?application_name=blkit_migration_test");
    db.client.batch_execute("CREATE FUNCTION public.pause_preparation() RETURNS event_trigger LANGUAGE plpgsql AS $$ BEGIN IF current_user='blkit_owner' AND tg_tag='GRANT' THEN PERFORM pg_advisory_xact_lock(77); END IF; END $$; CREATE EVENT TRIGGER pause_preparation ON ddl_command_end EXECUTE FUNCTION public.pause_preparation(); SELECT pg_advisory_lock(77)").await.unwrap();
    let before = db.snapshot().await;
    let settings = db.settings(true);
    let runner = tokio::spawn(async move {
        prepare_database(&settings, Preparation::Init { role_prefix: None }).await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let blocked: bool = db.client.query_one("SELECT EXISTS(SELECT FROM pg_stat_activity WHERE application_name='blkit_migration_test' AND wait_event='advisory')", &[]).await.unwrap().get(0);
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("runner did not reach late SQL lock");
    assert!(
        !db.client
            .query_one("SELECT pg_try_advisory_lock($1)", &[&LOCK])
            .await
            .unwrap()
            .get::<_, bool>(0),
        "preparation lock released before grant completion"
    );
    runner.abort();
    assert!(runner.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let count: i64 = db.client.query_one("SELECT count(*) FROM pg_stat_activity WHERE application_name='blkit_migration_test'", &[]).await.unwrap().get(0);
            if count == 0 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("cancelled SQL retained a server session while its blocker remained held");
    assert!(
        db.client
            .query_one("SELECT pg_try_advisory_lock($1)", &[&LOCK])
            .await
            .unwrap()
            .get::<_, bool>(0),
        "cancelled SQL retained the preparation lock while its blocker remained held"
    );
    db.client
        .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .await
        .unwrap();
    db.client
        .execute("SELECT pg_advisory_unlock(77)", &[])
        .await
        .unwrap();
    assert!(
        before == db.snapshot().await,
        "cancelled SQL committed partial preparation"
    );
    db.client
        .batch_execute(
            "DROP EVENT TRIGGER pause_preparation; DROP FUNCTION public.pause_preparation()",
        )
        .await
        .unwrap();
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::Initialized);
}

#[tokio::test]
async fn failed_rerun_preserves_the_prior_valid_version_data_and_grants() {
    let db = Database::start().await;
    db.init().await.unwrap();
    db.client.batch_execute("INSERT INTO public.instances(id,namespace,version,process,input,status,created_at,updated_at,checkpoint) VALUES ('retained','n','v','p','{}','queued',1,1,'checkpoint'); CREATE FUNCTION public.reject_rerun() RETURNS event_trigger LANGUAGE plpgsql AS $$ BEGIN IF current_user='blkit_owner' AND tg_tag='ALTER DEFAULT PRIVILEGES' THEN RAISE EXCEPTION 'migration-secret-sentinel'; END IF; END $$; CREATE EVENT TRIGGER reject_rerun ON ddl_command_end EXECUTE FUNCTION public.reject_rerun()").await.unwrap();
    let before = db.snapshot().await;
    let error = prepare_database(
        &db.settings(false),
        Preparation::Migrate { role_prefix: None },
    )
    .await
    .unwrap_err();
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
    assert!(
        before == db.snapshot().await,
        "failed rerun changed valid preparation"
    );
    db.client
        .batch_execute("DROP EVENT TRIGGER reject_rerun; DROP FUNCTION public.reject_rerun()")
        .await
        .unwrap();
    assert_eq!(
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
}

#[tokio::test]
async fn migrate_uses_persisted_custom_prefix_while_init_checks_its_default() {
    let db = Database::start().await;
    let endpoint = db.url.rsplit_once('@').unwrap().1;
    let settings = EffectivePostgresConfig::from_iter(
        None,
        [
            ("BLKIT_ADMIN_DATABASE_URL", db.url.clone()),
            ("BLKIT_POSTGRES_INSECURE_DEV", "true".into()),
            (
                "BLKIT_API_DATABASE_URL",
                format!("postgresql://tenant_api:{SENTINEL}@{endpoint}"),
            ),
            (
                "BLKIT_WORKER_DATABASE_URL",
                format!("postgresql://tenant_worker:{SENTINEL}@{endpoint}"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        prepare_database(
            &settings,
            Preparation::Init {
                role_prefix: Some("tenant")
            }
        )
        .await
        .unwrap(),
        PreparationOutcome::Initialized
    );
    db.assert_refused_unchanged(Preparation::Init { role_prefix: None })
        .await;
    assert_eq!(
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
    let row = db
        .client
        .query_one(
            "SELECT role_prefix, owner_role FROM public.blkit_deployment",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(row.get::<_, &str>(0), "tenant");
    assert_eq!(row.get::<_, &str>(1), "tenant_owner");
}

#[tokio::test]
async fn preparation_preserves_unrelated_objects_owners_data_and_acls() {
    let db = Database::start().await;
    db.client.batch_execute("CREATE TABLE public.unrelated(id text); INSERT INTO public.unrelated VALUES ('keep'); GRANT SELECT ON public.unrelated TO PUBLIC").await.unwrap();
    let sql = "SELECT jsonb_build_object('owner', pg_get_userbyid(c.relowner), 'acl', c.relacl, 'data', (SELECT jsonb_agg(to_jsonb(u)) FROM public.unrelated u))::text FROM pg_class c WHERE c.oid='public.unrelated'::regclass";
    let before: String = db.client.query_one(sql, &[]).await.unwrap().get(0);
    db.init().await.unwrap();
    prepare_database(
        &db.settings(false),
        Preparation::Migrate { role_prefix: None },
    )
    .await
    .unwrap();
    let after: String = db.client.query_one(sql, &[]).await.unwrap().get(0);
    assert_eq!(before, after);
}

// Missing graph/rule validation would accept the shape and repair the stale ACL.
async fn assert_unknown_behavior_refused_before_reconciliation(ddl: &str) {
    let db = Database::start().await;
    db.init().await.unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute("GRANT UPDATE (checkpoint) ON TABLE public.instances TO blkit_api")
        .await
        .unwrap();
    db.client.batch_execute(ddl).await.unwrap();
    for mode in [
        Preparation::Init { role_prefix: None },
        Preparation::Migrate { role_prefix: None },
    ] {
        let error = db.assert_refused_unchanged(mode).await;
        assert!(
            error.to_string().contains("schema"),
            "unknown behavior was not rejected during schema validation: {error}"
        );
    }
}

#[tokio::test]
async fn unknown_update_rule_is_refused_without_reconciliation_or_data_changes() {
    assert_unknown_behavior_refused_before_reconciliation(
        "CREATE RULE suppress_updates AS ON UPDATE TO public.instances DO INSTEAD NOTHING",
    )
    .await;
}

#[tokio::test]
async fn unknown_inheritance_child_is_refused_without_reconciliation_or_data_changes() {
    assert_unknown_behavior_refused_before_reconciliation(
        "CREATE TABLE public.unrelated_child () INHERITS (public.instances); INSERT INTO public.unrelated_child(id,namespace,version,process,input,status,created_at,updated_at) VALUES ('child-row','n','v','p','{}','queued',1,1)",
    ).await;
}

#[tokio::test]
async fn unknown_inheritance_parent_is_refused_without_reconciliation_or_data_changes() {
    assert_unknown_behavior_refused_before_reconciliation(
        "CREATE TABLE public.unrelated_parent (id text); ALTER TABLE public.instances INHERIT public.unrelated_parent",
    ).await;
}

#[tokio::test]
async fn unknown_partition_parent_is_refused_without_reconciliation_or_data_changes() {
    assert_unknown_behavior_refused_before_reconciliation(
        "CREATE TABLE public.unrelated_partitioned (LIKE public.blkit_deployment INCLUDING ALL) PARTITION BY LIST(singleton); ALTER TABLE public.unrelated_partitioned ATTACH PARTITION public.blkit_deployment FOR VALUES IN (true)",
    ).await;
}

#[tokio::test]
async fn blocked_sql_lock_wait_is_bounded_and_preserves_prepared_state() {
    let db = Database::start().await;
    db.init().await.unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute("BEGIN; LOCK TABLE public.instances IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let before = db.snapshot().await;
    let start = Instant::now();
    let attempt = tokio::time::timeout(
        Duration::from_secs(3),
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None },
        ),
    )
    .await;
    // Keep the conflicting SQL lock held until failure and rollback are verified.
    let error = attempt.expect("SQL lock wait was not bounded").unwrap_err();
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(
        before == db.snapshot().await,
        "SQL lock failure changed prepared state"
    );
    db.client.batch_execute("ROLLBACK").await.unwrap();
    assert_eq!(
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
}

async fn load_legacy(db: &Database) {
    db.client
        .batch_execute("SET TIME ZONE 'UTC'")
        .await
        .unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/schema.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    assert_legacy_data(db, &expected_legacy_data()).await;
}

fn expected_legacy_data() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/postgres-legacy/expected-data.json")).unwrap()
}

async fn assert_legacy_data(db: &Database, expected: &serde_json::Value) {
    for (table, expected_rows) in expected.as_object().unwrap() {
        let json: String = db
            .client
            .query_one(
                &format!(
                    "SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]')::text FROM public.{table} t"
                ),
                &[],
            )
            .await
            .unwrap()
            .get(0);
        let mut actual: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        let mut expected = expected_rows.as_array().unwrap().clone();
        actual.sort_by_key(serde_json::Value::to_string);
        expected.sort_by_key(serde_json::Value::to_string);
        assert_eq!(actual, expected, "persisted table {table}");
    }
}

// Refusing the actual frozen baseline or omitting its backfill breaks this upgrade.
#[tokio::test]
async fn legacy_adoption_preserves_every_row_and_checkpoint_except_null_queue_backfill() {
    let db = Database::start().await;
    load_legacy(&db).await;
    db.client.batch_execute("UPDATE public.instances SET queued_at_ms=NULL WHERE id='running'; UPDATE public.instances SET queued_at_ms=42 WHERE id='reply'").await.unwrap();
    let checkpoints: Vec<(String, Vec<u8>)> = db.client.query("SELECT id, convert_to(checkpoint,'UTF8') FROM public.instances WHERE checkpoint IS NOT NULL ORDER BY id", &[]).await.unwrap().iter().map(|r| (r.get(0),r.get(1))).collect();
    let error = db
        .assert_refused_unchanged(Preparation::Init { role_prefix: None })
        .await;
    assert!(error.to_string().contains("migrate"));
    assert_eq!(
        prepare_database(
            &db.settings(true),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::Migrated
    );
    let mut expected = expected_legacy_data();
    for row in expected["instances"].as_array_mut().unwrap() {
        if row["id"] == "reply" {
            row["queued_at_ms"] = 42.into();
        }
    }
    assert_legacy_data(&db, &expected).await;
    let after: Vec<(String, Vec<u8>)> = db.client.query("SELECT id, convert_to(checkpoint,'UTF8') FROM public.instances WHERE checkpoint IS NOT NULL ORDER BY id", &[]).await.unwrap().iter().map(|r| (r.get(0),r.get(1))).collect();
    assert_eq!(checkpoints, after, "exact checkpoint bytes");
    assert_eq!(db.client.query_one("SELECT count(*) FROM public.blkit_schema_migrations WHERE version=1 AND identity='blkit-postgres-v1' AND checksum='e7d49695bd5c35b92aaaf76e9de8572b12215c01258f1b193408351573f17a04' AND applied_at <= clock_timestamp()", &[]).await.unwrap().get::<_, i64>(0), 1);
    let before = db.snapshot().await;
    assert_eq!(
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
    assert!(
        before == db.snapshot().await,
        "rerun changed persisted data/history/passwords"
    );
}

// Adoption must reach the history write but roll back DDL/backfill/roles/grants on failure.
#[tokio::test]
async fn legacy_adoption_history_failure_rolls_back_everything_and_retry_succeeds() {
    let db = Database::start().await;
    load_legacy(&db).await;
    db.client.batch_execute("UPDATE public.instances SET queued_at_ms=NULL WHERE id='running'; CREATE FUNCTION public.reject_history() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'migration-secret-sentinel'; END $$; CREATE FUNCTION public.install_history_fault() RETURNS event_trigger LANGUAGE plpgsql AS $$ BEGIN IF tg_tag='ALTER DEFAULT PRIVILEGES' AND current_user='blkit_owner' AND NOT EXISTS(SELECT FROM pg_trigger WHERE tgrelid=to_regclass('public.blkit_schema_migrations') AND tgname='reject_history') THEN CREATE TRIGGER reject_history BEFORE INSERT ON public.blkit_schema_migrations FOR EACH ROW EXECUTE FUNCTION public.reject_history(); END IF; END $$; CREATE EVENT TRIGGER install_history_fault ON ddl_command_end EXECUTE FUNCTION public.install_history_fault()").await.unwrap();
    let before = db.snapshot().await;
    let error = prepare_database(
        &db.settings(true),
        Preparation::Migrate { role_prefix: None },
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot record successful migration history"),
        "wrong failure stage: {error}"
    );
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
    assert!(
        before == db.snapshot().await,
        "failed adoption stranded changes"
    );
    db.client.batch_execute("DROP EVENT TRIGGER install_history_fault; DROP FUNCTION public.install_history_fault(); DROP FUNCTION public.reject_history()").await.unwrap();
    assert_eq!(
        prepare_database(
            &db.settings(true),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::Migrated
    );
    assert_legacy_data(&db, &expected_legacy_data()).await;
}

#[tokio::test]
async fn legacy_shape_drift_partial_metadata_rules_and_edges_are_refused_populated() {
    let db = Database::start().await;
    load_legacy(&db).await;
    for (change, restore) in [
        (
            "ALTER TABLE public.instances ADD COLUMN unexpected text",
            "ALTER TABLE public.instances DROP COLUMN unexpected",
        ),
        (
            "ALTER TABLE public.instances ALTER COLUMN generation TYPE integer",
            "ALTER TABLE public.instances ALTER COLUMN generation TYPE bigint",
        ),
        (
            "ALTER TABLE public.instances ALTER COLUMN attempt SET DEFAULT 1",
            "ALTER TABLE public.instances ALTER COLUMN attempt SET DEFAULT 0",
        ),
        (
            "ALTER TABLE public.instances ADD CONSTRAINT unexpected CHECK (attempt>=0)",
            "ALTER TABLE public.instances DROP CONSTRAINT unexpected",
        ),
        (
            "ALTER TABLE public.input_requests DROP CONSTRAINT input_requests_instance_id_fkey",
            "ALTER TABLE public.input_requests ADD CONSTRAINT input_requests_instance_id_fkey FOREIGN KEY(instance_id) REFERENCES public.instances(id)",
        ),
        (
            "DROP INDEX public.input_requests_answered",
            "CREATE INDEX input_requests_answered ON public.input_requests(instance_id) WHERE status='answered'",
        ),
        (
            "CREATE INDEX unexpected ON public.instances(status)",
            "DROP INDEX public.unexpected",
        ),
        (
            "ALTER TABLE public.instances DISABLE TRIGGER instance_terminal_interests_notify",
            "ALTER TABLE public.instances ENABLE TRIGGER instance_terminal_interests_notify",
        ),
        (
            "ALTER FUNCTION public.notify_instance_interests() SECURITY DEFINER",
            "ALTER FUNCTION public.notify_instance_interests() SECURITY INVOKER",
        ),
        (
            "ALTER TABLE public.process_policies SET UNLOGGED",
            "ALTER TABLE public.process_policies SET LOGGED",
        ),
        (
            "ALTER TABLE public.instances ENABLE ROW LEVEL SECURITY",
            "ALTER TABLE public.instances DISABLE ROW LEVEL SECURITY",
        ),
        (
            "CREATE RULE suppress_updates AS ON UPDATE TO public.instances DO INSTEAD NOTHING",
            "DROP RULE suppress_updates ON public.instances",
        ),
        (
            "CREATE TABLE public.unrelated_child () INHERITS (public.instances); INSERT INTO public.unrelated_child(id,namespace,version,process,input,status,created_at,updated_at) VALUES ('child-row','n','v','p','{}','pending',1,1)",
            "DROP TABLE public.unrelated_child",
        ),
        (
            "CREATE TABLE public.unrelated_parent(id text); ALTER TABLE public.instances INHERIT public.unrelated_parent",
            "ALTER TABLE public.instances NO INHERIT public.unrelated_parent; DROP TABLE public.unrelated_parent",
        ),
        (
            "CREATE TABLE public.unrelated_partitioned (LIKE public.workers INCLUDING ALL) PARTITION BY LIST(id); ALTER TABLE public.unrelated_partitioned ATTACH PARTITION public.workers FOR VALUES IN ('worker-active','worker-draining')",
            "ALTER TABLE public.unrelated_partitioned DETACH PARTITION public.workers; DROP TABLE public.unrelated_partitioned",
        ),
        (
            "CREATE TABLE public.blkit_deployment(singleton boolean)",
            "DROP TABLE public.blkit_deployment",
        ),
        (
            "CREATE TABLE public.blkit_schema_migrations(version bigint)",
            "DROP TABLE public.blkit_schema_migrations",
        ),
        (
            "CREATE TABLE public.blkit_future(version bigint)",
            "DROP TABLE public.blkit_future",
        ),
    ] {
        db.client.batch_execute(change).await.unwrap();
        for mode in [
            Preparation::Init { role_prefix: None },
            Preparation::Migrate { role_prefix: None },
        ] {
            let error = db.assert_refused_unchanged(mode).await;
            assert!(
                error.to_string().contains("schema") || error.to_string().contains("objects"),
                "not a structural refusal: {error}"
            );
        }
        db.client.batch_execute(restore).await.unwrap();
    }
    // An older baseline missing a current column is not silently repaired by IF NOT EXISTS.
    db.client
        .batch_execute("ALTER TABLE public.instances DROP COLUMN checkpoint")
        .await
        .unwrap();
    let error = db
        .assert_refused_unchanged(Preparation::Migrate { role_prefix: None })
        .await;
    assert!(error.to_string().contains("schema"));
}

fn prefixed_settings(db: &Database, prefix: &str) -> EffectivePostgresConfig {
    let endpoint = db.url.rsplit_once('@').unwrap().1;
    EffectivePostgresConfig::from_iter(
        None,
        [
            ("BLKIT_ADMIN_DATABASE_URL", db.url.clone()),
            ("BLKIT_POSTGRES_INSECURE_DEV", "true".into()),
            (
                "BLKIT_API_DATABASE_URL",
                format!("postgresql://{prefix}_api:{SENTINEL}@{endpoint}"),
            ),
            (
                "BLKIT_WORKER_DATABASE_URL",
                format!("postgresql://{prefix}_worker:{SENTINEL}@{endpoint}"),
            ),
        ],
    )
    .unwrap()
}

#[tokio::test]
async fn legacy_adoption_custom_prefix_avoids_collisions_and_reruns_preserve_identity() {
    let db = Database::start().await;
    load_legacy(&db).await;
    db.client.batch_execute("CREATE ROLE blkit_api LOGIN PASSWORD 'migration-secret-sentinel'; CREATE ROLE unrelated_owner; CREATE TABLE public.unrelated(id text); INSERT INTO public.unrelated VALUES ('keep'); ALTER TABLE public.unrelated OWNER TO unrelated_owner; GRANT SELECT ON public.unrelated TO PUBLIC; ALTER DEFAULT PRIVILEGES FOR ROLE unrelated_owner IN SCHEMA public GRANT SELECT ON TABLES TO PUBLIC").await.unwrap();
    let before = db.snapshot().await;
    let error = prepare_database(
        &db.settings(true),
        Preparation::Migrate { role_prefix: None },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("collision"));
    assert!(
        before == db.snapshot().await,
        "role collision changed legacy state"
    );
    let unrelated_sql = "SELECT jsonb_build_object('table', (SELECT to_jsonb(c)-'relpages'-'reltuples'-'relallvisible' FROM pg_class c WHERE c.oid='public.unrelated'::regclass), 'rows', (SELECT jsonb_agg(to_jsonb(t)) FROM public.unrelated t), 'defaults', (SELECT jsonb_agg(to_jsonb(d)) FROM pg_default_acl d WHERE d.defaclrole='unrelated_owner'::regrole), 'collision', (SELECT to_jsonb(r) FROM pg_authid r WHERE rolname='blkit_api'))::text";
    let unrelated: String = db
        .client
        .query_one(unrelated_sql, &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        prepare_database(
            &prefixed_settings(&db, "tenant"),
            Preparation::Migrate {
                role_prefix: Some("tenant")
            }
        )
        .await
        .unwrap(),
        PreparationOutcome::Migrated
    );
    assert_legacy_data(&db, &expected_legacy_data()).await;
    let passwords: Vec<String> = db.client.query("SELECT rolpassword FROM pg_authid WHERE rolname IN ('tenant_api','tenant_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    for prefix in [None, Some("tenant")] {
        assert_eq!(
            prepare_database(
                &db.settings(false),
                Preparation::Migrate {
                    role_prefix: prefix
                }
            )
            .await
            .unwrap(),
            PreparationOutcome::AlreadyCurrent
        );
    }
    db.assert_refused_unchanged(Preparation::Migrate {
        role_prefix: Some("other"),
    })
    .await;
    db.assert_refused_unchanged(Preparation::Migrate {
        role_prefix: Some("blkit"),
    })
    .await;
    db.assert_refused_unchanged(Preparation::Init { role_prefix: None })
        .await;
    let after: Vec<String> = db.client.query("SELECT rolpassword FROM pg_authid WHERE rolname IN ('tenant_api','tenant_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(passwords == after, "migrate rotated existing passwords");
    assert_eq!(
        unrelated,
        db.client
            .query_one(unrelated_sql, &[])
            .await
            .unwrap()
            .get::<_, String>(0)
    );
    assert_eq!(
        db.client
            .query_one("SELECT role_prefix FROM public.blkit_deployment", &[])
            .await
            .unwrap()
            .get::<_, &str>(0),
        "tenant"
    );
    assert!(db.client.query_one("SELECT NOT EXISTS(SELECT FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname <> 'unrelated' AND pg_get_userbyid(c.relowner)<>'tenant_owner') AND (SELECT pg_get_userbyid(proowner)='tenant_owner' FROM pg_proc WHERE oid='public.notify_instance_interests()'::regprocedure)", &[]).await.unwrap().get::<_, bool>(0), "known table/index/function ownership not transferred");
}

#[tokio::test]
async fn legacy_adoption_requires_only_new_login_secrets_and_validates_their_identity() {
    let db = Database::start().await;
    load_legacy(&db).await;
    let error = db
        .assert_refused_unchanged(Preparation::Migrate { role_prefix: None })
        .await;
    assert!(error.to_string().contains("BLKIT_API_DATABASE_URL"));
    for (key, url) in [
        (
            "BLKIT_API_DATABASE_URL",
            format!(
                "postgresql://wrong:{SENTINEL}@{}",
                db.url.rsplit_once('@').unwrap().1
            ),
        ),
        (
            "BLKIT_WORKER_DATABASE_URL",
            format!("postgresql://blkit_worker:{SENTINEL}@127.0.0.1:1/other"),
        ),
    ] {
        let settings = EffectivePostgresConfig::from_iter(
            None,
            [
                ("BLKIT_ADMIN_DATABASE_URL", db.url.clone()),
                ("BLKIT_POSTGRES_INSECURE_DEV", "true".into()),
                (
                    "BLKIT_API_DATABASE_URL",
                    format!(
                        "postgresql://blkit_api:{SENTINEL}@{}",
                        db.url.rsplit_once('@').unwrap().1
                    ),
                ),
                (
                    "BLKIT_WORKER_DATABASE_URL",
                    format!(
                        "postgresql://blkit_worker:{SENTINEL}@{}",
                        db.url.rsplit_once('@').unwrap().1
                    ),
                ),
                (key, url),
            ],
        )
        .unwrap();
        let before = db.snapshot().await;
        let error = prepare_database(&settings, Preparation::Migrate { role_prefix: None })
            .await
            .unwrap_err();
        assert!(error.to_string().contains(key));
        assert!(!format!("{error:?} {error}").contains(SENTINEL));
        assert!(
            before == db.snapshot().await,
            "invalid login credentials mutated legacy"
        );
    }
    assert_eq!(
        prepare_database(
            &db.settings(true),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::Migrated
    );
    // Existing managed logins need neither runtime URL, and passwords are not reset.
    assert_eq!(
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
    db.client
        .batch_execute("DROP OWNED BY blkit_worker; DROP ROLE blkit_worker")
        .await
        .unwrap();
    let endpoint = db.url.rsplit_once('@').unwrap().1;
    let only_worker = EffectivePostgresConfig::from_iter(
        None,
        [
            ("BLKIT_ADMIN_DATABASE_URL", db.url.clone()),
            ("BLKIT_POSTGRES_INSECURE_DEV", "true".into()),
            (
                "BLKIT_WORKER_DATABASE_URL",
                format!("postgresql://blkit_worker:{SENTINEL}@{endpoint}"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        prepare_database(&only_worker, Preparation::Migrate { role_prefix: None })
            .await
            .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
}

#[tokio::test]
async fn legacy_adoption_noninheriting_owner_succeeds_but_missing_authority_rolls_back() {
    let mut db = Database::start().await;
    load_legacy(&db).await;
    db.client.batch_execute("CREATE ROLE deployer LOGIN NOINHERIT PASSWORD 'migration-secret-sentinel'; ALTER DATABASE postgres OWNER TO deployer; GRANT USAGE, CREATE ON SCHEMA public TO deployer; GRANT ALL ON ALL TABLES IN SCHEMA public TO deployer").await.unwrap();
    for table in expected_legacy_data().as_object().unwrap().keys() {
        db.client
            .batch_execute(&format!("ALTER TABLE public.{table} OWNER TO deployer"))
            .await
            .unwrap();
    }
    db.client
        .batch_execute("ALTER FUNCTION public.notify_instance_interests() OWNER TO deployer")
        .await
        .unwrap();
    db.url = db
        .url
        .replace("postgres:postgres@", &format!("deployer:{SENTINEL}@"));
    let before = db.snapshot().await;
    let error = prepare_database(
        &db.settings(true),
        Preparation::Migrate { role_prefix: None },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("role-creation authority"));
    assert!(before == db.snapshot().await);
    db.client
        .batch_execute(
            "ALTER ROLE deployer CREATEROLE; ALTER TABLE public.instances OWNER TO postgres",
        )
        .await
        .unwrap();
    let before = db.snapshot().await;
    let error = prepare_database(
        &db.settings(true),
        Preparation::Migrate { role_prefix: None },
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("ownership") || error.to_string().contains("DDL authority"),
        "missing object ownership accepted: {error}"
    );
    assert!(before == db.snapshot().await);
    // Operator supplies the missing authority on only the known historical objects.
    for table in expected_legacy_data().as_object().unwrap().keys() {
        db.client
            .batch_execute(&format!("ALTER TABLE public.{table} OWNER TO deployer"))
            .await
            .unwrap();
    }
    db.client
        .batch_execute("ALTER FUNCTION public.notify_instance_interests() OWNER TO deployer")
        .await
        .unwrap();
    assert_eq!(
        prepare_database(
            &db.settings(true),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::Migrated
    );
    assert_legacy_data(&db, &expected_legacy_data()).await;
    assert_eq!(
        prepare_database(
            &db.settings(false),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::AlreadyCurrent
    );
}

#[tokio::test]
async fn concurrent_legacy_migrations_and_init_share_the_namespace_lock() {
    let mut db = Database::start().await;
    load_legacy(&db).await;
    db.url.push_str("?application_name=blkit_migration_test");
    db.client
        .execute("SELECT pg_advisory_lock($1)", &[&LOCK])
        .await
        .unwrap();
    let settings = db.settings(true);
    let first_settings = db.settings(true);
    let first = tokio::spawn(async move {
        prepare_database(&first_settings, Preparation::Migrate { role_prefix: None }).await
    });
    wait_for_admin_query(&db.client, "%pg_try_advisory_lock%").await;
    let error = db
        .assert_refused_unchanged(Preparation::Init { role_prefix: None })
        .await;
    assert!(error.to_string().contains("migrate"));
    let second = tokio::spawn(async move {
        prepare_database(&settings, Preparation::Migrate { role_prefix: None }).await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let waiting: i64 = db.client.query_one("SELECT count(*) FROM pg_stat_activity WHERE application_name='blkit_migration_test' AND query LIKE '%pg_try_advisory_lock%'", &[]).await.unwrap().get(0);
            if waiting == 2 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("both migrations did not contend on the namespace lock");
    db.client
        .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .await
        .unwrap();
    let mut outcomes = [
        first.await.unwrap().unwrap(),
        second.await.unwrap().unwrap(),
    ];
    outcomes.sort_by_key(|o| matches!(o, PreparationOutcome::AlreadyCurrent));
    assert_eq!(
        outcomes,
        [
            PreparationOutcome::Migrated,
            PreparationOutcome::AlreadyCurrent
        ]
    );
    assert_eq!(
        db.client
            .query_one("SELECT count(*) FROM public.blkit_schema_migrations", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
    assert_legacy_data(&db, &expected_legacy_data()).await;
    assert_eq!(db.init().await.unwrap(), PreparationOutcome::AlreadyCurrent);
}

#[tokio::test]
async fn legacy_shape_is_revalidated_under_lock_before_any_adoption_mutation() {
    let mut db = Database::start().await;
    load_legacy(&db).await;
    db.url.push_str("?application_name=blkit_migration_test");
    db.client
        .execute("SELECT pg_advisory_lock($1)", &[&LOCK])
        .await
        .unwrap();
    let settings = db.settings(true);
    let runner = tokio::spawn(async move {
        prepare_database(&settings, Preparation::Migrate { role_prefix: None }).await
    });
    wait_for_admin_query(&db.client, "%pg_try_advisory_lock%").await;
    db.client
        .batch_execute(
            "CREATE RULE suppress_updates AS ON UPDATE TO public.instances DO INSTEAD NOTHING",
        )
        .await
        .unwrap();
    let before = db.snapshot().await;
    db.client
        .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .await
        .unwrap();
    let error = runner.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("schema"));
    assert!(
        before == db.snapshot().await,
        "under-lock legacy refusal changed database"
    );
}

// Real SCRAM logins, not a Store constructor (runtime startup DDL remains task 6.1).
#[tokio::test]
async fn legacy_adoption_installs_operational_column_grants_and_known_ownership() {
    let db = Database::start().await;
    load_legacy(&db).await;
    assert_eq!(
        prepare_database(
            &db.settings(true),
            Preparation::Migrate { role_prefix: None }
        )
        .await
        .unwrap(),
        PreparationOutcome::Migrated
    );
    assert!(db.client.query_one("SELECT NOT EXISTS(SELECT FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND pg_get_userbyid(c.relowner)<>'blkit_owner') AND (SELECT pg_get_userbyid(proowner)='blkit_owner' FROM pg_proc WHERE oid='public.notify_instance_interests()'::regprocedure)", &[]).await.unwrap().get::<_, bool>(0));
    let mut api_config = db.config.clone();
    api_config.user("blkit_api").password(SENTINEL);
    let api = connect(&api_config).await;
    let mut worker_config = db.config.clone();
    worker_config.user("blkit_worker").password(SENTINEL);
    let worker = connect(&worker_config).await;
    for client in [&api, &worker] {
        for table in [
            "instances",
            "input_requests",
            "instance_attachments",
            "instance_interests",
            "workers",
            "worker_capabilities",
            "process_policies",
            "blkit_deployment",
            "blkit_schema_migrations",
        ] {
            client
                .query_one(&format!("SELECT count(*) FROM public.{table}"), &[])
                .await
                .unwrap();
        }
        for sql in [
            "UPDATE public.blkit_schema_migrations SET version=2",
            "CREATE TABLE public.forbidden(id text)",
            "SET ROLE blkit_owner",
        ] {
            assert!(
                client.batch_execute(sql).await.is_err(),
                "forbidden SQL was accepted"
            );
        }
    }
    api.batch_execute("INSERT INTO public.instances(id,namespace,version,process,input,status,created_at,updated_at) VALUES ('new','n','v','p','{}','pending',1,1); INSERT INTO public.instance_attachments(instance_id,ordinal,object_key,filename,byte_length) VALUES ('new',0,'opaque/key','file',0); INSERT INTO public.instance_interests(instance_id,channel,expires_at) VALUES ('new','adoption_notice',clock_timestamp()+interval '1 hour')").await.unwrap();
    worker.batch_execute("INSERT INTO public.workers(id,heartbeat_at) VALUES ('new-worker',1); INSERT INTO public.worker_capabilities(worker_id,namespace,version,process) VALUES ('new-worker','n','v','p'); INSERT INTO public.process_policies(namespace,version,process) VALUES ('n','v','p'); UPDATE public.instances SET checkpoint='opaque checkpoint bytes',owner_id='new-worker',lease_until=2,generation=1 WHERE id='new'; INSERT INTO public.input_requests(instance_id,request_id,prompt,schema) VALUES ('new','r','prompt','schema TEXT')").await.unwrap();
    api.batch_execute("UPDATE public.input_requests SET status='answered',response='reply TEXT, not JSON' WHERE instance_id='new'; UPDATE public.instances SET status='cancelled',owner_id=NULL,lease_until=NULL WHERE id='new'").await.unwrap();
    assert_eq!(
        worker
            .query_one(
                "SELECT response FROM public.input_requests WHERE instance_id='new'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, &str>(0),
        "reply TEXT, not JSON"
    );
    assert_eq!(
        api.query_one(
            "SELECT checkpoint FROM public.instances WHERE id='new'",
            &[]
        )
        .await
        .unwrap()
        .get::<_, &str>(0),
        "opaque checkpoint bytes"
    );
    for sql in [
        "UPDATE public.instances SET checkpoint='forbidden'",
        "UPDATE public.input_requests SET schema='forbidden'",
        "INSERT INTO public.workers(id,heartbeat_at) VALUES ('forbidden',1)",
    ] {
        assert!(api.batch_execute(sql).await.is_err());
    }
    for sql in [
        "UPDATE public.input_requests SET response='forbidden'",
        "INSERT INTO public.instances(id) VALUES ('forbidden')",
        "DELETE FROM public.instance_interests",
        "UPDATE public.instance_attachments SET filename='forbidden'",
        "UPDATE public.process_policies SET retry_policy='forbidden'",
    ] {
        assert!(worker.batch_execute(sql).await.is_err());
    }
    assert!(
        !db.client
            .query_one(
                "SELECT has_table_privilege('public','public.instances','SELECT')",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
}

#[tokio::test]
async fn explicit_migrate_prefix_is_validated_before_loading_credentials_or_connecting() {
    let settings = EffectivePostgresConfig::from_iter(None, [] as [(&str, &str); 0]).unwrap();
    for prefix in [
        "",
        "pg_reserved",
        "uppercaseA",
        "has-dash",
        "unsafe;SQL",
        &"x".repeat(57),
    ] {
        let error = prepare_database(
            &settings,
            Preparation::Migrate {
                role_prefix: Some(prefix),
            },
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("role prefix"),
            "prefix did not fail before credential resolution: {error}"
        );
        assert!(!error.to_string().contains(prefix) || prefix.is_empty());
    }
}

async fn assert_table_profile_drift_refused(managed: bool, ddl: &str) {
    let db = Database::start().await;
    if managed {
        db.init().await.unwrap();
        db.client
            .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
            .await
            .unwrap();
        db.client
            .batch_execute("GRANT UPDATE(checkpoint) ON public.instances TO blkit_api")
            .await
            .unwrap();
    } else {
        load_legacy(&db).await;
    }
    db.client.batch_execute(ddl).await.unwrap();
    let before = db.snapshot().await;
    let error = prepare_database(
        &db.settings(true),
        Preparation::Migrate { role_prefix: None },
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("schema"),
        "not refused at structural validation: {error}"
    );
    assert!(
        before == db.snapshot().await,
        "table-profile drift triggered preparation or grant repair"
    );
    db.assert_refused_unchanged(Preparation::Init { role_prefix: None })
        .await;
}

#[tokio::test]
async fn legacy_replica_identity_drift_is_refused() {
    assert_table_profile_drift_refused(false, "ALTER TABLE public.instances REPLICA IDENTITY FULL")
        .await;
}

#[tokio::test]
async fn managed_replica_identity_drift_is_refused() {
    assert_table_profile_drift_refused(true, "ALTER TABLE public.instances REPLICA IDENTITY FULL")
        .await;
}

#[tokio::test]
async fn legacy_relation_options_drift_is_refused() {
    assert_table_profile_drift_refused(
        false,
        "ALTER TABLE public.instances SET (autovacuum_enabled=false)",
    )
    .await;
}

#[tokio::test]
async fn managed_relation_options_drift_is_refused() {
    assert_table_profile_drift_refused(
        true,
        "ALTER TABLE public.instances SET (autovacuum_enabled=false)",
    )
    .await;
}

#[tokio::test]
async fn legacy_access_method_drift_is_refused() {
    // A named alias exercises the descriptor without extensions.
    assert_table_profile_drift_refused(false, "CREATE ACCESS METHOD alternative_heap TYPE TABLE HANDLER heap_tableam_handler; ALTER TABLE public.instances SET ACCESS METHOD alternative_heap").await;
}

#[tokio::test]
async fn managed_access_method_drift_is_refused() {
    assert_table_profile_drift_refused(true, "CREATE ACCESS METHOD alternative_heap TYPE TABLE HANDLER heap_tableam_handler; ALTER TABLE public.instances SET ACCESS METHOD alternative_heap").await;
}

// Generated RI trigger names contain OIDs; discover and quote the one INSERT check.
async fn set_internal_fk_insert_trigger(db: &Database, mode: &str) {
    let trigger: String = db.client.query_one(
        "SELECT quote_ident(t.tgname) FROM pg_trigger t JOIN pg_constraint con ON con.oid=t.tgconstraint WHERE con.conrelid='public.input_requests'::regclass AND con.conname='input_requests_instance_id_fkey' AND con.contype='f' AND t.tgisinternal AND t.tgrelid=con.conrelid AND t.tgtype & 4 <> 0",
        &[],
    ).await.unwrap().get(0);
    db.client
        .batch_execute(&format!(
            "ALTER TABLE public.input_requests {mode} TRIGGER {trigger}"
        ))
        .await
        .unwrap();
    assert_eq!(db.client.query_one("SELECT count(*) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND t.tgisinternal AND t.tgenabled<>'O'", &[]).await.unwrap().get::<_, i64>(0), 1, "changed more than the selected internal trigger");
    assert!(db.client.query_one("SELECT tgenabled='O' FROM pg_trigger WHERE tgrelid='public.input_requests'::regclass AND tgname='input_request_interests_notify' AND NOT tgisinternal", &[]).await.unwrap().get::<_, bool>(0), "notification trigger must remain canonical");
}

#[tokio::test]
async fn legacy_internal_fk_trigger_drift_is_refused_before_adoption() {
    let db = Database::start().await;
    load_legacy(&db).await;
    for mode in ["DISABLE", "ENABLE REPLICA", "ENABLE ALWAYS"] {
        set_internal_fk_insert_trigger(&db, mode).await;
        let before = db.snapshot().await;
        let error = prepare_database(
            &db.settings(true),
            Preparation::Migrate { role_prefix: None },
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("schema"),
            "not a structural refusal: {error}"
        );
        assert!(!format!("{error:?} {error}").contains(SENTINEL));
        assert!(
            before == db.snapshot().await,
            "internal trigger refusal mutated legacy state"
        );
        let error = db
            .assert_refused_unchanged(Preparation::Init { role_prefix: None })
            .await;
        assert!(error.to_string().contains("schema"));
    }
    assert_legacy_data(&db, &expected_legacy_data()).await;
}

#[tokio::test]
async fn managed_internal_fk_trigger_drift_is_refused_before_stale_acl_reconciliation() {
    let db = Database::start().await;
    db.init().await.unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute("GRANT UPDATE(checkpoint) ON public.instances TO blkit_api")
        .await
        .unwrap();
    set_internal_fk_insert_trigger(&db, "DISABLE").await;
    for mode in [
        Preparation::Migrate { role_prefix: None },
        Preparation::Init { role_prefix: None },
    ] {
        let error = db.assert_refused_unchanged(mode).await;
        assert!(
            error.to_string().contains("schema"),
            "not a structural refusal: {error}"
        );
    }
    assert_legacy_data(&db, &expected_legacy_data()).await;
}

async fn assert_internal_fk_trigger_drift_revalidated_under_lock(managed: bool) {
    let mut db = Database::start().await;
    if managed {
        db.init().await.unwrap();
        db.client
            .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
            .await
            .unwrap();
        db.client
            .batch_execute("GRANT UPDATE(checkpoint) ON public.instances TO blkit_api")
            .await
            .unwrap();
    } else {
        load_legacy(&db).await;
    }
    db.url.push_str("?application_name=blkit_migration_test");
    db.client
        .execute("SELECT pg_advisory_lock($1)", &[&LOCK])
        .await
        .unwrap();
    let settings = db.settings(true);
    let runner = tokio::spawn(async move {
        prepare_database(&settings, Preparation::Migrate { role_prefix: None }).await
    });
    wait_for_admin_query(&db.client, "%pg_try_advisory_lock%").await;
    set_internal_fk_insert_trigger(&db, "DISABLE").await;
    let before = db.snapshot().await;
    db.client
        .execute("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .await
        .unwrap();
    let error = runner.await.unwrap().unwrap_err();
    assert!(
        error.to_string().contains("schema"),
        "not a structural refusal: {error}"
    );
    assert!(!format!("{error:?} {error}").contains(SENTINEL));
    assert!(
        before == db.snapshot().await,
        "under-lock internal trigger refusal mutated state"
    );
    assert_legacy_data(&db, &expected_legacy_data()).await;
}

#[tokio::test]
async fn legacy_internal_fk_trigger_drift_while_waiting_is_refused_under_lock() {
    assert_internal_fk_trigger_drift_revalidated_under_lock(false).await;
}

#[tokio::test]
async fn managed_internal_fk_trigger_drift_while_waiting_is_refused_under_lock() {
    assert_internal_fk_trigger_drift_revalidated_under_lock(true).await;
}
