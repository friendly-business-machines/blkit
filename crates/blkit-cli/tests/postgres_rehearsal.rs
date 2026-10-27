//! Actual paired legacy/current backup restores, not a reload of the historical SQL.
use blkit_core::postgres_config::DatabaseRole;
use serde_json::{Value, json};
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::Duration,
};
use testcontainers_modules::testcontainers::core::ExecCommand;
use tokio::time::{sleep, timeout};
use tokio_postgres::Client;

#[path = "../../../tools/test-support/generated_remote.rs"]
mod generated_remote;
#[path = "support/managed_postgres.rs"]
mod managed_postgres;
#[path = "../../../tools/test-support/managed_runtime.rs"]
#[allow(dead_code)]
mod managed_runtime;
#[path = "../../../tools/test-support/remote_attachment_reader.rs"]
mod remote_attachment_reader;
#[path = "../../../tools/test-support/seaweed.rs"]
mod seaweed;
use generated_remote::Generated;
use managed_postgres::Database;

const TABLES: [&str; 7] = [
    "instances",
    "input_requests",
    "instance_attachments",
    "instance_interests",
    "workers",
    "worker_capabilities",
    "process_policies",
];

fn private_directory(path: &Path) {
    fs::create_dir(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

fn private_write(path: &Path, bytes: &[u8]) {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap().write_all(bytes).unwrap();
}

fn redacted(text: &str) {
    for secret in [
        "postgres://",
        "postgresql://",
        "startup-test",
        "postgres:postgres",
        "SCRAM-SHA-256$",
    ] {
        assert!(!text.contains(secret), "credential in child diagnostic");
    }
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct RuntimeFixtures {
    worker: Generated,
    _api: Generated,
    objects: PathBuf,
    reader: PathBuf,
    api: PathBuf,
    evidence: PathBuf,
}
impl RuntimeFixtures {
    fn build() -> Self {
        let worker = Generated::new("worker-only");
        let api = Generated::new("api-only");
        let evidence =
            std::env::temp_dir().join(format!("blkit-rehearsal-{}-evidence", std::process::id()));
        private_directory(&evidence);
        private_directory(&worker.root.join("backup"));
        fs::write(
            worker.root.join(".blkit/src/bin/legacy-objects.rs"),
            include_str!("support/legacy_objects.rs"),
        )
        .unwrap();
        fs::write(
            worker.root.join(".blkit/src/bin/legacy-reader.rs"),
            remote_attachment_reader::SOURCE,
        )
        .unwrap();
        let binaries = worker.build(false);
        let api_binaries = api.build(false);
        println!(
            "owned generated roots: {}, {}; evidence: {}",
            worker.root.display(),
            api.root.display(),
            evidence.display()
        );
        Self {
            objects: binaries.join("legacy-objects"),
            reader: binaries.join("legacy-reader"),
            api: api_binaries.join(format!("{}-api", api.name)),
            worker,
            _api: api,
            evidence,
        }
    }
    fn root(&self) -> PathBuf {
        self.worker.root.join("backup")
    }
    fn cloud_command(&self, binary: &Path, endpoint: &str) -> Command {
        let mut command = Command::new(binary);
        command.env_clear();
        seaweed::configure(&mut command, endpoint);
        command.env("TOKIO_WORKER_THREADS", "2");
        command
    }
    fn objects(&self, endpoint: &str, mode: &str, manifest: &Path, backup: &Path) {
        let output = self
            .cloud_command(&self.objects, endpoint)
            .args([mode.as_ref(), manifest.as_os_str(), backup.as_os_str()])
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        redacted(&text);
        assert!(output.status.success(), "object fixture {mode}: {text}");
        println!("{}", text.trim());
    }
    fn read_attachments(&self, url: &str, endpoint: &str, manifest: &Value) {
        for (id, scope) in [
            ("input", "-"),
            ("reply", "1"),
            ("done", "1"),
            ("cancelled", "-"),
        ] {
            let mut command = self.cloud_command(&self.reader, endpoint);
            managed_runtime::configure(&mut command, url, &["worker"]);
            let output = command.args([id, scope]).output().unwrap();
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            redacted(&diagnostic);
            assert!(output.status.success(), "worker reader: {diagnostic}");
            let files: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
            let expected: Vec<Value> = manifest.as_array().unwrap().iter()
                .filter(|row| row["instance_id"] == id && row["request_id"] == if scope == "-" { Value::Null } else { json!(scope) })
                .map(|row| json!({"ordinal":row["ordinal"], "filename":row["filename"], "media_type":row["media_type"], "byte_length":row["byte_length"], "bytes":row["bytes"]})).collect();
            assert_eq!(
                files, expected,
                "restored worker attachment scope {id}/{scope}"
            );
        }
        println!(
            "worker reads: all start/reply/consumed-terminal/cancelled metadata and bytes match"
        );
    }
    fn api(&self, url: &str, endpoint: &str, label: &str, bind: &str) -> Process {
        let mut command = self.cloud_command(&self.api, endpoint);
        managed_runtime::configure(&mut command, url, &["api"]);
        let log = self.evidence.join(format!("{label}.log"));
        let file = fs::File::create(&log).unwrap();
        command
            .arg(bind)
            .stdout(file.try_clone().unwrap())
            .stderr(file);
        let process = Process(command.spawn().unwrap());
        println!("owned API child {}: {}", process.0.id(), log.display());
        process
    }
    async fn assert_rejected(&self, db: &Database, endpoint: &str, label: &str) {
        // Prove authentication independently: incompatibility is not a bad-password test.
        assert_logins(db).await;
        let before = snapshot(&db.client).await;
        let mut process = self.api(&db.url, endpoint, label, "127.0.0.1:0");
        let status = timeout(Duration::from_secs(20), async {
            loop {
                if let Some(status) = process.0.try_wait().unwrap() {
                    break status;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("unsupported schema unexpectedly served or hung");
        assert!(!status.success(), "unsupported schema started");
        let text = fs::read_to_string(self.evidence.join(format!("{label}.log"))).unwrap();
        redacted(&text);
        assert!(
            text.contains("database operation failed"),
            "wrong startup failure: {text}"
        );
        assert!(
            text.contains("blkit db init") && text.contains("blkit db migrate"),
            "missing startup preparation guidance: {text}"
        );
        let settings = db.settings();
        let options = settings.connection_options().unwrap();
        for role in [DatabaseRole::Api, DatabaseRole::Worker] {
            let role = settings.resolve_role(role, &options).unwrap();
            let error = role
                .connect_store(4)
                .await
                .err()
                .expect("unsupported runtime schema accepted");
            assert!(error.contains("blkit db init") && error.contains("blkit db migrate"));
        }
        quiescent(&db.client).await;
        cli(&db.url, false, false);
        assert!(
            before == snapshot(&db.client).await,
            "unsupported startup/CLI mutated schema/data/grants/passwords"
        );
        println!(
            "{label}: valid API/worker logins; generated API and both constructors + CLI refuse; full snapshot unchanged"
        );
    }
}

fn cli(url: &str, roles: bool, success: bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blkit"));
    command
        .env_clear()
        .args(["db", "migrate"])
        .env("BLKIT_ADMIN_DATABASE_URL", url)
        .env("BLKIT_POSTGRES_INSECURE_DEV", "true");
    if roles {
        command
            .env(
                "BLKIT_API_DATABASE_URL",
                managed_runtime::role_url(url, "api"),
            )
            .env(
                "BLKIT_WORKER_DATABASE_URL",
                managed_runtime::role_url(url, "worker"),
            );
    }
    let output = command.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    redacted(&text);
    assert_eq!(output.status.success(), success, "CLI migrate: {text}");
    if success {
        assert!(text.contains("Database migrated.") || text.contains("Database already current."));
    }
}

async fn rows(client: &Client, tables: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for table in tables {
        let text: String = client.query_one(&format!("SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]')::text FROM public.{table} t"), &[]).await.unwrap().get(0);
        result.insert((*table).into(), serde_json::from_str(&text).unwrap());
    }
    sorted(Value::Object(result))
}

fn sorted(mut data: Value) -> Value {
    for rows in data.as_object_mut().unwrap().values_mut() {
        rows.as_array_mut().unwrap().sort_by_key(Value::to_string);
    }
    data
}

async fn snapshot(client: &Client) -> Vec<String> {
    let mut result: Vec<String> = client
        .query(include_str!("support/postgres_schema_snapshot.sql"), &[])
        .await
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();
    // Catalog/ACL/password inspection stays in memory; never include it in assertion output.
    result.push(client.query_one("SELECT jsonb_build_object('classes',(SELECT jsonb_agg(to_jsonb(c)-'oid'-'relfilenode'-'relpages'-'reltuples'-'relallvisible'-'relfrozenxid'-'relminmxid' ORDER BY c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public'),'columns',(SELECT jsonb_agg(jsonb_build_array(c.relname,a.attname,a.attacl) ORDER BY c.relname,a.attnum) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND a.attnum>0),'functions',(SELECT jsonb_agg(to_jsonb(p) ORDER BY p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public'),'roles',(SELECT jsonb_agg(to_jsonb(r) ORDER BY r.oid) FROM pg_authid r WHERE rolname !~ '^pg_'),'memberships',(SELECT jsonb_agg(to_jsonb(m) ORDER BY m.oid) FROM pg_auth_members m),'defaults',(SELECT jsonb_agg(to_jsonb(d) ORDER BY d.oid) FROM pg_default_acl d),'database_acl',(SELECT datacl::text FROM pg_database WHERE datname=current_database()),'schema',(SELECT to_jsonb(n) FROM pg_namespace n WHERE n.nspname='public'),'role_settings',(SELECT jsonb_agg(to_jsonb(s) ORDER BY s.setdatabase,s.setrole) FROM pg_db_role_setting s))::text", &[]).await.unwrap().get(0));
    result.push(rows(client, &TABLES).await.to_string());
    for table in ["blkit_deployment", "blkit_schema_migrations"] {
        if client
            .query_one(
                "SELECT to_regclass($1) IS NOT NULL",
                &[&format!("public.{table}")],
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
        {
            result.push(rows(client, &[table]).await.to_string());
        }
    }
    result
}

async fn quiescent(client: &Client) {
    timeout(Duration::from_secs(10), async {
        loop {
            client.simple_query("SELECT pg_stat_clear_snapshot()").await.unwrap();
            let count: i64 = client.query_one("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND backend_type='client backend'", &[]).await.unwrap().get(0);
            if count == 0 { break; }
            sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("owned database writers/sessions did not quiesce");
}

async fn assert_logins(db: &Database) {
    let settings = db.settings();
    let options = settings.connection_options().unwrap();
    for (role, user) in [
        (DatabaseRole::Api, "blkit_api"),
        (DatabaseRole::Worker, "blkit_worker"),
    ] {
        let resolved = settings.resolve_role(role, &options).unwrap();
        let (client, driver) = resolved.connection().connect().await.unwrap();
        let task = tokio::spawn(driver);
        let row = client.query_one("SELECT current_user::text, NOT has_schema_privilege(current_user,'public','CREATE'), NOT pg_has_role(current_user,'blkit_owner','MEMBER'), NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls FROM pg_roles WHERE rolname=current_user", &[]).await.unwrap();
        assert_eq!(row.get::<_, &str>(0), user);
        assert!(row.get::<_, bool>(1) && row.get::<_, bool>(2) && row.get::<_, bool>(3));
        drop(client);
        task.await.unwrap().unwrap();
    }
    quiescent(&db.client).await;
}

async fn utility(db: &Database, command: &str) {
    // Owned local bootstrap socket only: credentials never appear in utility argv.
    let mut result = db
        .container()
        .exec(
            ExecCommand::new(["sh", "-ec", command])
                .with_env_vars([("PGUSER", "postgres"), ("PGDATABASE", "postgres")]),
        )
        .await
        .unwrap();
    let stdout = result.stdout_to_vec().await.unwrap();
    let stderr = result.stderr_to_vec().await.unwrap();
    assert_eq!(
        result.exit_code().await.unwrap(),
        Some(0),
        "owned PostgreSQL utility failed (diagnostics suppressed)"
    );
    assert!(
        stdout.is_empty() && stderr.is_empty(),
        "unexpected PostgreSQL utility output (suppressed)"
    );
}

async fn dump(db: &Database, destination: &Path) -> Vec<u8> {
    quiescent(&db.client).await;
    utility(
        db,
        "umask 077; pg_dump --format=custom --file=/tmp/rehearsal.dump",
    )
    .await;
    let bytes = db
        .container()
        .copy_file_from("/tmp/rehearsal.dump", Vec::new())
        .await
        .unwrap();
    assert!(
        bytes.starts_with(b"PGDMP"),
        "not a PostgreSQL custom archive"
    );
    private_write(destination, &bytes);
    utility(db, "rm /tmp/rehearsal.dump").await;
    bytes
}

async fn restore(db: &Database, archive: &Path, current: bool) {
    quiescent(&db.client).await;
    let output = Command::new("docker")
        .arg("cp")
        .arg(archive)
        .arg(format!("{}:/tmp/rehearsal.dump", db.container().id()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "copy owned PostgreSQL archive failed (diagnostics suppressed)"
    );
    let command = if current {
        "pg_restore --exit-on-error --single-transaction --dbname=restored_current /tmp/rehearsal.dump; rm /tmp/rehearsal.dump"
    } else {
        "pg_restore --exit-on-error --single-transaction --dbname=postgres /tmp/rehearsal.dump; rm /tmp/rehearsal.dump"
    };
    utility(db, command).await;
}

fn manifest(expected: &Value, late: bool) -> Value {
    let mut entries = expected["instance_attachments"].as_array().unwrap().clone();
    entries.sort_by_key(|row| (row["instance_id"].to_string(), row["ordinal"].as_i64()));
    for row in &mut entries {
        let length = row["byte_length"].as_u64().unwrap() as usize;
        let fill = match row["instance_id"].as_str().unwrap() {
            "input" => 0xa5,
            "reply" => b'R',
            "done" => 0x5a,
            "cancelled" => b'C',
            _ => unreachable!(),
        };
        row["bytes"] = json!(vec![fill; length]);
    }
    if late {
        entries.push(json!({"instance_id":"cancelled", "request_id":null, "ordinal":1, "object_key":"blkit/attachments/retained-newer", "filename":"newer.bin", "media_type":null, "byte_length":3, "bytes":[0,128,255]}));
    }
    Value::Array(entries)
}

// A migration that rewrites checkpoint/request TEXT, loses rows/objects, or a runtime
// that administers unsupported schemas breaks this actual upgrade/recovery exercise.
#[tokio::test]
async fn legacy_upgrade_and_paired_backup_restores_preserve_data_and_private_objects() {
    let fixtures = RuntimeFixtures::build();
    let db = Database::start().await;
    let recovery = Database::start().await;
    let (_objects, endpoint) = seaweed::start().await;
    let (_restored_objects, restored_endpoint) = seaweed::start().await;
    for client in [&db.client, &recovery.client] {
        client.batch_execute("SET TIME ZONE 'UTC'").await.unwrap();
    }
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/schema.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    let frozen = sorted(
        serde_json::from_str(include_str!("fixtures/postgres-legacy/expected-data.json")).unwrap(),
    );
    assert_eq!(rows(&db.client, &TABLES).await, frozen);
    // Additional owned rehearsal rows exercise NULL-only backfill and cancelled retention;
    // the immutable captured assets are never rewritten to match current code.
    db.client.batch_execute("UPDATE public.instances SET queued_at_ms=NULL WHERE id='running'; UPDATE public.instances SET queued_at_ms=42 WHERE id='reply'; INSERT INTO public.instance_attachments(instance_id,ordinal,object_key,filename,media_type,byte_length) VALUES('cancelled',0,'blkit/attachments/retained-cancelled','cancelled.bin',NULL,5)").await.unwrap();
    let mut legacy_expected = frozen;
    for row in legacy_expected["instances"].as_array_mut().unwrap() {
        if row["id"] == "running" {
            row["queued_at_ms"] = Value::Null;
        }
        if row["id"] == "reply" {
            row["queued_at_ms"] = json!(42);
        }
    }
    legacy_expected["instance_attachments"].as_array_mut().unwrap().push(json!({"instance_id":"cancelled", "request_id":null, "ordinal":0, "object_key":"blkit/attachments/retained-cancelled", "filename":"cancelled.bin", "media_type":null, "byte_length":5}));
    let legacy_expected = sorted(legacy_expected);
    assert_eq!(rows(&db.client, &TABLES).await, legacy_expected);
    let original_manifest = manifest(&legacy_expected, false);
    let manifest_path = fixtures.root().join("objects.json");
    private_write(
        &manifest_path,
        &serde_json::to_vec(&original_manifest).unwrap(),
    );
    let object_backup = fixtures.root().join("objects");
    private_directory(&object_backup);
    fixtures.objects(&endpoint, "seed", &manifest_path, &object_backup);
    quiescent(&db.client).await;
    let legacy_dump = fixtures.root().join("legacy.dump");
    let legacy_bytes = dump(&db, &legacy_dump).await;
    fixtures.objects(&endpoint, "backup", &manifest_path, &object_backup);
    let object_bytes: Vec<Vec<u8>> = (0..5)
        .map(|index| fs::read(object_backup.join(format!("{index}.bin"))).unwrap())
        .collect();
    assert_eq!(
        object_bytes.iter().map(Vec::len).collect::<Vec<_>>(),
        [5, 4096, 2048, 0, 17]
    );
    println!(
        "paired LEGACY backup: PGDMP {} bytes + 5 private objects/6166 bytes; no writers",
        legacy_bytes.len()
    );

    cli(&db.url, true, true);
    let mut upgraded_expected = legacy_expected.clone();
    for row in upgraded_expected["instances"].as_array_mut().unwrap() {
        if row["id"] == "running" {
            row["queued_at_ms"] = json!(1700000000000_i64);
        }
    }
    assert_eq!(
        rows(&db.client, &TABLES).await,
        upgraded_expected,
        "only NULL queue timestamp may change"
    );
    assert_logins(&db).await;
    let upgraded_snapshot = snapshot(&db.client).await;
    fixtures.read_attachments(&db.url, &endpoint, &original_manifest);
    assert!(
        upgraded_snapshot == snapshot(&db.client).await,
        "worker reader mutated legacy state"
    );
    let metadata = rows(&db.client, &["blkit_deployment", "blkit_schema_migrations"]).await;
    let current_dump = fixtures.root().join("current.dump");
    let current_bytes = dump(&db, &current_dump).await;
    // No object writers have run; the same private object snapshot pairs with both dumps.
    fixtures.objects(&endpoint, "verify", &manifest_path, &object_backup);
    println!(
        "paired CURRENT backup: PGDMP {} bytes + unchanged private object snapshot",
        current_bytes.len()
    );

    // A later authoritative reference is deliberately absent from both backups.
    // Recovery must retain it in the source, not use an older snapshot as a purge list.
    let newer_manifest = manifest(&legacy_expected, true);
    let newer_path = fixtures.root().join("newer-objects.json");
    private_write(&newer_path, &serde_json::to_vec(&newer_manifest).unwrap());
    let late_path = fixtures.root().join("late-object.json");
    private_write(
        &late_path,
        &serde_json::to_vec(&json!([newer_manifest.as_array().unwrap().last().unwrap()])).unwrap(),
    );
    fixtures.objects(&endpoint, "seed", &late_path, &object_backup);
    db.client.batch_execute("INSERT INTO public.instance_attachments(instance_id,ordinal,object_key,filename,byte_length) VALUES('cancelled',1,'blkit/attachments/retained-newer','newer.bin',3)").await.unwrap();
    quiescent(&db.client).await;
    let source_retained_snapshot = snapshot(&db.client).await;

    // Restore the PRE-upgrade archive into another empty owned PostgreSQL container.
    restore(&recovery, &legacy_dump, false).await;
    assert_eq!(
        rows(&recovery.client, &TABLES).await,
        legacy_expected,
        "actual legacy restore"
    );
    fixtures.objects(
        &restored_endpoint,
        "restore",
        &manifest_path,
        &object_backup,
    );
    cli(&recovery.url, true, true);
    assert_eq!(
        rows(&recovery.client, &TABLES).await,
        upgraded_expected,
        "restored legacy adoption"
    );
    assert_logins(&recovery).await;
    let recovered_snapshot = snapshot(&recovery.client).await;
    fixtures.read_attachments(&recovery.url, &restored_endpoint, &original_manifest);
    assert!(recovered_snapshot == snapshot(&recovery.client).await);
    println!(
        "LEGACY restore -> exact seven-table snapshot -> matching CLI migrate -> exact NULL-only backfill + worker bytes: PASS"
    );

    // Restore the CURRENT archive into another new database, using already-managed
    // cluster roles. pg_dump does not capture cluster roles or per-database settings.
    recovery
        .client
        .batch_execute("CREATE DATABASE restored_current")
        .await
        .unwrap();
    restore(&recovery, &current_dump, true).await;
    fixtures.objects(
        &restored_endpoint,
        "restore",
        &manifest_path,
        &object_backup,
    );
    let current_url =
        recovery.url.strip_suffix("postgres").unwrap().to_owned() + "restored_current";
    let (current, driver) = tokio_postgres::connect(&current_url, tokio_postgres::NoTls)
        .await
        .unwrap();
    let driver = tokio::spawn(driver);
    current.batch_execute("SET TIME ZONE 'UTC'").await.unwrap();
    assert_eq!(rows(&current, &TABLES).await, upgraded_expected);
    assert_eq!(
        rows(&current, &["blkit_deployment", "blkit_schema_migrations"]).await,
        metadata,
        "history timestamps/identity/checksum restored exactly"
    );
    quiescent(&current).await;
    cli(&current_url, false, true);
    assert_eq!(rows(&current, &TABLES).await, upgraded_expected);
    assert_eq!(
        rows(&current, &["blkit_deployment", "blkit_schema_migrations"]).await,
        metadata
    );
    let current_snapshot = snapshot(&current).await;
    fixtures.read_attachments(&current_url, &restored_endpoint, &original_manifest);
    assert!(current_snapshot == snapshot(&current).await);
    println!(
        "CURRENT restore -> exact seven tables + history -> canonical grants/database reconciliation -> worker bytes: PASS"
    );

    // Original objects were never rewritten; retain both sole paired backups too.
    assert_eq!(current.query_one("SELECT count(*) FROM public.instance_attachments WHERE object_key='blkit/attachments/retained-newer'", &[]).await.unwrap().get::<_, i64>(0), 0);
    fixtures.objects(&endpoint, "verify", &newer_path, &object_backup);
    fixtures.read_attachments(&db.url, &endpoint, &newer_manifest);
    assert!(fs::read(&legacy_dump).unwrap() == legacy_bytes);
    assert!(fs::read(&current_dump).unwrap() == current_bytes);
    for (index, bytes) in object_bytes.iter().enumerate() {
        assert!(fs::read(object_backup.join(format!("{index}.bin"))).unwrap() == *bytes);
    }
    println!(
        "retention: source 6 objects (including newer committed reference), both DB archives and 5 original object backup files unchanged; no object deletion"
    );

    // Unmodified generated API performs normal reconciliation AFTER the exact restore
    // assertions, only in this separate restored CURRENT database, never the source.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut api = fixtures.api(
        &current_url,
        &restored_endpoint,
        "current-restored-api",
        &format!("127.0.0.1:{port}"),
    );
    timeout(Duration::from_secs(20), async {
        loop {
            assert!(
                api.0.try_wait().unwrap().is_none(),
                "restored API stopped before readiness"
            );
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("restored API did not start");
    managed_runtime::assert_child_configuration(&api.0, &["api"]);
    managed_runtime::assert_sessions(&current_url, &["api"]).await;
    for (id, status) in [
        ("done", "completed"),
        ("cancelled", "cancelled"),
        ("input", "waiting-for-input"),
        ("reply", "pending"),
    ] {
        let output = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--max-time",
                "5",
                &format!("http://127.0.0.1:{port}/instances/{id}"),
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "restored API HTTP read failed");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], status);
    }
    drop(api);
    quiescent(&current).await;
    let after_runtime = rows(&current, &TABLES).await;
    let changed_instances: Vec<_> = after_runtime["instances"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            !upgraded_expected["instances"]
                .as_array()
                .unwrap()
                .contains(row)
        })
        .map(|row| row["id"].clone())
        .collect();
    println!(
        "real restored API: limited-role/no-admin HTTP reads PASS; normal reconciliation changed instance IDs only after preservation boundary: {changed_instances:?}"
    );
    for before in upgraded_expected["instances"].as_array().unwrap() {
        let after = after_runtime["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|after| after["id"] == before["id"])
            .unwrap();
        assert_eq!(
            before["checkpoint"], after["checkpoint"],
            "runtime checkpoint bytes"
        );
    }
    fixtures.read_attachments(&current_url, &restored_endpoint, &original_manifest);
    let api_log = fs::read_to_string(fixtures.evidence.join("current-restored-api.log")).unwrap();
    redacted(&api_log);
    drop(current);
    driver.await.unwrap().unwrap();

    // Faults are confined to the owned recovery DB; original source/backups survive.
    for (label, change, undo) in [
        (
            "older-version",
            "UPDATE public.blkit_schema_migrations SET version=0",
            "UPDATE public.blkit_schema_migrations SET version=1",
        ),
        (
            "newer-version",
            "UPDATE public.blkit_schema_migrations SET version=2",
            "UPDATE public.blkit_schema_migrations SET version=1",
        ),
        (
            "checksum-drift",
            "UPDATE public.blkit_schema_migrations SET checksum=repeat('0',64)",
            "",
        ),
    ] {
        recovery.client.batch_execute(change).await.unwrap();
        fixtures
            .assert_rejected(&recovery, &restored_endpoint, label)
            .await;
        if !undo.is_empty() {
            recovery.client.batch_execute(undo).await.unwrap();
        }
    }
    // Restore the checksum from the supported runtime contract, not copied grant/DDL logic.
    recovery
        .client
        .execute(
            "UPDATE public.blkit_schema_migrations SET checksum=$1",
            &[&blkit_core::postgres_schema::SUPPORTED_SCHEMA_CHECKSUM],
        )
        .await
        .unwrap();
    recovery
        .client
        .batch_execute("ALTER TABLE public.instances ADD COLUMN unknown_shape text")
        .await
        .unwrap();
    let before = snapshot(&recovery.client).await;
    cli(&recovery.url, false, false);
    assert!(
        before == snapshot(&recovery.client).await,
        "unknown structural schema changed"
    );
    println!("unknown populated schema: actual CLI refusal, complete snapshot unchanged: PASS");
    assert!(
        source_retained_snapshot == snapshot(&db.client).await,
        "recovery changed authoritative source"
    );
    assert!(fs::read(&legacy_dump).unwrap() == legacy_bytes);
    assert!(fs::read(&current_dump).unwrap() == current_bytes);
    for (index, bytes) in object_bytes.iter().enumerate() {
        assert!(fs::read(object_backup.join(format!("{index}.bin"))).unwrap() == *bytes);
    }
    fixtures.objects(&endpoint, "verify", &newer_path, &object_backup);
    fixtures.objects(&restored_endpoint, "verify", &manifest_path, &object_backup);
    println!(
        "REHEARSAL PASS: two real pg_dump/pg_restore rows; exact legacy/current state and private bytes; limited-role generated startup; unsupported valid-auth nonmutation; owned cleanup on drop"
    );
}
