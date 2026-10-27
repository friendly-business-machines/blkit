use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

const SENTINEL: &str = "cli-credential-sentinel";

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "blkit-cli-db-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blkit"));
        command.current_dir(self.path()).env("PATH", "");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("BLKIT_") {
                command.env_remove(name);
            }
        }
        command
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn assert_redacted(output: &Output) {
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SENTINEL));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SENTINEL));
}

#[test]
fn db_commands_are_discoverable_in_help_and_all_supported_completions() {
    let directory = Directory::new();
    let malformed_secret = directory.path().join("malformed.secret");
    fs::write(&malformed_secret, SENTINEL).unwrap();
    fs::write(
        directory.path().join(".env"),
        format!("INVALID='unterminated-{SENTINEL}"),
    )
    .unwrap();
    for args in [
        vec!["--help"],
        vec!["db", "--help"],
        vec!["db", "init", "--help"],
        vec!["db", "migrate", "--help"],
    ] {
        let output = directory
            .command()
            .args(&args)
            .env("BLKIT_ADMIN_DATABASE_URL_FILE", &malformed_secret)
            .env("BLKIT_API_DATABASE_URL_FILE", &malformed_secret)
            .env("BLKIT_WORKER_DATABASE_URL_FILE", &malformed_secret)
            .env("BLKIT_POSTGRES_CA_FILE", &malformed_secret)
            .output()
            .unwrap();
        assert!(output.status.success(), "help failed: {args:?}");
        assert_redacted(&output);
        let text = String::from_utf8_lossy(&output.stdout);
        if args.len() == 1 {
            for invocation in ["blkit db init", "blkit db migrate"] {
                assert!(text.contains(invocation), "root help omits {invocation}");
            }
        }
        for requirement in [
            "Administration uses",
            "BLKIT_ADMIN_DATABASE_URL",
            "runtime logins",
            "BLKIT_API_DATABASE_URL",
            "BLKIT_WORKER_DATABASE_URL",
            "_FILE",
            "--env-file",
            "dotenv",
            "secret files",
            "BLKIT_POSTGRES_CA_FILE",
            "corporate PEM CA bundle",
            "BLKIT_POSTGRES_INSECURE_DEV",
            "development/testing only",
            "--role-prefix",
            "<prefix>_owner",
            "<prefix>_api",
            "<prefix>_worker",
            "existing, dedicated",
            "does not create a server or database",
        ] {
            assert!(
                text.contains(requirement),
                "help omits {requirement}: {args:?}"
            );
        }
    }
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let output = directory
            .command()
            .args(["--completions", shell])
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        for name in ["db", "init", "migrate", "transpile", "update"] {
            assert!(text.contains(name), "{shell} omits {name}");
        }
        for option in ["env-file", "role-prefix"] {
            let spelling = if shell == "fish" {
                format!("-l {option}")
            } else {
                format!("--{option}")
            };
            assert!(text.contains(&spelling), "{shell} omits option {option}");
        }
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn credential_arguments_and_legacy_urls_are_rejected_without_echo() {
    let directory = Directory::new();
    let url = format!("postgresql://user:{SENTINEL}@localhost/db");
    for args in [
        vec!["db", "init", "--database-url", &url],
        vec!["db", "migrate", "--admin-database-url", &url],
        vec!["db", "init", "--username", SENTINEL],
        vec!["db", "migrate", "--password", SENTINEL],
        vec!["db", "init", &url],
        vec!["db", "migrate", &url],
        vec![&url, "out.rs"],
        vec!["transpile", &url],
        vec!["update", &url],
        vec!["--completions", SENTINEL],
        vec!["db", "init", "--role-prefix", SENTINEL],
    ] {
        let output = directory.command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert_redacted(&output);
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("prepared"));
    }
}

// FIFO reads would hang and any connection would be accepted by this owned listener.
#[cfg(unix)]
#[test]
fn help_completion_and_invalid_usage_do_not_read_files_or_connect() {
    let directory = Directory::new();
    let fifo = directory.path().join("unreadable-until-opened");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "postgresql://user:{SENTINEL}@{}/db",
        listener.local_addr().unwrap()
    );
    let selected = fifo.to_str().unwrap();
    let cases: &[(&[&str], bool)] = &[
        (&["--help"], true),
        (&["--version"], true),
        (&["--completions", "bash"], true),
        (&["--completions", "elvish"], true),
        (&["--completions", "fish"], true),
        (&["--completions", "powershell"], true),
        (&["--completions", "zsh"], true),
        (&["--completions", "invalid"], false),
        (&["db"], false),
        (&["db", "--help"], true),
        (&["db", "init", "--env-file", selected, "--help"], true),
        (&["db", "migrate", "--env-file", selected, "--help"], true),
        (&["db", "init", "--env-file", selected, "--unknown"], false),
        (
            &[
                "db",
                "init",
                "--env-file",
                selected,
                "--role-prefix",
                SENTINEL,
            ],
            false,
        ),
        (
            &[
                "db",
                "migrate",
                "--env-file",
                selected,
                "--role-prefix",
                SENTINEL,
            ],
            false,
        ),
        (&["db", "migrate", "--env-file", selected, &url], false),
    ];
    for (args, success) in cases {
        let stdout = directory.path().join("stdout");
        let stderr = directory.path().join("stderr");
        let mut child = directory
            .command()
            .args(*args)
            .env("BLKIT_ADMIN_DATABASE_URL", &url)
            .env("BLKIT_API_DATABASE_URL_FILE", &fifo)
            .env("BLKIT_WORKER_DATABASE_URL_FILE", &fifo)
            .env("BLKIT_POSTGRES_CA_FILE", &fifo)
            .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
            .stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("invocation blocked reading configuration: {args:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            status.success(),
            *success,
            "incorrect exit status: {args:?}"
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!fs::read_to_string(stdout).unwrap().contains(SENTINEL));
        assert!(!fs::read_to_string(stderr).unwrap().contains(SENTINEL));
    }
}

use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio_postgres::{Client, NoTls};

struct Database {
    _container: ContainerAsync<Postgres>,
    client: Client,
    endpoint: String,
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
        let endpoint = format!("{host}:{port}/postgres");
        let client = connect(&format!("postgresql://postgres:postgres@{endpoint}")).await;
        client
            .batch_execute("ALTER ROLE postgres PASSWORD 'cli-credential-sentinel'")
            .await
            .unwrap();
        Self {
            _container: container,
            client,
            endpoint,
        }
    }
    fn admin_url(&self) -> String {
        format!("postgresql://postgres:{SENTINEL}@{}", self.endpoint)
    }
    fn runtime_url(&self, prefix: &str, role: &str) -> String {
        format!("postgresql://{prefix}_{role}:{SENTINEL}@{}", self.endpoint)
    }
    fn command(&self, directory: &Directory) -> Command {
        let mut command = directory.command();
        command
            .env("BLKIT_ADMIN_DATABASE_URL", self.admin_url())
            .env("BLKIT_POSTGRES_INSECURE_DEV", "true");
        command
    }
}
async fn connect(url: &str) -> Client {
    let (client, driver) = tokio_postgres::connect(url, NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = driver.await;
    });
    client
}
fn assert_success(output: &Output, outcome: &str) {
    assert_redacted(output);
    assert!(
        output.status.success(),
        "administration failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(outcome));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr
            .matches("postgres transport is insecure plaintext")
            .count(),
        1
    );
}
fn assert_failure(output: &Output, context: &str) {
    assert_redacted(output);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "failed operation claimed success");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(context),
        "missing safe context {context}"
    );
}

// Dispatch must really commit, accept file inputs, and leave persisted prefixes and passwords intact.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_init_and_admin_only_reruns_use_real_runner_and_lazy_credentials() {
    let db = Database::start().await;
    let directory = Directory::new();
    fs::write(
        directory.path().join(".env"),
        format!("INVALID='unterminated-{SENTINEL}"),
    )
    .unwrap();
    fs::write(
        directory.path().join("admin.secret"),
        format!("{}\r\n", db.admin_url()),
    )
    .unwrap();
    fs::write(
        directory.path().join("api.secret"),
        format!("{}\n", db.runtime_url("cli_app", "api")),
    )
    .unwrap();
    fs::write(
        directory.path().join("worker.secret"),
        db.runtime_url("cli_app", "worker"),
    )
    .unwrap();
    fs::write(directory.path().join("selected.env"), "BLKIT_ADMIN_DATABASE_URL_FILE=admin.secret\nBLKIT_API_DATABASE_URL_FILE=api.secret\nBLKIT_WORKER_DATABASE_URL_FILE=worker.secret\nBLKIT_POSTGRES_INSECURE_DEV=true\n").unwrap();
    let output = directory
        .command()
        .args([
            "db",
            "init",
            "--role-prefix",
            "cli_app",
            "--env-file",
            "selected.env",
        ])
        .output()
        .unwrap();
    assert_success(&output, "initialized");
    let prefix: String = db
        .client
        .query_one("SELECT role_prefix FROM public.blkit_deployment", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(prefix, "cli_app");
    let api = connect(&db.runtime_url("cli_app", "api")).await;
    api.batch_execute("INSERT INTO public.instances (id,namespace,version,process,input,status,created_at,updated_at) VALUES ('saved','n','v','p','{}','queued',1,1)").await.unwrap();
    let worker = connect(&db.runtime_url("cli_app", "worker")).await;
    worker
        .batch_execute("INSERT INTO public.workers(id,heartbeat_at) VALUES ('saved-worker',1)")
        .await
        .unwrap();
    let passwords: Vec<String> = db.client.query("SELECT rolpassword FROM pg_authid WHERE rolname IN ('cli_app_api','cli_app_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|row| row.get(0)).collect();
    for args in [
        vec!["db", "init", "--role-prefix", "cli_app"],
        vec!["db", "migrate"],
    ] {
        let output = db
            .command(&directory)
            .args(args)
            .env("BLKIT_API_DATABASE_URL", SENTINEL)
            .env(
                "BLKIT_API_DATABASE_URL_FILE",
                directory.path().join(SENTINEL),
            )
            .env(
                "BLKIT_WORKER_DATABASE_URL_FILE",
                directory.path().join(SENTINEL),
            )
            .env("BLKIT_WORKER_DB_POOL_MAX", SENTINEL)
            .env("BLKIT_API_DB_POOL_MAX", SENTINEL)
            .env("BLKIT_ATTACHMENT_BACKEND", SENTINEL)
            .env("BLKIT_ATTACHMENT_BUCKET", SENTINEL)
            .env("AWS_ACCESS_KEY_ID", SENTINEL)
            .output()
            .unwrap();
        assert_success(&output, "already current");
    }
    let after: Vec<String> = db.client.query("SELECT rolpassword FROM pg_authid WHERE rolname IN ('cli_app_api','cli_app_worker') ORDER BY rolname", &[]).await.unwrap().iter().map(|row| row.get(0)).collect();
    assert!(passwords == after, "rerun changed existing passwords");
    assert_eq!(
        db.client
            .query_one(
                "SELECT count(*) FROM public.instances WHERE id='saved'",
                &[]
            )
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
    assert_eq!(
        db.client
            .query_one("SELECT count(*) FROM public.blkit_schema_migrations", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
    assert!(!directory.path().join(".blkit").exists());
    assert!(!directory.path().join("Cargo.toml").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_legacy_migrate_commits_with_explicit_prefix_and_preserves_data() {
    let db = Database::start().await;
    let directory = Directory::new();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/schema.sql"))
        .await
        .unwrap();
    db.client
        .batch_execute(include_str!("fixtures/postgres-legacy/data.sql"))
        .await
        .unwrap();
    let rows: i64 = db
        .client
        .query_one("SELECT count(*) FROM public.instances", &[])
        .await
        .unwrap()
        .get(0);
    let output = db
        .command(&directory)
        .args(["db", "migrate", "--role-prefix", "cli_legacy"])
        .env(
            "BLKIT_API_DATABASE_URL",
            db.runtime_url("cli_legacy", "api"),
        )
        .env(
            "BLKIT_WORKER_DATABASE_URL",
            db.runtime_url("cli_legacy", "worker"),
        )
        .output()
        .unwrap();
    assert_success(&output, "migrated");
    assert_eq!(
        db.client
            .query_one("SELECT count(*) FROM public.instances", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        rows
    );
    assert_eq!(
        db.client
            .query_one("SELECT role_prefix FROM public.blkit_deployment", &[])
            .await
            .unwrap()
            .get::<_, &str>(0),
        "cli_legacy"
    );
    assert_eq!(
        db.client
            .query_one("SELECT version FROM public.blkit_schema_migrations", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
    let output = db
        .command(&directory)
        .args(["db", "migrate"])
        .output()
        .unwrap();
    assert_success(&output, "already current");
}

#[test]
fn configuration_errors_are_redacted_before_connect_or_warning() {
    let directory = Directory::new();
    let secret = directory.path().join(SENTINEL);
    fs::write(&secret, SENTINEL).unwrap();
    for (setting, value, context) in [
        (
            "BLKIT_ADMIN_DATABASE_URL",
            SENTINEL.to_string(),
            "BLKIT_ADMIN_DATABASE_URL",
        ),
        (
            "BLKIT_ADMIN_DATABASE_URL_FILE",
            secret.to_str().unwrap().to_string(),
            "BLKIT_ADMIN_DATABASE_URL_FILE",
        ),
        (
            "BLKIT_ADMIN_DATABASE_URL_FILE",
            directory
                .path()
                .join(format!("missing-{SENTINEL}"))
                .to_str()
                .unwrap()
                .to_string(),
            "BLKIT_ADMIN_DATABASE_URL_FILE",
        ),
        (
            "BLKIT_POSTGRES_CA_FILE",
            secret.to_str().unwrap().to_string(),
            "BLKIT_POSTGRES_CA_FILE",
        ),
        (
            "BLKIT_POSTGRES_INSECURE_DEV",
            SENTINEL.to_string(),
            "BLKIT_POSTGRES_INSECURE_DEV",
        ),
    ] {
        let output = directory
            .command()
            .args(["db", "init"])
            .env(setting, value)
            .output()
            .unwrap();
        assert_failure(&output, context);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("insecure plaintext"));
    }
    fs::write(&secret, format!("BROKEN='unterminated-{SENTINEL}")).unwrap();
    let output = directory
        .command()
        .args(["db", "init", "--env-file", secret.to_str().unwrap()])
        .env(
            "BLKIT_ADMIN_DATABASE_URL",
            "postgresql://postgres:postgres@localhost/postgres",
        )
        .output()
        .unwrap();
    assert_failure(&output, "env-file");
    fs::write(
        &secret,
        format!("BLKIT_ADMIN_DATABASE_URL={SENTINEL}\nBLKIT_POSTGRES_INSECURE_DEV=true\n"),
    )
    .unwrap();
    assert_failure(
        &directory
            .command()
            .args(["db", "init", "--env-file", secret.to_str().unwrap()])
            .output()
            .unwrap(),
        "BLKIT_ADMIN_DATABASE_URL",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_tls_permissions_driver_and_migration_failures_never_claim_success_or_leak() {
    let db = Database::start().await;
    let directory = Directory::new();
    let output = db
        .command(&directory)
        .args(["db", "init"])
        .env(
            "BLKIT_ADMIN_DATABASE_URL",
            format!("postgresql://postgres:{SENTINEL}-wrong@{}", db.endpoint),
        )
        .output()
        .unwrap();
    assert_failure(&output, "administrative connection failed");
    let output = db
        .command(&directory)
        .args(["db", "init"])
        .env(
            "BLKIT_ADMIN_DATABASE_URL",
            format!(
                "postgresql://postgres:{SENTINEL}@{}/{SENTINEL}",
                db.endpoint.split('/').next().unwrap()
            ),
        )
        .output()
        .unwrap();
    assert_failure(&output, "administrative connection failed");
    let output = db
        .command(&directory)
        .args(["db", "init"])
        .env_remove("BLKIT_POSTGRES_INSECURE_DEV")
        .output()
        .unwrap();
    assert_failure(&output, "administrative connection failed");
    db.client
        .batch_execute("CREATE ROLE limited LOGIN PASSWORD 'cli-credential-sentinel'")
        .await
        .unwrap();
    let output = db
        .command(&directory)
        .args(["db", "init"])
        .env(
            "BLKIT_ADMIN_DATABASE_URL",
            format!("postgresql://limited:{SENTINEL}@{}", db.endpoint),
        )
        .output()
        .unwrap();
    assert_failure(&output, "authority");
    // Missing runtime secret must roll back the initial DDL, not print a success.
    let output = db
        .command(&directory)
        .args(["db", "init"])
        .env("BLKIT_API_DATABASE_URL", SENTINEL)
        .output()
        .unwrap();
    assert_failure(&output, "BLKIT_API_DATABASE_URL");
    assert!(
        db.client
            .query_one(
                "SELECT to_regclass('public.blkit_schema_migrations') IS NULL",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    db.client
        .batch_execute(
            "CREATE TABLE public.instances (id integer); INSERT INTO public.instances VALUES (42)",
        )
        .await
        .unwrap();
    let output = db
        .command(&directory)
        .args(["db", "migrate"])
        .output()
        .unwrap();
    assert_failure(&output, "schema");
    assert_eq!(
        db.client
            .query_one("SELECT id FROM public.instances", &[])
            .await
            .unwrap()
            .get::<_, i32>(0),
        42
    );
    assert!(
        db.client
            .query_one(
                "SELECT to_regclass('public.blkit_schema_migrations') IS NULL",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
}
