//! Catch backend feature leakage and loss of generated local initialization/reopen behavior.
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

use blkit_transpiler::project::Project;
use serde_json::{Value, json};

fn cargo(manifest: &Path, args: &[&str], label: &str) -> String {
    let output = Command::new("cargo")
        .args(args)
        .arg("--offline")
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .unwrap();
    let log = std::env::temp_dir().join(format!(
        "blkit-local-boundaries-{}-{label}.log",
        std::process::id()
    ));
    fs::write(&log, [&output.stdout[..], &output.stderr[..]].concat()).unwrap();
    assert!(output.status.success(), "cargo {args:?}: {}", log.display());
    println!("cargo {args:?}: {}", log.display());
    String::from_utf8(output.stdout).unwrap()
}

fn has_package(tree: &str, name: &str) -> bool {
    tree.lines()
        .any(|line| line.split_whitespace().next() == Some(name))
}

#[test]
fn core_backend_features_are_independent_and_additive() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../blkit-core/Cargo.toml");
    for (label, features, local, remote) in [
        ("default", "", false, false),
        ("none", "", false, false),
        ("local", "local-persistence", true, false),
        ("remote", "remote-persistence", false, true),
        ("both", "local-persistence,remote-persistence", true, true),
    ] {
        let mut args = vec!["build", "--lib"];
        if label != "default" {
            args.push("--no-default-features");
        }
        if !features.is_empty() {
            args.extend(["--features", features]);
        }
        cargo(&manifest, &args, &format!("core-{label}-build"));
        let mut args = vec!["tree", "--edges", "normal", "--prefix", "none"];
        if label != "default" {
            args.push("--no-default-features");
        }
        if !features.is_empty() {
            args.extend(["--features", features]);
        }
        let tree = cargo(&manifest, &args, &format!("core-{label}-tree"));
        assert_eq!(has_package(&tree, "turso"), local, "{label}");
        for package in [
            "deadpool-postgres",
            "tokio-postgres",
            "tokio-postgres-rustls",
            "rustls-native-certs",
            "dotenv-ng",
        ] {
            assert_eq!(has_package(&tree, package), remote, "{label}: {package}");
        }
        for package in [
            "blkit-cli",
            "blkit-transpiler",
            "axum",
            "object_store",
            "opentelemetry-otlp",
        ] {
            assert!(!has_package(&tree, package), "{label}: {package}");
        }
        if !remote {
            assert!(!has_package(&tree, "rustls"), "{label}");
        }
    }
}

struct GeneratedLocal {
    root: PathBuf,
    name: String,
}

impl GeneratedLocal {
    fn new(target: &str) -> Self {
        let name = format!("local_{}_{}", target.replace('-', "_"), std::process::id());
        let root = std::env::temp_dir().join(&name);
        fs::create_dir(&root).unwrap();
        let generated = Self { root, name };
        fs::write(
            generated.root.join("blkit.toml"),
            format!(
                "[project]\nname = {:?}\nblkit = \"0.1.0\"\nbuild_target = {target:?}\n{}",
                generated.name,
                if target == "api-worker" {
                    "persistence = \"local\"\n"
                } else {
                    ""
                },
            ),
        )
        .unwrap();
        fs::write(generated.root.join("route.bl"), "namespace orders; version \"1\"; start_event start { output input: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.input -> done.result; }").unwrap();
        Project::load(&generated.root).unwrap().transpile().unwrap();
        generated
    }

    fn build(&self, local: bool) -> PathBuf {
        let manifest = self.root.join(".blkit/Cargo.toml");
        cargo(&manifest, &["build"], &format!("{}-build", self.name));
        let tree = cargo(
            &manifest,
            &["tree", "--edges", "normal", "--prefix", "none"],
            &format!("{}-tree", self.name),
        );
        for package in [
            "blkit-cli",
            "blkit-transpiler",
            "chumsky",
            "logos",
            "deadpool-postgres",
            "tokio-postgres",
            "tokio-postgres-rustls",
            "rustls-native-certs",
            "dotenv-ng",
        ] {
            assert!(!has_package(&tree, package), "{}: {package}", self.name);
        }
        assert_eq!(has_package(&tree, "turso"), local);
        assert_eq!(has_package(&tree, "axum"), local);
        if !local {
            assert!(!has_package(&tree, "rustls"));
            assert!(!has_package(&tree, "object_store"));
        }
        std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.root.join(".blkit/target"))
            .join("debug")
    }
}

impl Drop for GeneratedLocal {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Process(Child);

impl Process {
    fn start(project: &GeneratedLocal, binary: &Path, bind: &str, phase: &str) -> Self {
        let log = std::env::temp_dir().join(format!("{}-{phase}.log", project.name));
        let output = fs::File::create(&log).unwrap();
        let process = Self(
            Command::new(binary)
                .current_dir(&project.root)
                .env_clear()
                .env("TOKIO_WORKER_THREADS", "2")
                .env("BLKIT_API_DB_POOL_MAX", "bad")
                .env("BLKIT_WORKER_DB_POOL_MAX", "0")
                .env("BLKIT_POSTGRES_INSECURE_DEV", "invalid")
                .env("BLKIT_POSTGRES_CA_FILE", "missing-ca.pem")
                .env("BLKIT_ADMIN_DATABASE_URL", "invalid-unused-admin")
                .env("BLKIT_API_DATABASE_URL", "invalid-unused-api")
                .env("BLKIT_WORKER_DATABASE_URL_FILE", "missing-unused-worker")
                .arg(project.root.join("local.db"))
                .args(["2", bind])
                .stdout(Stdio::from(output.try_clone().unwrap()))
                .stderr(Stdio::from(output))
                .spawn()
                .unwrap(),
        );
        println!("local {phase}: {}", log.display());
        process
    }

    fn wait_ready(&mut self, bind: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                self.0.try_wait().unwrap().is_none(),
                "local process exited before serving"
            );
            if TcpStream::connect(bind).is_ok() {
                return;
            }
            assert!(Instant::now() < deadline, "local startup deadline");
            sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(bind: &str, method: &str, path: &str, body: &str) -> Value {
    let mut stream = TcpStream::connect(bind).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {bind}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    let (headers, body) = text.split_once("\r\n\r\n").unwrap();
    assert!(
        matches!(headers.split_whitespace().nth(1), Some("200" | "202")),
        "{text}"
    );
    serde_json::from_str(body).unwrap()
}

fn completed(bind: &str, id: &str, expected: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let row = request(bind, "GET", &format!("/instances/{id}"), "");
        if row["status"] == "completed" {
            assert_eq!(row["result"], expected);
            return row;
        }
        assert!(
            matches!(row["status"].as_str(), Some("pending" | "running")),
            "{row}"
        );
        assert!(Instant::now() < deadline, "local execution deadline: {row}");
        sleep(Duration::from_millis(20));
    }
}

#[test]
fn initialized_project_builds_and_completes_local_echo_without_postgres() {
    let name = format!("local-init-echo-{}", std::process::id());
    let root = std::env::temp_dir().join(&name);
    fs::create_dir(&root).unwrap();
    let project = GeneratedLocal { root, name };
    let initialized = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .current_dir(&project.root)
        .args(["init", "--no-vcs"])
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{initialized:?}");
    assert!(project.root.join("blkit.toml").is_file());
    assert!(project.root.join("main.bl").is_file());
    for path in [".git", "Cargo.toml", "Cargo.lock", ".blkit", "local.db"] {
        assert!(!project.root.join(path).exists(), "unexpected {path}");
    }

    let transpiled = Command::new(env!("CARGO_BIN_EXE_blkit"))
        .current_dir(&project.root)
        .env("PATH", "")
        .arg("transpile")
        .output()
        .unwrap();
    assert!(transpiled.status.success(), "{transpiled:?}");
    assert!(project.root.join(".blkit/Cargo.toml").is_file());
    assert!(!project.root.join("local.db").exists());
    let binaries = project.build(true);
    let binary = binaries.join(format!("{}-api-worker", project.name));
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = socket.local_addr().unwrap().to_string();
    drop(socket);
    let mut process = Process::start(&project, &binary, &bind, "initialized");
    process.wait_ready(&bind);
    assert!(project.root.join("local.db").is_file());
    let admitted = request(
        &bind,
        "POST",
        "/processes/example/1/echo/instances",
        r#"{"input":"42"}"#,
    );
    let row = completed(&bind, admitted["id"].as_str().unwrap(), "42");
    assert_eq!(row["input"], json!({"input": "42"}));
    println!("initialized project: local example/1/echo completed with input and result 42");
}

#[test]
fn generated_reusable_crate_builds_without_backend_or_administration_dependencies() {
    let project = GeneratedLocal::new("crate");
    project.build(false);
}

#[test]
fn generated_local_server_and_api_worker_initialize_and_reopen_without_postgres() {
    for target in ["server", "api-worker"] {
        let project = GeneratedLocal::new(target);
        let binaries = project.build(true);
        let binary = binaries.join(format!("{}-{target}", project.name));
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let bind = socket.local_addr().unwrap().to_string();
        drop(socket);
        let database = project.root.join("local.db");
        assert!(!database.exists());
        let mut process = Process::start(&project, &binary, &bind, "new");
        process.wait_ready(&bind);
        assert!(database.is_file());
        let admitted = request(
            &bind,
            "POST",
            "/processes/orders/1/route/instances",
            r#"{"input":"5"}"#,
        );
        let id = admitted["id"].as_str().unwrap();
        let original = completed(&bind, id, "5");
        drop(process);
        let mut process = Process::start(&project, &binary, &bind, "reopen");
        process.wait_ready(&bind);
        let reopened = completed(&bind, id, "5");
        assert_eq!(reopened["input"], original["input"]);
        assert_eq!(reopened["id"], json!(id));
        let admitted = request(
            &bind,
            "POST",
            "/processes/orders/1/route/instances",
            r#"{"input":"9"}"#,
        );
        completed(&bind, admitted["id"].as_str().unwrap(), "9");
        println!("{target}: new database completed 5; reopened retained {id} and completed 9");
        drop(process);
    }
}
