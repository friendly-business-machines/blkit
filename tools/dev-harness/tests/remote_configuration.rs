//! Harness binaries must share generated roles' URL-free, fail-closed configuration boundary.
use std::{net::TcpListener, process::Command};

fn command(binary: &str) -> Command {
    let mut command = Command::new(binary);
    command.env_clear().env("TOKIO_WORKER_THREADS", "2");
    command
}

#[test]
fn remote_help_is_url_free_and_needs_no_configuration() {
    for (binary, operands) in [
        (env!("CARGO_BIN_EXE_blkit-api"), "[BIND_ADDRESS]"),
        (env!("CARGO_BIN_EXE_blkit-worker"), "[MAX_TASKS] [LEASE_MS]"),
    ] {
        let output = command(binary).arg("--help").output().unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("[--env-file PATH]"), "{help}");
        assert!(help.contains(operands), "{help}");
        assert!(!help.contains("POSTGRES_URL"), "{help}");
    }
}

#[test]
fn remote_configuration_errors_precede_network_and_redact_legacy_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let secret = "PRIVATE_HARNESS_CONFIG_SENTINEL";
    let url = format!(
        "postgres://user:{secret}@{}/db",
        listener.local_addr().unwrap()
    );
    for (binary, role, pool) in [
        (
            env!("CARGO_BIN_EXE_blkit-api"),
            "BLKIT_API_DATABASE_URL",
            "BLKIT_API_DB_POOL_MAX",
        ),
        (
            env!("CARGO_BIN_EXE_blkit-worker"),
            "BLKIT_WORKER_DATABASE_URL",
            "BLKIT_WORKER_DB_POOL_MAX",
        ),
    ] {
        for (args, settings, expected) in [
            (vec![url.as_str()], vec![], "invalid remote arguments"),
            (
                vec![],
                vec![
                    ("BLKIT_ADMIN_DATABASE_URL", url.as_str()),
                    ("BLKIT_DATABASE_URL", url.as_str()),
                ],
                role,
            ),
            (vec![], vec![(role, url.as_str()), (pool, "0")], pool),
        ] {
            let output = command(binary).args(args).envs(settings).output().unwrap();
            assert!(!output.status.success());
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(text.contains(expected), "{text}");
            assert!(
                !text.contains(secret) && !text.contains("postgres://"),
                "private diagnostics withheld"
            );
            assert!(
                listener.accept().is_err(),
                "invalid configuration attempted database IO"
            );
        }
    }
}

#[test]
fn remote_env_file_overlay_and_secret_file_are_validated_before_io() {
    struct Files(std::path::PathBuf);
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let files =
        Files(std::env::temp_dir().join(format!("blkit-harness-config-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&files.0).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let secret = "PRIVATE_HARNESS_FILE_SENTINEL";
    let secret_file = files.0.join("url");
    std::fs::write(
        &secret_file,
        format!(
            "postgres://role:{secret}@{}/db\r\n",
            listener.local_addr().unwrap()
        ),
    )
    .unwrap();
    for (binary, role, other, pool) in [
        (
            env!("CARGO_BIN_EXE_blkit-api"),
            "BLKIT_API_DATABASE_URL",
            "BLKIT_WORKER_DATABASE_URL_FILE",
            "BLKIT_API_DB_POOL_MAX",
        ),
        (
            env!("CARGO_BIN_EXE_blkit-worker"),
            "BLKIT_WORKER_DATABASE_URL",
            "BLKIT_API_DATABASE_URL_FILE",
            "BLKIT_WORKER_DB_POOL_MAX",
        ),
    ] {
        let dotenv = files.0.join("selected.env");
        // The inactive role and admin path must not be opened; pool and attachment settings
        // must come from the same selected overlay as the role's mounted secret.
        std::fs::write(&dotenv, format!("{role}_FILE={}\n{other}=unreadable\nBLKIT_ADMIN_DATABASE_URL_FILE=unreadable\n{pool}=2\nBLKIT_POSTGRES_INSECURE_DEV=true\nBLKIT_ATTACHMENT_BACKEND=fs\n", secret_file.display())).unwrap();
        for (overlay, expected) in [
            (vec![(pool, "0")], pool),
            (vec![], "attachment initialization failed"),
            (vec![(role, "postgres://role:other@127.0.0.1:1/db")], role),
        ] {
            let output = command(binary)
                .args(["--env-file", dotenv.to_str().unwrap()])
                .envs(overlay)
                .output()
                .unwrap();
            assert!(!output.status.success());
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(text.contains(expected), "{text}");
            assert!(
                !text.contains(secret) && !text.contains("postgres://"),
                "private diagnostics withheld"
            );
            assert!(
                listener.accept().is_err(),
                "configuration failure attempted database IO"
            );
        }
        let output = command(binary)
            .args(["--env-file", files.0.join("missing").to_str().unwrap()])
            .env(role, "postgres://role:private@127.0.0.1:1/db")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("env-file"));
    }
}
