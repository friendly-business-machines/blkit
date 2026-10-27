//! Task 7.3: real generated outputs, CLI-managed SCRAM logins, and both transports.
use blkit_core::{
    compiled_graph::GraphCheckpoint,
    postgres_config::{DatabaseRole, EffectivePostgresConfig},
    postgres_connection,
};
use serde_json::{Value, json};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{sleep, timeout};

#[path = "../../../tools/test-support/generated_remote.rs"]
#[allow(dead_code)]
mod generated_remote;
#[path = "support/managed_postgres.rs"]
mod managed_postgres;
#[path = "../../../tools/test-support/managed_runtime.rs"]
#[allow(dead_code)]
mod managed_runtime;
#[path = "../../blkit-core/tests/support/postgres_tls.rs"]
#[allow(dead_code, unexpected_cfgs)]
mod postgres_tls;
#[path = "../../../tools/test-support/remote_attachment_reader.rs"]
mod remote_attachment_reader;
#[path = "../../../tools/test-support/seaweed.rs"]
mod seaweed;
use generated_remote::Generated;

// Reuses the attachment suite's review business flow, plus a checkpointed task flow.
const SOURCE: &str = r#"namespace orders; version "1";
start_event start { output amount: Number; }
input_request review { prompt "Approve?"; output approved: Bool; }
end_event done { input result: Bool; }
process route {
  flow start -> review; flow review -> done;
  bind review.approved -> done.result;
}
start_event work_start { output mode: String; }
end_event work_done { input result: String; }
decision_task before {
  input mode: String; output result: String = value;
  literal_expression value { output result: String; expression mode; }
}
decision_task action {
  input mode: String; output result: String = value;
  literal_expression value { output result: String; expression mode; }
}
process work {
  retry max_retries 2 retry_for "60s" retry_delay "100ms" backoff exponential;
  flow work_start -> before; flow before -> action; flow action -> work_done;
  bind work_start.mode -> before.mode; bind before.result -> action.mode;
  bind action.result -> work_done.result;
}
process limited {
  deadline queued "2s";
  retry max_retries 2 retry_for "60s" retry_delay "100ms" backoff exponential;
  flow work_start -> before; flow before -> action; flow action -> work_done;
  bind work_start.mode -> before.mode; bind before.result -> action.mode;
  bind action.result -> work_done.result;
}
"#;

// Only fixture task bodies change: generated entrypoints/store/control/worker remain untouched.
const TASKS: &str = r#"
pub fn named_graph_definitions() -> Vec<blkit_core::compiled_graph::GraphDefinition> {
    let mut graphs = compiled_graph_definitions();
    for graph in &mut graphs {
        for node in &mut graph.nodes {
            if !matches!(node.name, "before" | "action") { continue; }
            let name = node.name;
            node.kind = blkit_core::compiled_graph::GraphNodeKind::Task(std::sync::Arc::new(move |input, _| {
                use std::io::Write;
                let root = std::path::PathBuf::from(std::env::var("BLKIT_DEPLOYMENT_GATE").unwrap());
                let mode = input["mode"].as_str().unwrap();
                assert!(["ok", "retry", "cancel", "deadline", "loss"].contains(&mode));
                let log = root.join(format!("{mode}-{name}.log"));
                writeln!(std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap(), "{}", std::process::id()).unwrap();
                if name == "action" {
                    if mode == "retry" && std::fs::OpenOptions::new().create_new(true).write(true).open(root.join("retry-once")).is_ok() {
                        return Err("intentional disposable task failure".into());
                    }
                    while matches!(mode, "cancel" | "deadline" | "loss") && !root.join(format!("{mode}-release")).exists() {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                let result = Ok(input["mode"].clone());
                if name == "action" && matches!(mode, "cancel" | "deadline") {
                    std::fs::write(root.join(format!("{mode}-action-return.log")), "ready-to-return\n").unwrap();
                }
                result
            }));
        }
    }
    graphs
}
"#;

struct Database {
    tls: Option<postgres_tls::TlsPostgres>,
    plain: Option<managed_postgres::Database>,
    settings: EffectivePostgresConfig,
}
impl Database {
    async fn start(secure: bool) -> Self {
        let (tls, plain, settings) = if secure {
            let db = postgres_tls::TlsPostgres::start_unprepared().await;
            let parsed: tokio_postgres::Config = db.url.parse().unwrap();
            assert_eq!(
                parsed.get_hosts(),
                &[tokio_postgres::config::Host::Tcp("localhost".into())]
            );
            assert_eq!(parsed.get_ports().len(), 1);
            assert_eq!(parsed.get_hostaddrs().len(), 1);
            assert!(parsed.get_hostaddrs()[0].is_ipv4());
            assert!(
                parsed.get_user() == Some("postgres") && parsed.get_password() == Some(b"postgres")
            );
            assert_eq!(parsed.get_dbname(), Some("postgres"));
            let admin = format!(
                "postgres://postgres:postgres@localhost:{}/postgres?hostaddr={}",
                parsed.get_ports()[0],
                parsed.get_hostaddrs()[0]
            );
            let api = managed_runtime::role_url(&admin, "api");
            let worker = managed_runtime::role_url(&admin, "worker");
            let settings = EffectivePostgresConfig::from_iter(
                None,
                [
                    ("BLKIT_ADMIN_DATABASE_URL", admin),
                    ("BLKIT_API_DATABASE_URL", api),
                    ("BLKIT_WORKER_DATABASE_URL", worker),
                    (
                        "BLKIT_POSTGRES_CA_FILE",
                        postgres_tls::certificate("ca.pem")
                            .to_str()
                            .unwrap()
                            .to_owned(),
                    ),
                ],
            )
            .unwrap();
            (Some(db), None, settings)
        } else {
            let db = managed_postgres::Database::start().await;
            let settings = db.settings();
            (None, Some(db), settings)
        };
        let db = Self {
            tls,
            plain,
            settings,
        };
        let count: i64 = db
            .admin()
            .query_one(
                "SELECT count(*) FROM pg_tables WHERE schemaname='public'",
                &[],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            count, 0,
            "CLI provisioning must start from an empty schema, not fake metadata"
        );
        let auth: String = db
            .admin()
            .query_one("SHOW password_encryption", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(auth, "scram-sha-256");
        let rules = db
            .admin()
            .query(
                "SELECT auth_method FROM pg_hba_file_rules WHERE type LIKE 'host%'",
                &[],
            )
            .await
            .unwrap();
        assert!(!rules.is_empty());
        assert!(
            rules
                .iter()
                .all(|r| r.get::<_, String>(0) == "scram-sha-256")
        );
        db.cli("init", true);
        for role in [DatabaseRole::Api, DatabaseRole::Worker] {
            let options = db.settings.connection_options().unwrap();
            let resolved = db.settings.resolve_role(role, &options).unwrap();
            let (client, driver) = resolved.connection().connect().await.unwrap();
            let task = tokio::spawn(driver);
            let user: String = client
                .query_one("SELECT current_user", &[])
                .await
                .unwrap()
                .get(0);
            assert_eq!(
                user,
                if matches!(role, DatabaseRole::Api) {
                    "blkit_api"
                } else {
                    "blkit_worker"
                }
            );
            drop(client);
            task.await.unwrap().unwrap();
            let wrong_url = db
                .url(role)
                .replace("startup-test", "wrong-disposable-password");
            let wrong = postgres_connection::PostgresConnection::with_options(&wrong_url, &options)
                .unwrap();
            assert!(
                wrong.connect().await.is_err(),
                "SCRAM must reject wrong runtime passwords"
            );
        }
        println!(
            "CLI-managed blkit_api and blkit_worker authenticated; all TCP HBA rules SCRAM and wrong role passwords rejected"
        );
        db
    }
    fn admin(&self) -> &tokio_postgres::Client {
        if let Some(tls) = &self.tls {
            &tls.admin
        } else {
            &self.plain.as_ref().unwrap().client
        }
    }
    fn url(&self, role: DatabaseRole) -> &str {
        self.settings
            .get(match role {
                DatabaseRole::Api => "BLKIT_API_DATABASE_URL",
                DatabaseRole::Worker => "BLKIT_WORKER_DATABASE_URL",
                DatabaseRole::Admin => "BLKIT_ADMIN_DATABASE_URL",
            })
            .unwrap()
            .unwrap()
    }
    fn transport(&self, command: &mut Command) {
        command
            .env_remove("BLKIT_POSTGRES_INSECURE_DEV")
            .env_remove("BLKIT_POSTGRES_CA_FILE");
        if self.tls.is_some() {
            command.env(
                "BLKIT_POSTGRES_CA_FILE",
                postgres_tls::certificate("ca.pem"),
            );
        } else {
            command.env("BLKIT_POSTGRES_INSECURE_DEV", "true");
        }
    }
    fn cli(&self, operation: &str, roles: bool) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blkit"));
        command
            .env_clear()
            .args(["db", operation])
            .env("BLKIT_ADMIN_DATABASE_URL", self.url(DatabaseRole::Admin));
        self.transport(&mut command);
        if roles {
            command
                .env("BLKIT_API_DATABASE_URL", self.url(DatabaseRole::Api))
                .env("BLKIT_WORKER_DATABASE_URL", self.url(DatabaseRole::Worker));
        }
        let output = command.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        redacted(&text);
        assert!(output.status.success(), "CLI {operation}: {text}");
        println!("CLI {operation} successful (runtime URLs supplied={roles})");
    }
    fn runtime(&self, command: &mut Command, root: &Path, roles: &[&str]) {
        managed_runtime::configure(command, "", &[]);
        self.transport(command);
        let mut dotenv = String::new();
        for role in roles {
            let setting = format!("BLKIT_{}_DATABASE_URL_FILE", role.to_uppercase());
            let path = root.join(format!("{role}.url"));
            fs::write(
                &path,
                self.url(if *role == "api" {
                    DatabaseRole::Api
                } else {
                    DatabaseRole::Worker
                }),
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            dotenv.push_str(&format!("{setting}={}\n", path.display()));
        }
        let file = root.join(format!("{}.env", roles.join("-")));
        fs::write(&file, dotenv).unwrap();
        command
            .arg("--env-file")
            .arg(file)
            .env("BLKIT_DEPLOYMENT_GATE", root);
    }
    async fn identities(&self, old_listener: Option<i32>) -> i32 {
        timeout(Duration::from_secs(10), async {
            loop {
                self.admin().simple_query("SELECT pg_stat_clear_snapshot()").await.unwrap();
                let rows = self.admin().query("SELECT a.pid, a.usename, a.application_name, a.query, s.ssl FROM pg_stat_activity a JOIN pg_stat_ssl s USING(pid) WHERE a.datname=current_database() AND a.usename IN ('blkit_api', 'blkit_worker')", &[]).await.unwrap();
                let mut api = 0;
                let mut worker = 0;
                let mut listener = None;
                for r in rows {
                    let pid: i32 = r.get(0);
                    let user: String = r.get(1);
                    let app: String = r.get(2);
                    let query: String = r.get(3);
                    assert_eq!(r.get::<_, bool>(4), self.tls.is_some(), "actual runtime session transport");
                    if query.starts_with("LISTEN ") {
                        assert_eq!(user, "blkit_api");
                        assert!(app.starts_with("blkit_") && query.contains(&app));
                        listener = Some(pid);
                    } else if user == "blkit_api" { api += 1; } else { worker += 1; }
                }
                if let Some(pid) = listener.filter(|pid| Some(*pid) != old_listener)
                    && api > 0 && worker > 0 {
                    println!("generated sessions: API pool={api}, worker pool={worker}, API LISTEN pid={pid}; ssl={}", self.tls.is_some());
                    return pid;
                }
                sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("actual generated API/worker/listener identity proof")
    }
}

fn redacted(text: &str) {
    for secret in [
        "startup-test",
        "password=postgres",
        "postgres://",
        "postgresql://",
    ] {
        assert!(
            !text.contains(secret),
            "credential-bearing diagnostics withheld"
        );
    }
}
struct Process {
    child: Child,
    log: PathBuf,
    insecure: bool,
}
impl Process {
    fn spawn(mut command: Command, label: &str) -> Self {
        let log = std::env::temp_dir().join(format!(
            "blkit-deployment-{}-{label}.log",
            std::process::id()
        ));
        let file = fs::File::create(&log).unwrap();
        command
            .stdout(Stdio::from(file.try_clone().unwrap()))
            .stderr(Stdio::from(file));
        let insecure = command.get_envs().any(|(name, value)| {
            name == "BLKIT_POSTGRES_INSECURE_DEV" && value.is_some_and(|value| value == "true")
        });
        let child = command.spawn().unwrap();
        Self {
            child,
            log,
            insecure,
        }
    }
    fn stop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
        }
        self.child.wait().unwrap();
        let text = fs::read_to_string(&self.log).unwrap();
        redacted(&text);
        if self.insecure {
            assert!(
                text.contains("postgres transport is insecure plaintext"),
                "explicit plaintext must warn visibly: {}",
                self.log.display()
            );
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn curl(
    port: u16,
    method: &str,
    path: &str,
    fields: &[String],
    body: Option<Value>,
) -> (u16, Value) {
    let mut command = Command::new("curl");
    command.args([
        "-sS",
        "--max-time",
        "5",
        "-w",
        "\n%{http_code}",
        "-X",
        method,
    ]);
    for field in fields {
        command.args(["-F", field]);
    }
    if let Some(body) = body {
        command.args([
            "-H",
            "Content-Type: application/json",
            "-d",
            &body.to_string(),
        ]);
    }
    let output = command
        .arg(format!("http://127.0.0.1:{port}{path}"))
        .output()
        .unwrap();
    assert!(output.status.success(), "HTTP transport failed");
    let text = String::from_utf8(output.stdout).unwrap();
    let (body, code) = text.rsplit_once('\n').unwrap();
    (
        code.parse().unwrap(),
        serde_json::from_str(body).unwrap_or(json!(body)),
    )
}
fn get(port: u16, id: &str) -> Value {
    let (code, value) = curl(port, "GET", &format!("/instances/{id}"), &[], None);
    assert_eq!(code, 200, "{value}");
    value
}
async fn state(port: u16, id: &str, expected: &str) -> Value {
    timeout(Duration::from_secs(15), async {
        loop {
            let row = get(port, id);
            if row["status"] == expected {
                return row;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("instance {id} did not reach {expected}: {}", get(port, id)))
}
fn admit(port: u16, process: &str, input: Value) -> String {
    let (code, body) = curl(
        port,
        "POST",
        &format!("/processes/orders/1/{process}/instances"),
        &[],
        Some(input),
    );
    assert_eq!(code, 202, "{body}");
    body["id"].as_str().unwrap().to_owned()
}
async fn file_contains(path: &Path, needle: &str) {
    timeout(Duration::from_secs(15), async {
        loop {
            if fs::read_to_string(path)
                .unwrap_or_default()
                .contains(needle)
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("fixture file {} missing {needle}", path.display()));
}
fn instrument(project: &Generated) {
    let lib = project.root.join(".blkit/src/lib.rs");
    let source = fs::read_to_string(&lib).unwrap();
    assert_eq!(
        source.matches("pub fn named_graph_definitions()").count(),
        1
    );
    fs::write(
        lib,
        format!(
            "{}\n{TASKS}",
            source.replacen(
                "pub fn named_graph_definitions()",
                "fn compiled_graph_definitions()",
                1
            )
        ),
    )
    .unwrap();
    fs::write(
        project.root.join(format!(
            ".blkit/src/bin/{}-attachment-inspect.rs",
            project.name
        )),
        remote_attachment_reader::SOURCE,
    )
    .unwrap();
}

struct Topology {
    api: Option<Generated>,
    worker: Generated,
    binaries: PathBuf,
    api_binaries: PathBuf,
}
impl Topology {
    fn build(target: &'static str) -> Self {
        let worker = Generated::with_source(target, SOURCE);
        instrument(&worker);
        let binaries = worker.build(false);
        let (api, api_binaries) = if target == "worker-only" {
            let api = Generated::new("api-only");
            let api_binaries = api.build(false);
            (Some(api), api_binaries)
        } else if target == "api-worker-split" {
            (None, worker.build(true))
        } else {
            (None, binaries.clone())
        };
        Self {
            api,
            worker,
            binaries,
            api_binaries,
        }
    }
    fn worker_binary(&self) -> PathBuf {
        self.binaries.join(format!(
            "{}-{}",
            self.worker.name,
            if self.worker.build_target == "api-worker" {
                "api-worker"
            } else {
                "worker"
            }
        ))
    }
    fn api_binary(&self) -> PathBuf {
        self.api_binaries.join(format!(
            "{}-api",
            self.api.as_ref().unwrap_or(&self.worker).name
        ))
    }
    fn launch_worker(
        &self,
        db: &Database,
        root: &Path,
        cloud: &str,
        port: u16,
        label: &str,
    ) -> Process {
        let mut command = Command::new(self.worker_binary());
        seaweed::configure(&mut command, cloud);
        let combined = self.worker.build_target == "api-worker";
        db.runtime(
            &mut command,
            root,
            if combined {
                &["api", "worker"]
            } else {
                &["worker"]
            },
        );
        command.args(["2", "1000"]);
        if combined {
            command.arg(format!("127.0.0.1:{port}"));
        }
        let process = Process::spawn(command, label);
        managed_runtime::assert_child_configuration(
            &process.child,
            if combined {
                &["api", "worker"]
            } else {
                &["worker"]
            },
        );
        process
    }
    fn launch_api(
        &self,
        db: &Database,
        root: &Path,
        cloud: &str,
        port: u16,
        label: &str,
    ) -> Process {
        let mut command = Command::new(self.api_binary());
        seaweed::configure(&mut command, cloud);
        db.runtime(&mut command, root, &["api"]);
        command.arg(format!("127.0.0.1:{port}"));
        let process = Process::spawn(command, label);
        managed_runtime::assert_child_configuration(&process.child, &["api"]);
        process
    }
    fn inspect(&self, db: &Database, root: &Path, cloud: &str, id: &str, scope: &str) -> Value {
        let mut command = Command::new(
            self.binaries
                .join(format!("{}-attachment-inspect", self.worker.name)),
        );
        seaweed::configure(&mut command, cloud);
        managed_runtime::configure(&mut command, "", &[]);
        db.transport(&mut command);
        // Existing reader resolves environment configuration (not executable arguments).
        command
            .env("BLKIT_WORKER_DATABASE_URL_FILE", root.join("worker.url"))
            .args([id, scope]);
        let output = command.output().unwrap();
        redacted(&String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "worker attachment reader failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn same_target_generated_instances_keep_independent_roots_on_drop() {
    let first = Generated::new("worker-only");
    let second = Generated::new("worker-only");
    let first_root = first.root.clone();
    let second_root = second.root.clone();
    if first_root == second_root {
        // Clean both owners before asserting so a second Drop cannot mask the collision.
        drop(second);
        fs::create_dir(&first_root).unwrap();
        drop(first);
        assert_ne!(
            first_root, second_root,
            "same-target generated roots collide"
        );
        return;
    }
    assert_ne!(first.name, second.name);
    let manifest = second.root.join(".blkit/Cargo.toml");
    let original = fs::read(&manifest).unwrap();
    let marker = second.root.join("owned-by-second");
    fs::write(&marker, b"second instance").unwrap();
    drop(first);
    assert!(!first_root.exists());
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(fs::read(&marker).unwrap(), b"second instance");
    drop(second);
    assert!(!second_root.exists());
}

#[test]
fn standalone_api_binary_uses_independent_package_target_directory() {
    let worker = Generated::new("worker-only");
    let api = Generated::new("api-only");
    let api_binaries = api.root.join(".blkit/target/debug");
    let expected = api_binaries.join(format!("{}-api", api.name));
    let topology = Topology {
        binaries: worker.root.join(".blkit/target/debug"),
        worker,
        api: Some(api),
        api_binaries,
    };
    assert_eq!(topology.api_binary(), expected);
}

#[test]
fn split_api_binary_uses_member_build_directory() {
    let worker = Generated::new("api-worker-split");
    let api_binaries = worker.root.join(".blkit/api/target/debug");
    let expected = api_binaries.join(format!("{}-api", worker.name));
    let topology = Topology {
        binaries: worker.root.join(".blkit/target/debug"),
        worker,
        api: None,
        api_binaries,
    };
    assert_eq!(topology.api_binary(), expected);
}

#[test]
fn api_and_worker_binaries_use_shared_build_directory() {
    let worker = Generated::new("worker-only");
    let api = Generated::new("api-only");
    let shared = PathBuf::from("/synthetic/shared-target/debug");
    let expected_api = shared.join(format!("{}-api", api.name));
    let expected_worker = shared.join(format!("{}-worker", worker.name));
    let topology = Topology {
        binaries: shared.clone(),
        api_binaries: shared,
        worker,
        api: Some(api),
    };
    assert_eq!(topology.api_binary(), expected_api);
    assert_eq!(topology.worker_binary(), expected_worker);
}

async fn ready(process: &mut Process, port: u16) {
    timeout(Duration::from_secs(15), async {
        loop {
            assert!(
                process.child.try_wait().unwrap().is_none(),
                "startup exited; log {}",
                process.log.display()
            );
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("generated API did not become ready");
}

async fn worker_ready(db: &Database, process: &mut Process) {
    let id = format!("worker-{}", process.child.id());
    timeout(Duration::from_secs(15), async {
        loop {
            assert!(process.child.try_wait().unwrap().is_none(), "worker startup exited; log {}", process.log.display());
            let count: i64 = db.admin().query_one(
                "SELECT count(*) FROM worker_capabilities c JOIN workers w ON w.id=c.worker_id
                 WHERE w.id=$1 AND NOT w.draining
                   AND w.heartbeat_at >= (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT-30000
                   AND c.namespace='orders' AND c.version='1' AND c.process IN ('route','work','limited')",
                &[&id],
            ).await.unwrap().get(0);
            if count == 3 { break; }
            sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("current generated worker must advertise all fixture capabilities before admission");
}

async fn scenario(topology: &Topology, db: &Database, cloud: &str) {
    let label = format!(
        "{}-{}",
        topology.worker.build_target,
        if db.tls.is_some() { "tls" } else { "insecure" }
    );
    let root = topology.worker.root.join(&label);
    fs::create_dir(&root).unwrap();
    println!("BEGIN {label}");
    let port = port();
    let combined = topology.worker.build_target == "api-worker";
    let mut worker = topology.launch_worker(db, &root, cloud, port, &format!("{label}-owner"));
    let mut api = if combined {
        None
    } else {
        Some(topology.launch_api(db, &root, cloud, port, &format!("{label}-api")))
    };
    ready(api.as_mut().unwrap_or(&mut worker), port).await;
    worker_ready(db, &mut worker).await;
    let ok = admit(port, "work", json!({"mode":"ok"}));
    assert_eq!(state(port, &ok, "completed").await["result"], "ok");

    // Actual multipart admission and worker request publication; no Store::create under API.
    let start_secret = "deployment-start-bytes";
    let reply_secret = "deployment-reply-bytes";
    let start_file = root.join("start.txt");
    let reply_file = root.join("reply.txt");
    fs::write(&start_file, start_secret).unwrap();
    fs::write(&reply_file, reply_secret).unwrap();
    let (code, accepted) = curl(
        port,
        "POST",
        "/processes/orders/1/route/instances",
        &[
            "input={\"amount\":\"3\"};type=application/json".into(),
            format!(
                "files=@{};filename=start.txt;type=text/plain",
                start_file.display()
            ),
        ],
        None,
    );
    assert_eq!(code, 202, "{accepted}");
    let id = accepted["id"].as_str().unwrap();
    let waiting = state(port, id, "waiting-for-input").await;
    let request = waiting["requests"][0]["id"].as_str().unwrap();
    assert_eq!(waiting["input"], json!({"amount":"3"}));
    assert_eq!(
        topology.inspect(db, &root, cloud, id, "-"),
        json!([
            {"ordinal":0,"filename":"start.txt","media_type":"text/plain","byte_length":22,"bytes":start_secret.as_bytes()}
        ])
    );
    let events_file = std::env::temp_dir().join(format!(
        "blkit-deployment-{}-{label}-events.log",
        std::process::id()
    ));
    let mut command = Command::new("curl");
    command
        .args(["-sN", "--max-time", "120"])
        .arg(format!("http://127.0.0.1:{port}/instances/{id}/events"));
    command
        .stdout(Stdio::from(fs::File::create(&events_file).unwrap()))
        .stderr(Stdio::null());
    let events = Process {
        child: command.spawn().unwrap(),
        log: events_file.clone(),
        insecure: false,
    };
    file_contains(&events_file, "waiting-for-input").await;
    let listener = db.identities(None).await;
    assert!(
        db.admin()
            .query_one("SELECT pg_terminate_backend($1)", &[&listener])
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let replacement = db.identities(Some(listener)).await;
    assert_ne!(listener, replacement);
    // Same open stream receives the listener resync, then client reconnect gets a fresh snapshot.
    timeout(Duration::from_secs(10), async {
        loop {
            if fs::read_to_string(&events_file)
                .unwrap()
                .matches("waiting-for-input")
                .count()
                >= 2
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("SSE stream did not resync after listener reconnect");
    let changed = fs::read_to_string(&events_file)
        .unwrap()
        .matches("event: changed")
        .count();
    let (code, reply) = curl(
        port,
        "POST",
        &format!("/instances/{id}/requests/{request}/response"),
        &[
            "response={\"approved\":true};type=application/json".into(),
            format!(
                "files=@{};filename=reply.txt;type=text/plain",
                reply_file.display()
            ),
        ],
        None,
    );
    assert_eq!(code, 202, "{reply}");
    assert_eq!(state(port, id, "completed").await["result"], true);
    timeout(Duration::from_secs(10), async {
        loop {
            let text = fs::read_to_string(&events_file).unwrap();
            let hints: Vec<Value> = text
                .split("event: changed\n")
                .skip(1)
                .filter_map(|event| event.lines().find_map(|line| line.strip_prefix("data: ")))
                .map(|data| serde_json::from_str(data).unwrap())
                .collect();
            assert!(hints.iter().all(|hint| hint["id"] == id));
            if hints.len() > changed {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("watched reply must produce a new SSE changed hint");
    drop(events);
    let reconnect_file = std::env::temp_dir().join(format!(
        "blkit-deployment-{}-{label}-reconnect.log",
        std::process::id()
    ));
    let mut command = Command::new("curl");
    command
        .args(["-sN", "--max-time", "120"])
        .arg(format!("http://127.0.0.1:{port}/instances/{id}/events"));
    command
        .stdout(Stdio::from(fs::File::create(&reconnect_file).unwrap()))
        .stderr(Stdio::null());
    let reconnect = Process {
        child: command.spawn().unwrap(),
        log: reconnect_file.clone(),
        insecure: false,
    };
    file_contains(&reconnect_file, "completed").await;
    let text = fs::read_to_string(&reconnect_file).unwrap();
    assert_eq!(
        text.lines().find(|l| l.starts_with("event: ")),
        Some("event: snapshot")
    );
    let snapshot: Value = serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot["id"], id);
    assert_eq!(snapshot["status"], "completed");
    assert_eq!(snapshot["result"], true);
    assert_eq!(
        topology.inspect(db, &root, cloud, id, request),
        json!([
            {"ordinal":0,"filename":"reply.txt","media_type":"text/plain","byte_length":22,"bytes":reply_secret.as_bytes()}
        ])
    );
    for text in [
        waiting.to_string(),
        get(port, id).to_string(),
        fs::read_to_string(&events_file).unwrap(),
        text,
    ] {
        assert!(
            !text.contains(start_secret)
                && !text.contains(reply_secret)
                && !text.contains("blkit/attachments/")
        );
    }
    drop(reconnect);
    println!(
        "{label}: admission/execution, multipart request/reply byte reads, SSE listener + client reconnect passed"
    );

    let retry = admit(port, "work", json!({"mode":"retry"}));
    assert_eq!(state(port, &retry, "completed").await["result"], "retry");
    let retry_row = db
        .admin()
        .query_one("SELECT attempt FROM instances WHERE id=$1", &[&retry])
        .await
        .unwrap();
    assert_eq!(retry_row.get::<_, i64>(0), 2);
    assert_eq!(
        fs::read_to_string(root.join("retry-before.log"))
            .unwrap()
            .lines()
            .count(),
        1,
        "committed checkpoint must not replay on retry"
    );
    assert_eq!(
        fs::read_to_string(root.join("retry-action.log"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    for (mode, process, terminal) in [
        ("cancel", "work", "cancelled"),
        ("deadline", "limited", "business-error"),
    ] {
        let id = admit(port, process, json!({"mode":mode}));
        file_contains(&root.join(format!("{mode}-action.log")), "\n").await;
        if mode == "cancel" {
            let (code, body) = curl(port, "POST", &format!("/instances/{id}/cancel"), &[], None);
            assert!([200, 202, 204].contains(&code), "{body}");
        }
        state(port, &id, terminal).await;
        let return_marker = root.join(format!("{mode}-action-return.log"));
        assert!(
            !return_marker.exists(),
            "blocked action must not reach its return boundary before release"
        );
        fs::write(root.join(format!("{mode}-release")), "").unwrap();
        file_contains(&return_marker, "ready-to-return\n").await;
        println!(
            "{label}: {mode} action reached fixture return boundary (result prepared; not a runtime publication barrier)"
        );
        assert_eq!(get(port, &id)["status"], terminal);
        let row = db
            .admin()
            .query_one(
                "SELECT attempt, result, terminal_name FROM instances WHERE id=$1",
                &[&id],
            )
            .await
            .unwrap();
        assert_eq!(row.get::<_, i64>(0), 1, "cancel/deadline must not retry");
        assert!(
            row.get::<_, Option<String>>(1).is_none(),
            "late task must not commit result"
        );
        if mode == "deadline" {
            assert_eq!(row.get::<_, Option<String>>(2).as_deref(), Some("timeout"));
        }
    }
    println!(
        "{label}: checkpoint retry, cancellation and queued deadline fence late results passed"
    );

    let lost = admit(port, "work", json!({"mode":"loss"}));
    file_contains(&root.join("loss-action.log"), "\n").await;
    let options = db.settings.connection_options().unwrap();
    let worker_store = db
        .settings
        .resolve_role(DatabaseRole::Worker, &options)
        .unwrap()
        .connect_store(4)
        .await
        .unwrap();
    let old = worker_store.get(&lost).await.unwrap().unwrap();
    assert!(
        old.instance
            .checkpoint
            .as_ref()
            .unwrap()
            .completed
            .contains_key("before")
    );
    let owner = old.owner_id.unwrap();
    worker.stop();
    worker = topology.launch_worker(db, &root, cloud, port, &format!("{label}-survivor"));
    if combined {
        ready(&mut worker, port).await;
    }
    file_contains(
        &root.join("loss-action.log"),
        &format!("{}\n", worker.child.id()),
    )
    .await;
    let recovered = worker_store.get(&lost).await.unwrap().unwrap();
    assert_eq!(recovered.instance.status, "running");
    assert_eq!(recovered.generation, old.generation + 1);
    assert_eq!(recovered.instance.attempt, 2);
    assert_eq!(
        fs::read_to_string(root.join("loss-before.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(root.join("loss-action.log"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert!(
        !worker_store
            .commit_checkpoint(&lost, &owner, old.generation, &GraphCheckpoint::default())
            .await
            .unwrap()
    );
    assert!(
        !worker_store
            .finish_owned(
                &lost,
                &owner,
                old.generation,
                "completed",
                Some(json!("stale")),
                None,
                None
            )
            .await
            .unwrap()
    );
    assert_eq!(
        get(port, &lost)["status"],
        "running",
        "stale owner must not finish the live replacement claim"
    );
    fs::write(root.join("loss-release"), "").unwrap();
    assert_eq!(state(port, &lost, "completed").await["result"], "loss");
    drop(worker_store);
    println!(
        "{label}: killed owner, surviving generated worker resumes checkpoint exactly once; old generation writes rejected"
    );

    worker.stop();
    if let Some(api) = &mut api {
        api.stop();
    }
    timeout(Duration::from_secs(10), async {
        loop {
            db.admin().simple_query("SELECT pg_stat_clear_snapshot()").await.unwrap();
            let count: i64 = db.admin().query_one("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND usename IN ('blkit_api','blkit_worker')", &[]).await.unwrap().get(0);
            if count == 0 { break; }
            sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("all runtime-role sessions must close before CLI reruns");
    println!("{label}: exact zero managed runtime sessions before reruns");
    let snapshot_sql = "SELECT row_to_json(t)::text FROM (
        SELECT 'instances' AS kind, row_to_json(i)::text AS value FROM instances i
        UNION ALL SELECT 'requests', row_to_json(r)::text FROM input_requests r
        UNION ALL SELECT 'attachments', row_to_json(a)::text FROM instance_attachments a
        UNION ALL SELECT 'interests', row_to_json(i)::text FROM instance_interests i
        UNION ALL SELECT 'workers', row_to_json(w)::text FROM workers w
        UNION ALL SELECT 'capabilities', row_to_json(c)::text FROM worker_capabilities c
        UNION ALL SELECT 'policies', row_to_json(p)::text FROM process_policies p
        UNION ALL SELECT 'deployment', row_to_json(d)::text FROM blkit_deployment d
        UNION ALL SELECT 'history', row_to_json(h)::text FROM blkit_schema_migrations h
        UNION ALL SELECT 'roles', rolname || ':' || rolpassword FROM pg_authid WHERE rolname IN ('blkit_api','blkit_worker')
        ) t ORDER BY kind, value";
    let before: Vec<String> = db
        .admin()
        .query(snapshot_sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    db.cli("init", true);
    db.cli("migrate", false);
    db.cli("migrate", false);
    let after: Vec<String> = db
        .admin()
        .query(snapshot_sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert!(
        before == after,
        "quiesced CLI reruns must preserve data, history and passwords"
    );
    worker = topology.launch_worker(db, &root, cloud, port, &format!("{label}-post-migrate"));
    if combined {
        ready(&mut worker, port).await;
    } else {
        api =
            Some(topology.launch_api(db, &root, cloud, port, &format!("{label}-post-migrate-api")));
        ready(api.as_mut().unwrap(), port).await;
    }
    worker_ready(db, &mut worker).await;
    assert_eq!(get(port, &lost)["result"], "loss");
    assert_eq!(
        topology.inspect(db, &root, cloud, id, request)[0]["bytes"],
        json!(reply_secret.as_bytes())
    );
    worker.stop();
    if let Some(api) = &mut api {
        api.stop();
    }
    if db.tls.is_none() {
        assert!(
            fs::read_to_string(&worker.log)
                .unwrap()
                .contains("insecure"),
            "explicit plaintext must warn visibly"
        );
    }
    println!(
        "PASS {label}: quiesced init + migrate + migrate preserve application rows, requests/replies, attachment bytes, passwords and exact history; managed restart succeeds"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_deployments_cover_all_behaviors_with_managed_logins_over_both_transports() {
    let (_cloud, cloud) = seaweed::start().await;
    for target in ["worker-only", "api-worker-split", "api-worker"] {
        let topology = Topology::build(target);
        for secure in [true, false] {
            let db = Database::start(secure).await;
            scenario(&topology, &db, &cloud).await;
        }
    }
}
