use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn remote_harness_roles_reject_missing_or_filesystem_cloud_store() {
    for binary in [
        env!("CARGO_BIN_EXE_blkit-api"),
        env!("CARGO_BIN_EXE_blkit-worker"),
    ] {
        for backend in [None, Some("fs")] {
            let mut command = Command::new(binary);
            command
                .arg("postgres://127.0.0.1:1/blkit")
                .env_remove("BLKIT_ATTACHMENT_BACKEND");
            if let Some(backend) = backend {
                command.env("BLKIT_ATTACHMENT_BACKEND", backend);
            }
            let output = command.output().unwrap();
            assert!(
                !output.status.success(),
                "{binary} started with {backend:?}"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("BLKIT_ATTACHMENT_BACKEND"),
                "{binary}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn local_harness_creates_writable_persistent_attachment_directory() {
    let root =
        std::env::temp_dir().join(format!("blkit-harness-attachment-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_blkit-dev"))
        .arg(root.join("instance.db"))
        .args(["2", "127.0.0.1:0"])
        .env_remove("BLKIT_ATTACHMENT_ROOT")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !root.join("attachments").exists() && start.elapsed() < Duration::from_secs(10) {
        assert!(
            child.try_wait().unwrap().is_none(),
            "local harness exited before attachment initialization"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let file = root.join("attachments/probe");
    assert!(Path::new(&root.join("attachments")).exists());
    fs::write(&file, b"writable").unwrap();
    assert_eq!(fs::read(file).unwrap(), b"writable");
    child.kill().unwrap();
    child.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
}
