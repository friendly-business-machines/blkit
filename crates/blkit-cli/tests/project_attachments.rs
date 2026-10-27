use blkit_transpiler::project::Project;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const SOURCE: &str = "namespace orders; version \"1\";\n";

fn generated(target: &str, persistence: &str) -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blkit-attachment-project-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("blkit.toml"), format!("[project]\nname = \"attachments_fixture_{suffix}\"\nblkit = \"0.1.0\"\nbuild_target = {target:?}\n{persistence}")).unwrap();
    if target != "api-only" {
        fs::write(root.join("route.bl"), SOURCE).unwrap();
    }
    Project::load(&root).unwrap().transpile().unwrap();
    root
}

fn features(manifest: &Path) -> Vec<String> {
    let content = fs::read_to_string(manifest).unwrap();
    let line = content
        .lines()
        .find(|line| line.starts_with("blkit-core = "))
        .unwrap();
    let features = line
        .split("features = [")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap();
    if features.is_empty() {
        return vec![];
    }
    features
        .split(", ")
        .map(|feature| feature.trim_matches('"').to_owned())
        .collect()
}

#[test]
fn generated_attachment_features_are_role_specific() {
    for (target, persistence, expected) in [
        ("crate", "", vec![]),
        (
            "server",
            "",
            vec![
                "api-server",
                "worker",
                "local-persistence",
                "logging",
                "attachment-local",
            ],
        ),
        (
            "api-worker",
            "persistence = \"local\"\n",
            vec![
                "api-server",
                "worker",
                "local-persistence",
                "logging",
                "attachment-local",
            ],
        ),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            vec![
                "api-server",
                "worker",
                "remote-persistence",
                "logging",
                "attachment-cloud",
            ],
        ),
        (
            "api-only",
            "",
            vec!["api-server", "remote-persistence", "attachment-cloud"],
        ),
        (
            "worker",
            "",
            vec![
                "worker",
                "remote-persistence",
                "logging",
                "attachment-cloud",
            ],
        ),
        (
            "worker-only",
            "",
            vec![
                "worker",
                "remote-persistence",
                "logging",
                "attachment-cloud",
            ],
        ),
        (
            "api-worker-split",
            "",
            vec![
                "worker",
                "remote-persistence",
                "logging",
                "attachment-cloud",
            ],
        ),
    ] {
        let root = generated(target, persistence);
        assert_eq!(
            features(&root.join(".blkit/Cargo.toml")),
            expected,
            "{target} {persistence}"
        );
        if target == "api-worker-split" {
            assert_eq!(
                features(&root.join(".blkit/api/Cargo.toml")),
                vec!["api-server", "remote-persistence", "attachment-cloud"]
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

fn build(root: &Path) -> PathBuf {
    let manifest = root.join(".blkit/Cargo.toml");
    let output = Command::new("cargo")
        .args(["build", "--offline", "--manifest-path"])
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if root.join(".blkit/api/Cargo.toml").exists() {
        let output = Command::new("cargo")
            .args(["build", "--offline", "--manifest-path"])
            .arg(root.join(".blkit/api/Cargo.toml"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or(root.join(".blkit/target"))
        .join("debug")
}

fn binary_name(root: &Path, role: &str) -> String {
    format!(
        "attachments_fixture_{}-{role}",
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .rsplit('-')
            .next()
            .unwrap()
    )
}

fn missing_cloud(binary: &Path, args: &[&str]) {
    let configure = |command: &mut Command| {
        command
            .env_clear()
            .env("TOKIO_WORKER_THREADS", "2")
            .env(
                "BLKIT_API_DATABASE_URL",
                "postgres://blkit_api:test@127.0.0.1:1/blkit",
            )
            .env(
                "BLKIT_WORKER_DATABASE_URL",
                "postgres://blkit_worker:test@127.0.0.1:1/blkit",
            );
    };
    let mut command = Command::new(binary);
    configure(&mut command);
    let output = command
        .args(args)
        .env_remove("BLKIT_ATTACHMENT_BACKEND")
        .env_remove("BLKIT_ATTACHMENT_BUCKET")
        .env_remove("BLKIT_ATTACHMENT_CONTAINER")
        .env_remove("BLKIT_LOG_OUTPUTS")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "{} unexpectedly started",
        binary.display()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("attachment initialization failed"),
        "{}: {stderr}",
        binary.display()
    );
    let mut command = Command::new(binary);
    configure(&mut command);
    let output = command
        .args(args)
        .env("BLKIT_ATTACHMENT_BACKEND", "fs")
        .env_remove("BLKIT_LOG_OUTPUTS")
        .output()
        .unwrap();
    assert!(
        !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("attachment initialization failed"),
        "{} accepted remote fs",
        binary.display()
    );
    let mut command = Command::new(binary);
    configure(&mut command);
    let output = command
        .args(args)
        .env("BLKIT_ATTACHMENT_BACKEND", "s3")
        .env_remove("BLKIT_ATTACHMENT_BUCKET")
        .env_remove("BLKIT_LOG_OUTPUTS")
        .output()
        .unwrap();
    assert!(
        !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("attachment initialization failed"),
        "{} accepted missing bucket",
        binary.display()
    );
}

#[test]
fn generated_remote_roles_reject_missing_cloud_before_postgres() {
    for (target, persistence, binaries) in [
        ("api-only", "", vec!["api"]),
        ("worker-only", "", vec!["worker"]),
        (
            "api-worker",
            "persistence = \"remote\"\n",
            vec!["api-worker"],
        ),
        ("api-worker-split", "", vec!["api", "worker"]),
    ] {
        let root = generated(target, persistence);
        let target_dir = build(&root);
        for role in binaries {
            let binary = target_dir.join(binary_name(&root, role));
            missing_cloud(&binary, &[]);
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn generated_local_default_attachment_directory_is_writable_before_serving() {
    for (target, persistence, role) in [
        ("server", "", "server"),
        ("api-worker", "persistence = \"local\"\n", "api-worker"),
    ] {
        let root = generated(target, persistence);
        let binary = build(&root).join(binary_name(&root, role));
        let db = root.join("instance.db");
        let mut child = Command::new(binary)
            .arg(&db)
            .args(["2", "127.0.0.1:0"])
            .env_remove("BLKIT_ATTACHMENT_ROOT")
            .env_remove("BLKIT_LOG_OUTPUTS")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !root.join("attachments").exists() && start.elapsed() < Duration::from_secs(10) {
            assert!(
                child.try_wait().unwrap().is_none(),
                "local binary exited before creating attachment directory"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            root.join("attachments").exists(),
            "no persistent attachment directory"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "local binary exited during startup"
        );
        let write = root.join("attachments/probe");
        fs::write(&write, b"writable").unwrap();
        assert_eq!(fs::read(&write).unwrap(), b"writable");
        child.kill().unwrap();
        child.wait().unwrap();
        let invalid_root = root.join("not-a-directory");
        fs::write(&invalid_root, b"occupied").unwrap();
        let output = Command::new(build(&root).join(binary_name(&root, role)))
            .arg(&db)
            .env("BLKIT_ATTACHMENT_ROOT", &invalid_root)
            .env_remove("BLKIT_LOG_OUTPUTS")
            .output()
            .unwrap();
        assert!(
            !output.status.success()
                && String::from_utf8_lossy(&output.stderr).contains("attachment directory"),
            "unusable local attachment root accepted: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
