use blkit_cli::postgres_admin::provision_managed_roles;
use blkit_core::{
    postgres_config::{DatabaseRole, EffectivePostgresConfig, RoleDatabase},
    postgres_connection::PostgresConnectionOptions,
};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio_postgres::{Client, Config, NoTls};

#[path = "support/postgres_privileges.rs"]
mod privileges;

const INITIAL: &str = include_str!("../postgres/0001_initial.sql");
const API_PASSWORD: &str = "api'\\'; CREATE ROLE injected SUPERUSER LOGIN;--% /?";
const WORKER_PASSWORD: &str = "worker\\' dollar$ quote\" colon: at@ plus+";

struct Database {
    _container: ContainerAsync<Postgres>,
    client: Client,
    config: Config,
    options: PostgresConnectionOptions,
    admin: RoleDatabase,
}

impl Database {
    async fn start() -> Self {
        // Explicit SCRAM host auth, never trust: wrong-password checks below guard it.
        let container = Postgres::default()
            .with_tag("17.6-alpine")
            .with_env_var("POSTGRES_HOST_AUTH_METHOD", "scram-sha-256")
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(container.get_host().await.unwrap().to_string());
        let mut config = Config::new();
        config
            .host(&host)
            .port(container.get_host_port_ipv4(5432).await.unwrap())
            .user("postgres")
            .password("postgres")
            .dbname("postgres");
        let (client, connection) = config.connect(NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let options = PostgresConnectionOptions::new(None, Some("true")).unwrap();
        let admin_config = EffectivePostgresConfig::from_iter(
            None,
            [(
                "BLKIT_ADMIN_DATABASE_URL",
                url(&config, "postgres", "postgres"),
            )],
        )
        .unwrap();
        let admin = admin_config
            .resolve_role(DatabaseRole::Admin, &options)
            .unwrap();
        Self {
            _container: container,
            client,
            config,
            options,
            admin,
        }
    }

    fn settings(&self, prefix: &str) -> EffectivePostgresConfig {
        self.settings_from(vec![
            (
                "BLKIT_API_DATABASE_URL",
                url(&self.config, &format!("{prefix}_api"), API_PASSWORD),
            ),
            (
                "BLKIT_WORKER_DATABASE_URL",
                url(&self.config, &format!("{prefix}_worker"), WORKER_PASSWORD),
            ),
        ])
    }

    fn settings_from(&self, values: Vec<(&str, String)>) -> EffectivePostgresConfig {
        EffectivePostgresConfig::from_iter(None, values).unwrap()
    }

    async fn login(&self, user: &str, password: impl AsRef<[u8]>) -> bool {
        let mut config = self.config.clone();
        config.user(user).password(password);
        match config.connect(NoTls).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                client.query_one("SELECT current_user", &[]).await.is_ok()
            }
            Err(_) => false,
        }
    }

    async fn prepare(&mut self, prefix: &str) {
        let settings = self.settings(prefix);
        let tx = self.client.transaction().await.unwrap();
        tx.batch_execute(INITIAL).await.unwrap();
        provision_managed_roles(&tx, Some(prefix), &settings, &self.admin, &self.options)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
}

fn url(config: &Config, user: &str, password: impl AsRef<[u8]>) -> String {
    // Encode every non-alphanumeric password byte, independently of provisioning.
    fn encode(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() {
                    char::from(*byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect()
    }
    let encoded = encode(password.as_ref());
    let user = encode(user.as_bytes());
    let tokio_postgres::config::Host::Tcp(host) = &config.get_hosts()[0] else {
        panic!("TCP fixture")
    };
    format!(
        "postgresql://{user}:{encoded}@{host}:{}/postgres",
        config.get_ports()[0]
    )
}

async fn role_count(client: &Client, prefix: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_roles WHERE rolname = ANY($1)",
            &[&vec![
                format!("{prefix}_owner"),
                format!("{prefix}_api"),
                format!("{prefix}_worker"),
            ]],
        )
        .await
        .unwrap()
        .get(0)
}

// Catches missing roles/metadata, unsafe literal escaping, and trust-only false positives.
#[tokio::test]
async fn creates_default_roles_persists_identity_and_authenticates_encoded_passwords() {
    let mut db = Database::start().await;
    let settings = db.settings("blkit");
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    tx.batch_execute("SET LOCAL standard_conforming_strings = off")
        .await
        .unwrap();
    let roles = provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
        .await
        .unwrap();
    assert_eq!(
        (roles.prefix(), roles.owner(), roles.api(), roles.worker()),
        ("blkit", "blkit_owner", "blkit_api", "blkit_worker")
    );
    let row = tx.query_one("SELECT singleton, role_prefix, owner_role, api_role, worker_role FROM public.blkit_deployment", &[]).await.unwrap();
    assert!(row.get::<_, bool>(0));
    assert_eq!(row.get::<_, &str>(1), "blkit");
    assert_eq!(row.get::<_, &str>(2), "blkit_owner");
    assert_eq!(row.get::<_, &str>(3), "blkit_api");
    assert_eq!(row.get::<_, &str>(4), "blkit_worker");
    let rows = tx.query("SELECT rolname, rolcanlogin, rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls OR rolinherit AS elevated FROM pg_catalog.pg_roles WHERE rolname IN ('blkit_owner', 'blkit_api', 'blkit_worker') ORDER BY rolname", &[]).await.unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert!(!row.get::<_, bool>(2));
        assert_eq!(
            row.get::<_, bool>(1),
            row.get::<_, &str>(0) != "blkit_owner"
        );
    }
    assert_eq!(
        tx.query_one(
            "SELECT count(*) FROM pg_catalog.pg_roles WHERE rolname = 'injected'",
            &[]
        )
        .await
        .unwrap()
        .get::<_, i64>(0),
        0
    );
    tx.commit().await.unwrap();
    assert!(db.login("blkit_api", API_PASSWORD).await);
    assert!(db.login("blkit_worker", WORKER_PASSWORD).await);
    assert!(!db.login("blkit_api", "incorrect").await);
    assert!(!db.login("blkit_worker", "incorrect").await);
    assert!(!db.login("blkit_owner", "anything").await);
}

// Catches PostgreSQL treating a literal URL password as an already-encrypted verifier.
#[tokio::test]
async fn verifier_shaped_passwords_authenticate_as_literal_plaintext() {
    let mut db = Database::start().await;
    let md5_shaped = "md54ab2c5d00339c4b2a4e921d2dc4edec7";
    // Public upstream postgres-protocol test vector: this verifier encodes "secret".
    let scram_shaped = "SCRAM-SHA-256$4096:AQIDBAUGBwgJCgsMDQ4PEA==$8rrDg00OqaiWXJ7p+sCgHEIaBSHY89ZJl3mfIsf32oY=:05L1f+yZbiN8O0AnO40Og85NNRhvzTS57naKRWCcsIA=";
    let settings = db.settings_from(vec![
        (
            "BLKIT_API_DATABASE_URL",
            url(&db.config, "literal_api", md5_shaped),
        ),
        (
            "BLKIT_WORKER_DATABASE_URL",
            url(&db.config, "literal_worker", scram_shaped),
        ),
    ]);
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    provision_managed_roles(&tx, Some("literal"), &settings, &db.admin, &db.options)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let md5_login = db.login("literal_api", md5_shaped).await;
    let scram_login = db.login("literal_worker", scram_shaped).await;
    assert_eq!((md5_login, scram_login), (true, true));
    assert!(!db.login("literal_worker", "secret").await);
    let tx = db.client.transaction().await.unwrap();
    let empty = EffectivePostgresConfig::from_iter(None, Vec::<(&str, &str)>::new()).unwrap();
    provision_managed_roles(&tx, None, &empty, &db.admin, &db.options)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(db.login("literal_api", md5_shaped).await);
    assert!(db.login("literal_worker", scram_shaped).await);
}

// Catches an unnecessary UTF-8 conversion changing/rejecting decoded password bytes.
#[tokio::test]
async fn percent_encoded_non_utf8_password_authenticates_with_original_bytes() {
    let mut db = Database::start().await;
    let password = b"bytes-\xff\xfe-'\\";
    let settings = db.settings_from(vec![
        (
            "BLKIT_API_DATABASE_URL",
            url(&db.config, "bytes_api", password),
        ),
        (
            "BLKIT_WORKER_DATABASE_URL",
            url(&db.config, "bytes_worker", WORKER_PASSWORD),
        ),
    ]);
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    provision_managed_roles(&tx, Some("bytes"), &settings, &db.admin, &db.options)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(db.login("bytes_api", password).await);
    assert!(
        !db.login("bytes_api", b"bytes-\xef\xbf\xbd\xef\xbf\xbd-'\\")
            .await
    );
}

// Catches eager secret reads and accidental ALTER ROLE PASSWORD on reruns/upgrades.
#[tokio::test]
async fn admin_only_rerun_ignores_unused_settings_and_preserves_passwords() {
    let mut db = Database::start().await;
    db.prepare("custom").await;
    let hashes: Vec<String> = db.client.query("SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname IN ('custom_api', 'custom_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    let settings = db.settings_from(vec![
        ("BLKIT_API_DATABASE_URL", "malformed secret".into()),
        ("BLKIT_API_DATABASE_URL_FILE", "not-a-readable-file".into()),
        (
            "BLKIT_WORKER_DATABASE_URL_FILE",
            "not-a-readable-file".into(),
        ),
    ]);
    let tx = db.client.transaction().await.unwrap();
    let roles = provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
        .await
        .unwrap();
    assert_eq!(roles.prefix(), "custom");
    tx.commit().await.unwrap();
    // Even valid replacement secrets must not change existing credentials.
    let settings = db.settings_from(vec![
        (
            "BLKIT_API_DATABASE_URL",
            url(&db.config, "custom_api", "replacement"),
        ),
        (
            "BLKIT_WORKER_DATABASE_URL",
            url(&db.config, "custom_worker", "replacement"),
        ),
    ]);
    let tx = db.client.transaction().await.unwrap();
    provision_managed_roles(&tx, Some("custom"), &settings, &db.admin, &db.options)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let after: Vec<String> = db.client.query("SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname IN ('custom_api', 'custom_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(hashes == after, "password verifiers must not change");
    assert!(db.login("custom_api", API_PASSWORD).await);
    assert!(db.login("custom_worker", WORKER_PASSWORD).await);
    assert!(!db.login("custom_api", "replacement").await);
}

// Catches resolving the wrong missing role, both roles, or changing the surviving login.
#[tokio::test]
async fn missing_login_requires_only_its_own_secret() {
    let mut db = Database::start().await;
    db.prepare("lazy").await;
    for (missing, setting, unused, password) in [
        (
            "lazy_worker",
            "BLKIT_WORKER_DATABASE_URL",
            "BLKIT_API_DATABASE_URL",
            WORKER_PASSWORD,
        ),
        (
            "lazy_api",
            "BLKIT_API_DATABASE_URL",
            "BLKIT_WORKER_DATABASE_URL",
            API_PASSWORD,
        ),
    ] {
        db.client
            .batch_execute(&format!("DROP ROLE {missing}"))
            .await
            .unwrap();
        let settings = db.settings_from(vec![]);
        let tx = db.client.transaction().await.unwrap();
        let error = provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
            .await
            .unwrap_err();
        assert!(error.to_string().contains(setting));
        tx.rollback().await.unwrap();
        let settings = db.settings_from(vec![
            (setting, url(&db.config, missing, password)),
            (unused, "invalid unused URL".into()),
            (
                if unused == "BLKIT_API_DATABASE_URL" {
                    "BLKIT_API_DATABASE_URL_FILE"
                } else {
                    "BLKIT_WORKER_DATABASE_URL_FILE"
                },
                "unreadable-unused-file".into(),
            ),
        ]);
        let tx = db.client.transaction().await.unwrap();
        provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(db.login(missing, password).await);
    }
    assert!(db.login("lazy_api", API_PASSWORD).await);
    assert!(db.login("lazy_worker", WORKER_PASSWORD).await);
}

// Catches silently adopting a cluster-global role without this database's metadata.
#[tokio::test]
async fn role_name_collision_preserves_unrelated_role_and_objects() {
    let mut db = Database::start().await;
    db.client.batch_execute("CREATE ROLE collision_api LOGIN PASSWORD 'untouched'; CREATE TABLE public.unrelated (value text); INSERT INTO public.unrelated VALUES ('keep'); ALTER TABLE public.unrelated OWNER TO collision_api;").await.unwrap();
    let settings = db.settings("collision");
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    let error = provision_managed_roles(&tx, Some("collision"), &settings, &db.admin, &db.options)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("collision_api"));
    tx.rollback().await.unwrap();
    assert_eq!(role_count(&db.client, "collision").await, 1);
    assert!(db.login("collision_api", "untouched").await);
    let row = db.client.query_one("SELECT value, pg_catalog.pg_get_userbyid(c.relowner) FROM public.unrelated, pg_catalog.pg_class c WHERE c.oid = 'public.unrelated'::pg_catalog.regclass", &[]).await.unwrap();
    assert_eq!(row.get::<_, &str>(0), "keep");
    assert_eq!(row.get::<_, &str>(1), "collision_api");
}

// Catches identifier truncation, interpolation, and persisted identity drift/adoption.
#[tokio::test]
async fn malformed_prefixes_and_deployment_identity_mismatches_fail_without_mutation() {
    let mut db = Database::start().await;
    db.prepare("identity").await;
    let settings = db.settings_from(vec![]);
    for prefix in [
        "",
        "9bad",
        "Upper",
        "pg_reserved",
        "bad-name",
        "a'; CREATE ROLE injected;--",
        "a\0b",
        "é",
        &"a".repeat(57),
    ] {
        let tx = db.client.transaction().await.unwrap();
        let error = provision_managed_roles(&tx, Some(prefix), &settings, &db.admin, &db.options)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("prefix"));
        tx.rollback().await.unwrap();
    }
    let tx = db.client.transaction().await.unwrap();
    let error = provision_managed_roles(&tx, Some("other"), &settings, &db.admin, &db.options)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("identity"));
    tx.rollback().await.unwrap();
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute("UPDATE public.blkit_deployment SET worker_role = 'not_owned'")
        .await
        .unwrap();
    let error = provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("identity"));
    tx.rollback().await.unwrap();
    assert_eq!(role_count(&db.client, "identity").await, 3);
    assert_eq!(role_count(&db.client, "other").await, 0);
}

// Catches off-by-one identifier validation at PostgreSQL's 63-byte name boundary.
#[tokio::test]
async fn maximum_length_prefix_creates_distinct_untruncated_names() {
    let mut db = Database::start().await;
    let prefix = "a".repeat(56);
    db.prepare(&prefix).await;
    assert_eq!(role_count(&db.client, &prefix).await, 3);
    assert!(db.login(&format!("{prefix}_worker"), WORKER_PASSWORD).await);
}

// Catches accepting or silently repairing direct elevation and wrong login attributes.
#[tokio::test]
async fn elevated_attributes_are_rejected_not_repaired() {
    let mut db = Database::start().await;
    db.prepare("attrs").await;
    let settings = db.settings_from(vec![]);
    for change in [
        "ALTER ROLE attrs_api SUPERUSER",
        "ALTER ROLE attrs_worker CREATEDB",
        "ALTER ROLE attrs_api CREATEROLE",
        "ALTER ROLE attrs_worker REPLICATION",
        "ALTER ROLE attrs_api BYPASSRLS",
        "ALTER ROLE attrs_worker NOLOGIN",
        "ALTER ROLE attrs_owner LOGIN",
        "ALTER ROLE attrs_api INHERIT",
        "ALTER ROLE attrs_owner CREATEROLE",
    ] {
        let tx = db.client.transaction().await.unwrap();
        tx.batch_execute(change).await.unwrap();
        let error = provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("attributes"));
        tx.rollback().await.unwrap();
    }
}

// Catches SET ROLE escalation even under NOINHERIT and indirect/predefined elevation.
#[tokio::test]
async fn owner_admin_opposite_and_inherited_memberships_are_rejected_not_repaired() {
    let mut db = Database::start().await;
    db.prepare("membership").await;
    db.client
        .batch_execute("CREATE ROLE bridge; GRANT pg_write_all_data TO bridge")
        .await
        .unwrap();
    let settings = db.settings_from(vec![]);
    for grant in [
        "GRANT membership_owner TO membership_api",
        "GRANT postgres TO membership_worker",
        "GRANT membership_api TO membership_worker",
        "GRANT membership_worker TO membership_api",
        "GRANT bridge TO membership_api",
        "GRANT pg_read_all_data TO membership_owner",
        "GRANT bridge TO membership_worker WITH ADMIN OPTION",
    ] {
        let tx = db.client.transaction().await.unwrap();
        tx.batch_execute(grant).await.unwrap();
        let error = provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("membership"));
        assert!(tx.query_one("SELECT EXISTS (SELECT FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles r ON r.oid=m.member WHERE r.rolname IN ('membership_owner', 'membership_api', 'membership_worker'))", &[]).await.unwrap().get::<_, bool>(0), "do not silently revoke elevation");
        tx.rollback().await.unwrap();
    }
}

// Catches using credentials for a different user/endpoint/db, missing or invalid passwords.
#[tokio::test]
async fn new_login_credentials_must_match_managed_identity_and_admin_database() {
    let mut db = Database::start().await;
    db.client.batch_execute(INITIAL).await.unwrap();
    let valid = url(&db.config, "credentials_api", API_PASSWORD);
    let worker = url(&db.config, "credentials_worker", WORKER_PASSWORD);
    let wrong_port = valid.replace(&format!(":{}/", db.config.get_ports()[0]), ":1/");
    let cases = [
        url(&db.config, "someone_else", API_PASSWORD),
        valid.replace("/postgres", "/another_database"),
        format!(
            "{}@different.invalid:{}/postgres",
            valid.split_once('@').unwrap().0,
            db.config.get_ports()[0]
        ),
        wrong_port,
        url(&db.config, "credentials_api", ""),
        url(&db.config, "credentials_api", "nul\0secret"),
        url(&db.config, "credentials_api", "").replace(":@", "@"),
    ];
    for api in cases {
        let settings = db.settings_from(vec![
            ("BLKIT_API_DATABASE_URL", api),
            ("BLKIT_WORKER_DATABASE_URL", worker.clone()),
        ]);
        let tx = db.client.transaction().await.unwrap();
        assert!(
            provision_managed_roles(&tx, Some("credentials"), &settings, &db.admin, &db.options)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        assert_eq!(role_count(&db.client, "credentials").await, 0);
        assert_eq!(
            db.client
                .query_one("SELECT count(*) FROM public.blkit_deployment", &[])
                .await
                .unwrap()
                .get::<_, i64>(0),
            0
        );
    }
    let settings = db.settings("credentials");
    let tx = db.client.transaction().await.unwrap();
    provision_managed_roles(&tx, Some("credentials"), &settings, &db.admin, &db.options)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(db.login("credentials_api", API_PASSWORD).await);
}

// Catches internal commits: later migration/grant/history failures must undo roles too.
#[tokio::test]
async fn caller_failure_rolls_back_roles_schema_and_deployment_metadata() {
    let mut db = Database::start().await;
    let settings = db.settings("atomic");
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    provision_managed_roles(&tx, Some("atomic"), &settings, &db.admin, &db.options)
        .await
        .unwrap();
    assert!(tx.batch_execute("SELECT 1 / 0").await.is_err());
    tx.rollback().await.unwrap();
    assert_eq!(role_count(&db.client, "atomic").await, 0);
    assert!(db.client.query_one("SELECT pg_catalog.to_regclass('public.blkit_deployment') IS NULL AND pg_catalog.to_regclass('public.instances') IS NULL AND pg_catalog.to_regclass('public.blkit_schema_migrations') IS NULL", &[]).await.unwrap().get::<_, bool>(0));
}

// Catches forgetting newly created roles after a provisioning error or retaining raw errors.
#[tokio::test]
async fn provisioning_failure_rolls_back_roles_and_redacts_server_error_sources() {
    let mut db = Database::start().await;
    let settings = db.settings("failure");
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    tx.batch_execute("CREATE FUNCTION public.reject_deployment() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'raw-secret-from-server'; END $$; CREATE TRIGGER reject_deployment BEFORE INSERT ON public.blkit_deployment FOR EACH ROW EXECUTE FUNCTION public.reject_deployment()").await.unwrap();
    let error = provision_managed_roles(&tx, Some("failure"), &settings, &db.admin, &db.options)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("deployment"));
    for rendered in [error.to_string(), format!("{error:?}")] {
        assert!(!rendered.contains("raw-secret-from-server"));
        assert!(!rendered.contains(API_PASSWORD));
        assert!(!rendered.contains("CREATE ROLE"));
    }
    assert!(std::error::Error::source(&error).is_none());
    tx.rollback().await.unwrap();
    assert_eq!(role_count(&db.client, "failure").await, 0);
    assert!(
        db.client
            .query_one(
                "SELECT pg_catalog.to_regclass('public.blkit_deployment') IS NULL",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
}

// Catches requiring a superuser or silently granting authority to an existing owner.
#[tokio::test]
async fn actual_create_and_owner_assumption_capabilities_are_required_without_superuser() {
    let db = Database::start().await;
    db.client.batch_execute(INITIAL).await.unwrap();
    db.client.batch_execute("CREATE ROLE \"ad\"\"min\" LOGIN PASSWORD 'limited'; GRANT SELECT, INSERT ON public.blkit_deployment TO \"ad\"\"min\"").await.unwrap();
    let settings = db.settings("capability");
    let admin_config = db.settings_from(vec![(
        "BLKIT_ADMIN_DATABASE_URL",
        url(&db.config, "ad\"min", "limited"),
    )]);
    let admin = admin_config
        .resolve_role(DatabaseRole::Admin, &db.options)
        .unwrap();
    let (mut client, connection) = admin.connection().connect().await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let tx = client.transaction().await.unwrap();
    let error = provision_managed_roles(&tx, Some("capability"), &settings, &admin, &db.options)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("authority"));
    tx.rollback().await.unwrap();
    assert_eq!(role_count(&db.client, "capability").await, 0);
    db.client
        .batch_execute("ALTER ROLE \"ad\"\"min\" CREATEROLE")
        .await
        .unwrap();
    let tx = client.transaction().await.unwrap();
    provision_managed_roles(&tx, Some("capability"), &settings, &admin, &db.options)
        .await
        .unwrap();
    assert_eq!(
        tx.query_one("SELECT current_user", &[])
            .await
            .unwrap()
            .get::<_, &str>(0),
        "ad\"min",
        "restore caller identity"
    );
    tx.commit().await.unwrap();
    db.client
        .batch_execute("REVOKE capability_owner FROM \"ad\"\"min\" CASCADE")
        .await
        .unwrap();
    let empty = db.settings_from(vec![]);
    let tx = client.transaction().await.unwrap();
    let error = provision_managed_roles(&tx, None, &empty, &admin, &db.options)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("authority"));
    tx.rollback().await.unwrap();
    assert!(db.login("capability_api", API_PASSWORD).await);
}
