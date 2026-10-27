use super::*;
use blkit_cli::postgres_admin::{ManagedRoles, reconcile_managed_privileges};
use tokio_postgres::error::SqlState;

async fn prepare(db: &mut Database) {
    let settings = db.settings("grants");
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    let roles = provision_managed_roles(&tx, Some("grants"), &settings, &db.admin, &db.options)
        .await
        .unwrap();
    reconcile_managed_privileges(&tx, &roles).await.unwrap();
    tx.execute("INSERT INTO public.blkit_schema_migrations (version, identity, checksum) VALUES (1, 'test', 'test')", &[]).await.unwrap();
    tx.commit().await.unwrap();
}

async fn login(db: &Database, user: &str, password: &str) -> Client {
    let mut config = db.config.clone();
    config.user(user).password(password);
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let row = client
        .query_one(
            "SELECT current_user, session_user, current_setting('search_path')",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(row.get::<_, &str>(0), user);
    assert_eq!(row.get::<_, &str>(1), user);

    client
}

async fn denied(client: &Client, sql: &str) {
    let error = client.batch_execute(sql).await.expect_err(sql);
    assert_eq!(
        error.code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE),
        "{sql}: unexpected error category"
    );
}

// Missing grants, especially typed input and invoker-trigger interest SELECT, break this.
#[tokio::test]
async fn real_logins_can_perform_only_their_operational_sql_primitives() {
    let mut db = Database::start().await;
    prepare(&mut db).await;
    assert!(!db.login("grants_api", "wrong").await);
    assert!(!db.login("grants_worker", "wrong").await);
    let api = login(&db, "grants_api", API_PASSWORD).await;
    let worker = login(&db, "grants_worker", WORKER_PASSWORD).await;
    worker.batch_execute("INSERT INTO workers (id, heartbeat_at) VALUES ('worker', 1);
        INSERT INTO worker_capabilities (worker_id, namespace, version, process) VALUES ('worker', 'ns', 'v1', 'p');
        INSERT INTO process_policies (namespace, version, process, retry_policy, deadline_origin, deadline_duration_ms) VALUES ('ns', 'v1', 'p', NULL, 'queued', 1000) ON CONFLICT DO NOTHING;
        SELECT * FROM process_policies;
        UPDATE workers SET heartbeat_at=2, draining=FALSE WHERE id='worker';").await.unwrap();
    api.batch_execute("INSERT INTO instances (id, namespace, version, process, input, status, created_at, updated_at, retry_policy, queued_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms)
        VALUES ('one', 'ns', 'v1', 'p', '{}', 'pending', 1, 1, NULL, 1000, 'queued', 1000, 2000);
        INSERT INTO instance_attachments (instance_id, request_id, ordinal, object_key, filename, media_type, byte_length) VALUES ('one', NULL, 0, 'key', 'file', NULL, 1);
        SELECT id FROM instances WHERE id='one' FOR SHARE;
        INSERT INTO instance_interests (instance_id, channel, expires_at) VALUES ('one', 'events', clock_timestamp() + interval '10 seconds')
            ON CONFLICT (instance_id, channel) DO UPDATE SET expires_at=EXCLUDED.expires_at;
        SELECT expires_at FROM instance_interests WHERE instance_id='one' FOR UPDATE;
        LISTEN events;").await.unwrap();
    worker.batch_execute("SELECT draining FROM workers WHERE id='worker' FOR SHARE;
        SELECT id FROM instances WHERE id='one' FOR UPDATE SKIP LOCKED;
        UPDATE instances SET status='running', owner_id='worker', lease_until=3000, generation=generation+1, attempt=attempt+1,
            input_resume=FALSE, next_eligible_at=NULL, wake_at_ms=NULL, first_claim_at_ms=1000, deadline_at_ms=2000, updated_at=2 WHERE id='one';
        UPDATE instances SET input='{\"typed\":true}', checkpoint='checkpoint', updated_at=3 WHERE id='one';
        INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ('one', 'reply', 'prompt', '{}') ON CONFLICT DO NOTHING;
        SELECT * FROM instance_attachments;
        SELECT channel FROM instance_interests;").await.unwrap();
    // Both trigger paths execute as their caller, with no direct function EXECUTE grant.
    api.batch_execute(
        "UPDATE input_requests SET status='answered', response='reply' WHERE instance_id='one';
        UPDATE instances SET status='pending', input_resume=TRUE, updated_at=4 WHERE id='one';",
    )
    .await
    .unwrap();
    worker.batch_execute("UPDATE input_requests SET status='consumed' WHERE instance_id='one';
        UPDATE instances SET status='completed', result='outcome', terminal_name=NULL, error=NULL, owner_id=NULL, lease_until=NULL, updated_at=5 WHERE id='one';").await.unwrap();
    let row = api
        .query_one(
            "SELECT input, checkpoint, result FROM instances WHERE id='one'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(row.get::<_, &str>(0), "{\"typed\":true}");
    assert_eq!(row.get::<_, &str>(1), "checkpoint");
    assert_eq!(row.get::<_, &str>(2), "outcome");
    for client in [&api, &worker] {
        client.batch_execute("SELECT * FROM instances; SELECT * FROM input_requests; SELECT * FROM instance_attachments;
            SELECT * FROM instance_interests; SELECT * FROM workers; SELECT * FROM worker_capabilities; SELECT * FROM process_policies;
            SELECT * FROM blkit_deployment; SELECT * FROM blkit_schema_migrations;
            UPDATE instances SET status='failed', error='expired', first_failure_at=1000, next_eligible_at=NULL,
                owner_id=NULL, lease_until=NULL, updated_at=6 WHERE id='one';
            UPDATE instances SET status='business-error', terminal_name='timeout', wake_at_ms=NULL, owner_id=NULL, lease_until=NULL, updated_at=7 WHERE id='one';
            UPDATE instances SET status='pending' WHERE id='one';
            UPDATE input_requests SET status='retired' WHERE instance_id='one';").await.unwrap();
    }
    api.batch_execute("UPDATE instances SET status='cancelled', owner_id=NULL, lease_until=NULL, updated_at=8 WHERE id='one';
        UPDATE instance_interests SET expires_at=clock_timestamp() WHERE instance_id='one';
        DELETE FROM instance_interests WHERE instance_id='one';").await.unwrap();
    worker.batch_execute("UPDATE workers SET draining=TRUE WHERE id='worker'; DELETE FROM workers WHERE id='worker'").await.unwrap();
    assert_eq!(
        api.query_one("SELECT count(*) FROM worker_capabilities", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        0,
        "worker deletion cascades without capability DELETE"
    );
    for client in [&api, &worker] {
        assert_eq!(
            client
                .query_one("SHOW search_path", &[])
                .await
                .unwrap()
                .get::<_, &str>(0),
            "pg_catalog, public, pg_temp"
        );
    }
    assert!(!db.client.query_one("SELECT prosecdef FROM pg_catalog.pg_proc WHERE oid='public.notify_instance_interests()'::pg_catalog.regprocedure", &[]).await.unwrap().get::<_, bool>(0));
}

// Catch broad table/column grants and accidental grant options, including columns not used today.
#[tokio::test]
async fn exact_column_matrix_and_forbidden_sql_are_enforced_by_postgres() {
    let mut db = Database::start().await;
    prepare(&mut db).await;
    let api = login(&db, "grants_api", API_PASSWORD).await;
    let worker = login(&db, "grants_worker", WORKER_PASSWORD).await;
    let admission = "id namespace version process input status created_at updated_at retry_policy queued_at_ms deadline_origin deadline_duration_ms deadline_at_ms";
    let api_updates = "status input_resume updated_at owner_id lease_until terminal_name wake_at_ms error first_failure_at next_eligible_at";
    let worker_updates = "status owner_id lease_until generation attempt input_resume next_eligible_at wake_at_ms first_claim_at_ms deadline_at_ms updated_at input checkpoint error first_failure_at result terminal_name";
    for (client, user, policies) in [
        (
            &api,
            "grants_api",
            vec![
                ("instances", admission, api_updates, false),
                ("input_requests", "", "status response", false),
                (
                    "instance_attachments",
                    "instance_id request_id ordinal object_key filename media_type byte_length",
                    "",
                    false,
                ),
                (
                    "instance_interests",
                    "instance_id channel expires_at",
                    "expires_at",
                    true,
                ),
                ("workers", "", "", false),
                ("worker_capabilities", "", "", false),
                ("process_policies", "", "", false),
                ("blkit_deployment", "", "", false),
                ("blkit_schema_migrations", "", "", false),
            ],
        ),
        (
            &worker,
            "grants_worker",
            vec![
                ("instances", "", worker_updates, false),
                (
                    "input_requests",
                    "instance_id request_id prompt schema",
                    "status",
                    false,
                ),
                ("instance_attachments", "", "", false),
                ("instance_interests", "", "", false),
                ("workers", "id heartbeat_at", "heartbeat_at draining", true),
                (
                    "worker_capabilities",
                    "worker_id namespace version process",
                    "",
                    false,
                ),
                (
                    "process_policies",
                    "namespace version process retry_policy deadline_origin deadline_duration_ms",
                    "",
                    false,
                ),
                ("blkit_deployment", "", "", false),
                ("blkit_schema_migrations", "", "", false),
            ],
        ),
    ] {
        for (table, inserts, updates, delete) in policies {
            let qualified = format!("public.{table}");
            for privilege in [
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
            ] {
                let allowed = privilege == "SELECT" || (privilege == "DELETE" && delete);
                let actual: bool = db
                    .client
                    .query_one(
                        "SELECT pg_catalog.has_table_privilege($1, $2, $3)",
                        &[&user, &qualified, &privilege],
                    )
                    .await
                    .unwrap()
                    .get(0);
                assert_eq!(actual, allowed, "{user}: {table} {privilege}");
                let grantable: bool = db
                    .client
                    .query_one(
                        "SELECT pg_catalog.has_table_privilege($1, $2, $3)",
                        &[&user, &qualified, &format!("{privilege} WITH GRANT OPTION")],
                    )
                    .await
                    .unwrap()
                    .get(0);
                assert!(!grantable, "{user}: {table} grant option");
            }
            let columns = db.client.query("SELECT attname::pg_catalog.text FROM pg_catalog.pg_attribute WHERE attrelid=$1::pg_catalog.text::pg_catalog.regclass AND attnum>0 AND NOT attisdropped", &[&qualified]).await.unwrap();
            for row in columns {
                let column: &str = row.get(0);
                for (privilege, allowed_columns) in
                    [("INSERT", inserts), ("UPDATE", updates), ("REFERENCES", "")]
                {
                    let allowed = allowed_columns.split_whitespace().any(|c| c == column);
                    let actual: bool = db
                        .client
                        .query_one(
                            "SELECT pg_catalog.has_column_privilege($1, $2, $3, $4)",
                            &[&user, &qualified, &column, &privilege],
                        )
                        .await
                        .unwrap()
                        .get(0);
                    assert_eq!(actual, allowed, "{user}: {table}.{column} {privilege}");
                    let grantable: bool = db
                        .client
                        .query_one(
                            "SELECT pg_catalog.has_column_privilege($1, $2, $3, $4)",
                            &[
                                &user,
                                &qualified,
                                &column,
                                &format!("{privilege} WITH GRANT OPTION"),
                            ],
                        )
                        .await
                        .unwrap()
                        .get(0);
                    assert!(!grantable, "{user}: {table}.{column} grant option");
                    if !allowed && privilege == "UPDATE" {
                        denied(client, &format!("UPDATE {qualified} SET {column}={column}")).await;
                    }
                }
            }
            if !delete {
                denied(client, &format!("DELETE FROM {qualified}")).await;
            }
            if inserts.is_empty() {
                denied(client, &format!("INSERT INTO {qualified} DEFAULT VALUES")).await;
            }
        }
        for sql in [
            "CREATE TABLE public.forbidden (id int)",
            "CREATE TEMP TABLE forbidden (id int)",
            "CREATE SCHEMA forbidden",
            "CREATE DATABASE forbidden",
            "CREATE ROLE forbidden",
            "ALTER ROLE grants_api SUPERUSER",
            "ALTER TABLE public.instances ADD COLUMN forbidden int",
            "DROP TABLE public.instances",
            "TRUNCATE public.instances",
            "SET ROLE grants_owner",
            "SET ROLE postgres",
            "GRANT grants_owner TO grants_api",
            "GRANT grants_api TO grants_worker",
            "CREATE FUNCTION public.forbidden() RETURNS int LANGUAGE sql AS 'SELECT 1'",
            "ALTER FUNCTION public.notify_instance_interests() SECURITY DEFINER",
        ] {
            denied(client, sql).await;
        }
    }
    denied(&worker, "SELECT * FROM process_policies FOR UPDATE").await;
    denied(&worker, "INSERT INTO input_requests (instance_id, request_id, prompt, schema, response) VALUES ('one', 'r', 'p', '{}', 'forged')").await;
    denied(&worker, "INSERT INTO input_requests (instance_id, request_id, prompt, schema, status) VALUES ('one', 'r', 'p', '{}', 'answered')").await;
    denied(&api, "INSERT INTO instances (id, namespace, version, process, input, status, created_at, updated_at, checkpoint) VALUES ('one', 'n', 'v', 'p', '{}', 'pending', 1, 1, 'forged')").await;
    // GRANT without grant option may emit a warning rather than an SQL error. Check its effect.
    api.batch_execute("GRANT UPDATE (checkpoint) ON public.instances TO grants_worker")
        .await
        .unwrap();
    worker
        .batch_execute("GRANT INSERT ON public.instances TO grants_api")
        .await
        .unwrap();
    assert!(
        !db.client
            .query_one(
                "SELECT pg_catalog.has_table_privilege('grants_api', 'public.instances', 'INSERT')",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
}

async fn provision_again(db: &mut Database) -> ManagedRoles {
    let settings = db.settings_from(vec![]);
    let tx = db.client.transaction().await.unwrap();
    let roles = provision_managed_roles(&tx, None, &settings, &db.admin, &db.options)
        .await
        .unwrap();
    reconcile_managed_privileges(&tx, &roles).await.unwrap();
    tx.commit().await.unwrap();
    roles
}

// Catch table-only revocation leaving old column ACLs, per-schema-only function default
// revocation leaving global PUBLIC EXECUTE, and sweeping unrelated owners' objects.
#[tokio::test]
async fn rerun_clears_stale_acl_and_defaults_without_touching_unrelated_owners() {
    let mut db = Database::start().await;
    prepare(&mut db).await;
    db.client.batch_execute("CREATE ROLE unrelated;
        CREATE SCHEMA owner_other AUTHORIZATION grants_owner;
        GRANT USAGE ON SCHEMA owner_other TO grants_api, grants_worker;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner IN SCHEMA owner_other GRANT EXECUTE ON FUNCTIONS TO grants_worker, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner IN SCHEMA owner_other GRANT ALL ON TABLES TO grants_api;
        CREATE TABLE public.unrelated (value text); INSERT INTO public.unrelated VALUES ('keep');
        ALTER TABLE public.unrelated OWNER TO unrelated;
        GRANT SELECT ON public.unrelated TO grants_api;
        ALTER DEFAULT PRIVILEGES FOR ROLE unrelated GRANT SELECT ON TABLES TO grants_worker;
        ALTER DEFAULT PRIVILEGES FOR ROLE unrelated IN SCHEMA public GRANT EXECUTE ON FUNCTIONS TO grants_api;
        GRANT UPDATE (checkpoint, status), INSERT (result) ON public.instances TO grants_api WITH GRANT OPTION;
        GRANT INSERT (response), UPDATE (response) ON public.input_requests TO grants_worker WITH GRANT OPTION;
        GRANT UPDATE (input) ON public.instances TO PUBLIC;
        GRANT ALL ON public.workers TO grants_api;
        GRANT ALL ON FUNCTION public.notify_instance_interests() TO PUBLIC, grants_worker;
        GRANT CREATE, TEMPORARY ON DATABASE postgres TO grants_api, grants_worker;
        GRANT CREATE ON SCHEMA public TO PUBLIC, grants_api, grants_worker;
        ALTER ROLE grants_api IN DATABASE postgres SET search_path = evil;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner GRANT ALL ON TABLES TO grants_api, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner IN SCHEMA public GRANT ALL ON TABLES TO grants_worker, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner GRANT ALL ON SEQUENCES TO grants_api, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner IN SCHEMA public GRANT ALL ON SEQUENCES TO grants_worker, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner GRANT EXECUTE ON FUNCTIONS TO grants_api, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner IN SCHEMA public GRANT EXECUTE ON FUNCTIONS TO grants_worker, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner GRANT USAGE ON TYPES TO grants_api, PUBLIC;
        ALTER DEFAULT PRIVILEGES FOR ROLE grants_owner IN SCHEMA public GRANT USAGE ON TYPES TO grants_worker, PUBLIC;").await.unwrap();
    let snapshot_sql = "SELECT pg_catalog.pg_get_userbyid(c.relowner), c.relacl::pg_catalog.text, d.defaclobjtype::pg_catalog.text, d.defaclnamespace::pg_catalog.text, d.defaclacl::pg_catalog.text FROM pg_catalog.pg_class c CROSS JOIN pg_catalog.pg_default_acl d JOIN pg_catalog.pg_roles r ON r.oid=d.defaclrole WHERE c.oid='public.unrelated'::pg_catalog.regclass AND r.rolname='unrelated' ORDER BY d.defaclobjtype, d.defaclnamespace";
    async fn snapshot(client: &Client, sql: &str) -> Vec<Vec<String>> {
        client
            .query(sql, &[])
            .await
            .unwrap()
            .into_iter()
            .map(|r| (0..5).map(|i| r.get(i)).collect())
            .collect()
    }
    let before = snapshot(&db.client, snapshot_sql).await;
    provision_again(&mut db).await;
    assert_eq!(snapshot(&db.client, snapshot_sql).await, before);
    assert!(db.login("grants_api", API_PASSWORD).await);
    assert!(db.login("grants_worker", WORKER_PASSWORD).await);
    let api = login(&db, "grants_api", API_PASSWORD).await;
    let worker = login(&db, "grants_worker", WORKER_PASSWORD).await;
    assert!(!db.client.query_one("SELECT pg_catalog.has_column_privilege('grants_api', 'public.instances', 'status', 'UPDATE WITH GRANT OPTION')", &[]).await.unwrap().get::<_, bool>(0));
    denied(&api, "UPDATE instances SET checkpoint='forged'").await;
    denied(&api, "UPDATE instances SET input='forged'").await;
    denied(&api, "INSERT INTO instances (result) VALUES ('forged')").await;
    denied(&worker, "UPDATE input_requests SET response='forged'").await;
    denied(
        &worker,
        "INSERT INTO input_requests (response) VALUES ('forged')",
    )
    .await;
    denied(
        &api,
        "INSERT INTO workers (id, heartbeat_at) VALUES ('forged', 1)",
    )
    .await;
    // New managed-owner objects get no implicit access, including a function in a
    // different schema (global function defaults must be denied, not just public).
    db.client
        .batch_execute(
            "CREATE SCHEMA future AUTHORIZATION grants_owner;
        GRANT USAGE ON SCHEMA future TO grants_api, grants_worker;
        SET ROLE grants_owner;
        CREATE TABLE public.future_table (value int);
        CREATE SEQUENCE public.future_sequence;
        CREATE FUNCTION public.future_function() RETURNS int LANGUAGE sql AS 'SELECT 1';
        CREATE FUNCTION future.global_function() RETURNS int LANGUAGE sql AS 'SELECT 1';
        CREATE FUNCTION owner_other.scoped_function() RETURNS int LANGUAGE sql AS 'SELECT 1';
        CREATE TABLE owner_other.scoped_table (value int);
        CREATE TYPE public.future_type AS ENUM ('value');
        RESET ROLE;",
        )
        .await
        .unwrap();
    for client in [&api, &worker] {
        for sql in [
            "SELECT * FROM public.future_table",
            "INSERT INTO public.future_table VALUES (1)",
            "SELECT nextval('public.future_sequence')",
            "SELECT public.future_function()",
            "SELECT future.global_function()",
            "SELECT owner_other.scoped_function()",
            "SELECT * FROM owner_other.scoped_table",
            "CREATE TEMP TABLE forbidden (id int)",
            "CREATE TABLE public.forbidden (id int)",
        ] {
            denied(client, sql).await;
        }
        assert!(!client.query_one("SELECT pg_catalog.has_type_privilege(current_user, 'public.future_type', 'USAGE')", &[]).await.unwrap().get::<_, bool>(0));
        assert_eq!(
            client
                .query_one("SHOW search_path", &[])
                .await
                .unwrap()
                .get::<_, &str>(0),
            "pg_catalog, public, pg_temp"
        );
    }
    assert_eq!(
        api.query_one("SELECT value FROM public.unrelated", &[])
            .await
            .unwrap()
            .get::<_, &str>(0),
        "keep"
    );
    // Known tables/indexes and function, not public or unrelated objects, change owner.
    let rows = db.client.query("SELECT relname, pg_catalog.pg_get_userbyid(relowner) FROM pg_catalog.pg_class WHERE relnamespace='public'::pg_catalog.regnamespace AND relname NOT IN ('unrelated', 'future_table', 'future_sequence') AND relkind IN ('r', 'i')", &[]).await.unwrap();
    assert!(rows.len() > 9);
    for row in rows {
        assert_eq!(
            row.get::<_, &str>(1),
            "grants_owner",
            "{}",
            row.get::<_, &str>(0)
        );
    }
    assert_eq!(db.client.query_one("SELECT pg_catalog.pg_get_userbyid(proowner) FROM pg_catalog.pg_proc WHERE oid='public.notify_instance_interests()'::pg_catalog.regprocedure", &[]).await.unwrap().get::<_, &str>(0), "grants_owner");
    let public_acl_count: i64 = db.client.query_one("SELECT count(*) FROM pg_catalog.pg_class c CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) a WHERE c.relnamespace='public'::pg_catalog.regnamespace AND pg_catalog.pg_get_userbyid(c.relowner)='grants_owner' AND a.grantee=0", &[]).await.unwrap().get(0);
    assert_eq!(public_acl_count, 0);
}

// Catch an internal commit or a separate grant session, and preserve caller authority.
#[tokio::test]
async fn caller_transaction_rolls_back_roles_schema_grants_defaults_and_history_together() {
    let mut db = Database::start().await;
    let database_acl: Option<String> = db.client.query_one("SELECT datacl::pg_catalog.text FROM pg_catalog.pg_database WHERE datname=current_database()", &[]).await.unwrap().get(0);
    let settings = db.settings("atomicgrants");
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute(INITIAL).await.unwrap();
    let roles =
        provision_managed_roles(&tx, Some("atomicgrants"), &settings, &db.admin, &db.options)
            .await
            .unwrap();
    reconcile_managed_privileges(&tx, &roles).await.unwrap();
    tx.batch_execute("INSERT INTO public.blkit_schema_migrations (version, identity, checksum) VALUES (1, 'test', 'test')").await.unwrap();
    assert!(tx.query_one("SELECT pg_catalog.has_table_privilege('atomicgrants_api', 'public.instances', 'SELECT') AND NOT pg_catalog.has_database_privilege('atomicgrants_api', current_database(), 'TEMP')", &[]).await.unwrap().get::<_, bool>(0));
    assert_eq!(tx.query_one("SELECT pg_catalog.pg_get_userbyid(relowner) FROM pg_catalog.pg_class WHERE oid='public.instances'::pg_catalog.regclass", &[]).await.unwrap().get::<_, &str>(0), "atomicgrants_owner");
    assert!(
        tx.query_one("SELECT count(*)>0 FROM pg_catalog.pg_default_acl", &[])
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    assert_eq!(
        tx.query_one("SELECT current_user", &[])
            .await
            .unwrap()
            .get::<_, &str>(0),
        "postgres"
    );
    assert!(tx.batch_execute("SELECT 1/0").await.is_err());
    tx.rollback().await.unwrap();
    assert_eq!(role_count(&db.client, "atomicgrants").await, 0);
    assert!(db.client.query_one("SELECT pg_catalog.to_regclass('public.instances') IS NULL AND pg_catalog.to_regclass('public.blkit_schema_migrations') IS NULL", &[]).await.unwrap().get::<_, bool>(0));
    assert_eq!(db.client.query_one("SELECT datacl::pg_catalog.text FROM pg_catalog.pg_database WHERE datname=current_database()", &[]).await.unwrap().get::<_, Option<String>>(0), database_acl);
    assert_eq!(
        db.client
            .query_one("SELECT count(*) FROM pg_catalog.pg_default_acl", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        0
    );
}

// Catch grant-only reruns trusting a stale ManagedRoles value after DBA elevation.
#[tokio::test]
async fn privilege_reconciliation_rejects_elevated_roles_and_redacts_sql_errors() {
    let mut db = Database::start().await;
    prepare(&mut db).await;
    let roles = provision_again(&mut db).await;
    for sql in [
        "ALTER ROLE grants_api SUPERUSER",
        "GRANT grants_owner TO grants_worker",
        "GRANT pg_read_all_data TO grants_owner",
    ] {
        let tx = db.client.transaction().await.unwrap();
        tx.batch_execute(sql).await.unwrap();
        let error = reconcile_managed_privileges(&tx, &roles).await.unwrap_err();
        assert!(
            error.to_string().contains("attributes") || error.to_string().contains("membership")
        );
        assert!(std::error::Error::source(&error).is_none());
        tx.rollback().await.unwrap();
    }
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute("DROP TABLE public.instance_attachments")
        .await
        .unwrap();
    let error = reconcile_managed_privileges(&tx, &roles).await.unwrap_err();
    assert!(error.to_string().contains("authority") || error.to_string().contains("managed"));
    assert!(std::error::Error::source(&error).is_none());
    assert!(!format!("{error:?}").contains("ALTER TABLE"));
    tx.rollback().await.unwrap();
}

// Catch a blanket SUPERUSER requirement or ownership transfer without owner schema CREATE.
#[tokio::test]
async fn database_owner_with_createrole_can_apply_policy_without_superuser() {
    let mut db = Database::start().await;
    db.client
        .batch_execute(
            "CREATE ROLE deployer LOGIN CREATEROLE PASSWORD 'limited';
        ALTER DATABASE postgres OWNER TO deployer; ALTER SCHEMA public OWNER TO deployer;",
        )
        .await
        .unwrap();
    let mut config = db.config.clone();
    config.user("deployer").password("limited");
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let settings = db.settings_from(vec![(
        "BLKIT_ADMIN_DATABASE_URL",
        url(&db.config, "deployer", "limited"),
    )]);
    db.admin = settings
        .resolve_role(DatabaseRole::Admin, &db.options)
        .unwrap();
    db.client = client;
    prepare(&mut db).await;
    // All objects were transferred; non-superuser reruns must still have actual authority.
    provision_again(&mut db).await;
    let api = login(&db, "grants_api", API_PASSWORD).await;
    assert_eq!(
        api.query_one("SELECT count(*) FROM public.blkit_deployment", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
}

// Effective DDL/ownership outside the known-object transfer must fail closed, never
// seize unrelated owners or rewrite their ACLs to make preparation appear safe.
#[tokio::test]
async fn unexpected_runtime_ownership_or_unrelated_public_create_is_rejected_not_repaired() {
    let mut db = Database::start().await;
    prepare(&mut db).await;
    let roles = provision_again(&mut db).await;
    db.client.batch_execute("CREATE ROLE unrelated;
        CREATE SCHEMA unrelated AUTHORIZATION unrelated;
        CREATE TABLE public.unrelated (value text); ALTER TABLE public.unrelated OWNER TO unrelated;
        CREATE FUNCTION public.unrelated_function() RETURNS int LANGUAGE sql AS 'SELECT 1';
        ALTER FUNCTION public.unrelated_function() OWNER TO unrelated;
        CREATE TYPE public.unrelated_type AS ENUM ('keep'); ALTER TYPE public.unrelated_type OWNER TO unrelated;
    ").await.unwrap();
    for sql in [
        "ALTER DATABASE postgres OWNER TO grants_api",
        "ALTER SCHEMA public OWNER TO grants_worker",
        "GRANT CREATE ON SCHEMA unrelated TO PUBLIC",
        "GRANT CREATE ON SCHEMA unrelated TO grants_worker",
        "ALTER SCHEMA unrelated OWNER TO grants_api",
        "ALTER TABLE public.unrelated OWNER TO grants_api",
        "ALTER FUNCTION public.unrelated_function() OWNER TO grants_worker",
        "ALTER TYPE public.unrelated_type OWNER TO grants_api",
    ] {
        let tx = db.client.transaction().await.unwrap();
        tx.batch_execute(sql).await.unwrap();
        let schema_acl: Option<String> = tx.query_one("SELECT nspacl::pg_catalog.text FROM pg_catalog.pg_namespace WHERE nspname='unrelated'", &[]).await.unwrap().get(0);
        let before: String = tx.query_one("SELECT pg_catalog.md5(pg_catalog.string_agg(s::pg_catalog.text, '' ORDER BY s::pg_catalog.text)) FROM (SELECT dbid, classid, objid, objsubid, refobjid, deptype FROM pg_catalog.pg_shdepend WHERE deptype='o') s", &[]).await.unwrap().get(0);
        let error = reconcile_managed_privileges(&tx, &roles)
            .await
            .expect_err("unexpected runtime DDL authority must fail preparation");
        assert!(error.to_string().contains("DBA review"));
        assert!(std::error::Error::source(&error).is_none());
        // This validation error leaves a queryable transaction: unrelated elevation
        // must still exist, rather than being silently revoked or re-owned.
        let after: String = tx.query_one("SELECT pg_catalog.md5(pg_catalog.string_agg(s::pg_catalog.text, '' ORDER BY s::pg_catalog.text)) FROM (SELECT dbid, classid, objid, objsubid, refobjid, deptype FROM pg_catalog.pg_shdepend WHERE deptype='o') s", &[]).await.unwrap().get(0);
        assert_eq!(before, after, "no ownership dependency rewrite");
        assert_eq!(tx.query_one("SELECT nspacl::pg_catalog.text FROM pg_catalog.pg_namespace WHERE nspname='unrelated'", &[]).await.unwrap().get::<_, Option<String>>(0), schema_acl, "unrelated schema ACL unchanged");
        tx.rollback().await.unwrap();
    }
    assert_eq!(db.client.query_one("SELECT pg_catalog.pg_get_userbyid(relowner) FROM pg_catalog.pg_class WHERE oid='public.unrelated'::pg_catalog.regclass", &[]).await.unwrap().get::<_, &str>(0), "unrelated");
    assert!(
        !db.client
            .query_one(
                "SELECT pg_catalog.has_schema_privilege('grants_api', 'unrelated', 'CREATE')",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
}

// ACLs granted by another grantor must not survive as hidden extra runtime rights.
#[tokio::test]
async fn reconciliation_rejects_surviving_other_grantor_rights_and_grant_options() {
    let mut db = Database::start().await;
    prepare(&mut db).await;
    let roles = provision_again(&mut db).await;
    db.client
        .batch_execute(
            "CREATE ROLE grantor; CREATE ROLE observer;
        GRANT USAGE ON SCHEMA public TO grantor;
        GRANT SELECT, UPDATE (checkpoint, status) ON public.instances TO grantor WITH GRANT OPTION;
        SET ROLE grantor;
        GRANT UPDATE (checkpoint, status) ON public.instances TO observer;
        RESET ROLE;",
        )
        .await
        .unwrap();
    for sql in [
        "GRANT UPDATE (checkpoint) ON public.instances TO grants_api",
        "GRANT UPDATE (status) ON public.instances TO grants_api WITH GRANT OPTION",
        "GRANT SELECT ON public.instances TO PUBLIC",
    ] {
        let tx = db.client.transaction().await.unwrap();
        tx.batch_execute(&format!(
            "SET LOCAL ROLE grantor; {sql}; SET LOCAL ROLE postgres"
        ))
        .await
        .unwrap();
        let error = reconcile_managed_privileges(&tx, &roles)
            .await
            .expect_err("must not claim success while forbidden rights or grant options survive");
        assert!(error.to_string().contains("DBA review"));
        assert!(std::error::Error::source(&error).is_none());
        for column in ["checkpoint", "status"] {
            assert!(tx.query_one("SELECT pg_catalog.has_column_privilege('grantor', 'public.instances', $1, 'UPDATE WITH GRANT OPTION') AND pg_catalog.has_column_privilege('observer', 'public.instances', $1, 'UPDATE')", &[&column]).await.unwrap().get::<_, bool>(0), "unrelated source and other grantee must be unchanged");
        }
        tx.rollback().await.unwrap();
    }
    // A second grantor may duplicate an already allowed right without elevation.
    let tx = db.client.transaction().await.unwrap();
    tx.batch_execute("SET LOCAL ROLE grantor; GRANT UPDATE (status) ON public.instances TO grants_api; SET LOCAL ROLE postgres").await.unwrap();
    reconcile_managed_privileges(&tx, &roles).await.unwrap();
    tx.rollback().await.unwrap();
    for column in ["checkpoint", "status"] {
        assert!(db.client.query_one("SELECT pg_catalog.has_column_privilege('grantor', 'public.instances', $1, 'UPDATE WITH GRANT OPTION') AND pg_catalog.has_column_privilege('observer', 'public.instances', $1, 'UPDATE')", &[&column]).await.unwrap().get::<_, bool>(0));
    }
}
