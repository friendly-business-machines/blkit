#![cfg(feature = "remote-persistence")]

use blkit_core::remote_executable::{
    ExecutableError, ExecutableOperation, RemoteArgs, RemoteExecutable,
};
use std::{
    ffi::OsString,
    process::{Command, Termination},
};

const SECRET: &str = "sentinel-remote-secret";

#[test]
fn remote_operands_preserve_defaults_order_and_bind_spellings() {
    for mode in [
        RemoteExecutable::Api,
        RemoteExecutable::Worker,
        RemoteExecutable::Combined,
    ] {
        let args = RemoteArgs::parse(mode, Vec::<OsString>::new())
            .unwrap()
            .unwrap();
        assert_eq!(args.max_tasks, 32);
        assert_eq!(args.lease_ms, 5000);
        assert_eq!(args.bind, "127.0.0.1:3000");
        assert!(args.env_file.is_none());
    }
    for bind in [
        "localhost:0",
        "127.0.0.1:8080",
        "[::1]:8080",
        "my_host.example.:42",
    ] {
        let args = RemoteArgs::parse(RemoteExecutable::Api, [bind])
            .unwrap()
            .unwrap();
        assert_eq!(args.bind, bind);
        let args = RemoteArgs::parse(
            RemoteExecutable::Combined,
            ["--env-file", "settings.env", "7", "1200", bind],
        )
        .unwrap()
        .unwrap();
        assert_eq!(args.max_tasks, 7);
        assert_eq!(args.lease_ms, 1200);
        assert_eq!(args.bind, bind);
        assert_eq!(args.env_file.unwrap(), std::path::Path::new("settings.env"));
    }
    let worker = RemoteArgs::parse(RemoteExecutable::Worker, ["9", "2000"])
        .unwrap()
        .unwrap();
    assert_eq!((worker.max_tasks, worker.lease_ms), (9, 2000));
    assert!(
        RemoteArgs::parse(RemoteExecutable::Api, ["--help"])
            .unwrap()
            .is_none()
    );
}

#[test]
fn invalid_remote_operands_fail_without_echoing_values() {
    for mode in [
        RemoteExecutable::Api,
        RemoteExecutable::Worker,
        RemoteExecutable::Combined,
    ] {
        for args in [
            vec![format!("postgres://user:{SECRET}@localhost/db")],
            vec![format!("postgresql://user:{SECRET}@localhost/db")],
            vec![format!("--password={SECRET}")],
            vec!["--password".into(), SECRET.into()],
            vec!["--env-file".into()],
            vec![
                "--env-file".into(),
                "a".into(),
                "--env-file".into(),
                SECRET.into(),
            ],
            vec!["--help".into(), SECRET.into()],
            vec!["--help".into(), format!("--password={SECRET}")],
            vec!["--env-file".into(), "".into()],
            vec!["a".into(), "b".into(), "c".into(), SECRET.into()],
        ] {
            let error = RemoteArgs::parse(mode, args).unwrap_err();
            assert!(!format!("{error} {error:?}").contains(SECRET));
            assert!(std::error::Error::source(&error).is_none());
        }
    }
    for bind in [
        "",
        "localhost",
        "localhost:65536",
        ":3000",
        "[::1:3000",
        "::1:3000",
        "host name:3000",
        "user:password@host:3000",
        "https://host:3000",
    ] {
        assert!(RemoteArgs::parse(RemoteExecutable::Api, [bind]).is_err());
    }
    for args in [
        ["0", "1"],
        ["1", "0"],
        ["1", "-1"],
        ["invalid", "2"],
        ["1", "invalid"],
    ] {
        assert!(RemoteArgs::parse(RemoteExecutable::Worker, args).is_err());
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_operands_fail_safely_but_native_file_paths_are_preserved() {
    use std::os::unix::ffi::OsStringExt;
    let value = OsString::from_vec(b"sentinel-remote-secret\xff".to_vec());
    let error = RemoteArgs::parse(RemoteExecutable::Api, [value.clone()]).unwrap_err();
    assert!(!format!("{error:?} {error}").contains(SECRET));
    let args = RemoteArgs::parse(
        RemoteExecutable::Api,
        [OsString::from("--env-file"), value.clone()],
    )
    .unwrap()
    .unwrap();
    assert!(!format!("{args:?}").contains(SECRET));
    assert_eq!(args.env_file.unwrap().as_os_str(), value);
}

fn probe() -> Result<(), Box<dyn std::error::Error>> {
    use blkit_core::postgres_config::{DatabaseRole, EffectivePostgresConfig};
    let raw: Vec<String> =
        serde_json::from_str(&std::env::var("BLKIT_REMOTE_TEST_ARGS").unwrap()).unwrap();
    let Some(args) = RemoteArgs::parse(RemoteExecutable::Api, raw)? else {
        println!("{}", RemoteExecutable::Api.usage());
        return Ok(());
    };
    let effective = EffectivePostgresConfig::from_env(args.env_file.as_deref())
        .map_err(ExecutableError::from)?;
    let options = effective
        .connection_options()
        .map_err(ExecutableError::from)?;
    let database = effective
        .resolve_role(DatabaseRole::Api, &options)
        .map_err(ExecutableError::from)?;
    effective.api_pool_max().map_err(ExecutableError::from)?;
    if let Some(warning) = options.warning() {
        eprintln!("{warning}");
    }
    if let Ok(phase) = std::env::var("BLKIT_REMOTE_TEST_DRIVER") {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (client, driver) = database
                .connection()
                .connect()
                .await
                .map_err(ExecutableError::from)?;
            if phase == "session" {
                driver.await.map_err(ExecutableError::from)?;
            }
            drop(client);
            Ok::<(), ExecutableError>(())
        })?;
    }
    if std::env::var_os("BLKIT_REMOTE_TEST_OPERATION").is_some() {
        return Err(ExecutableError::operation(ExecutableOperation::Bind).into());
    }
    Ok(())
}

#[test]
fn remote_executable_probe() {
    if std::env::var_os("BLKIT_REMOTE_TEST_ARGS").is_some() {
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .finish();
        let _subscriber = std::env::var_os("BLKIT_REMOTE_TEST_NO_SUBSCRIBER")
            .is_none()
            .then(|| tracing::subscriber::set_default(subscriber));
        let result = probe();
        if let Err(error) = &result {
            tracing::error!(error = %error, detail = ?error, "remote startup failed");
        }
        std::process::exit(if result.report() == std::process::ExitCode::SUCCESS {
            0
        } else {
            1
        });
    }
}

fn run(args: &[&str], vars: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "remote_executable_probe", "--nocapture"])
        .env_clear()
        .env(
            "BLKIT_REMOTE_TEST_ARGS",
            serde_json::to_string(args).unwrap(),
        )
        .env("BLKIT_POSTGRES_INSECURE_DEV", "true");
    for (key, value) in vars {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[test]
fn executable_termination_and_help_never_expose_configuration_or_operands() {
    let url = format!("postgres://user:{SECRET}@localhost/db");
    let directory = std::env::temp_dir().join(format!("blkit-remote-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let dotenv = directory.join(format!("{SECRET}.env"));
    std::fs::write(
        &dotenv,
        format!("BLKIT_API_DATABASE_URL='unterminated-{SECRET}\n"),
    )
    .unwrap();
    let secret = directory.join(format!("{SECRET}.url"));
    std::fs::write(&secret, SECRET).unwrap();
    let missing = directory.join(format!("{SECRET}-missing"));
    for (args, vars, context) in [
        (vec!["--help"], vec![], "usage:"),
        (vec!["--help", &url], vec![], "arguments"),
        (vec![&url], vec![], "arguments"),
        (vec!["--password", SECRET], vec![], "arguments"),
        (
            vec!["sentinel-remote-secret:invalid"],
            vec![],
            "BIND_ADDRESS",
        ),
        (
            vec!["--env-file", dotenv.to_str().unwrap()],
            vec![],
            "env-file",
        ),
        (
            vec!["--env-file", missing.to_str().unwrap()],
            vec![],
            "env-file",
        ),
        (
            vec![],
            vec![("BLKIT_API_DATABASE_URL_FILE", missing.to_str().unwrap())],
            "BLKIT_API_DATABASE_URL_FILE",
        ),
        (
            vec![],
            vec![("BLKIT_API_DATABASE_URL_FILE", secret.to_str().unwrap())],
            "BLKIT_API_DATABASE_URL_FILE",
        ),
        (
            vec![],
            vec![(
                "BLKIT_API_DATABASE_URL",
                "postgres://user:sentinel-remote-secret@localhost:invalid/db",
            )],
            "BLKIT_API_DATABASE_URL",
        ),
        (
            vec![],
            vec![
                ("BLKIT_API_DATABASE_URL", &url),
                ("BLKIT_API_DB_POOL_MAX", SECRET),
            ],
            "BLKIT_API_DB_POOL_MAX",
        ),
        (
            vec![],
            vec![
                ("BLKIT_API_DATABASE_URL", &url),
                ("BLKIT_REMOTE_TEST_OPERATION", SECRET),
            ],
            "bind",
        ),
    ] {
        let output = run(&args, &vars);
        assert_eq!(output.status.success(), context == "usage:");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            text.contains(context),
            "missing safe operation/setting context"
        );
        assert!(
            !text.contains(SECRET) && !text.contains(&url),
            "executable output leaked input"
        );
    }
    let output = run(
        &[],
        &[
            ("BLKIT_API_DATABASE_URL", &url),
            ("BLKIT_REMOTE_TEST_NO_SUBSCRIBER", "1"),
        ],
    );
    assert!(output.status.success());
    let warning = String::from_utf8_lossy(&output.stderr);
    assert!(warning.contains("plaintext") && warning.contains("BLKIT_POSTGRES_INSECURE_DEV"));
    assert!(!warning.contains(SECRET));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn driver_errors_are_safe_in_tracing_and_boxed_error_termination() {
    use std::io::{Read, Write};
    for phase in ["startup", "session"] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(error) => panic!("driver fixture accept: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut size = [0; 4];
            stream.read_exact(&mut size).unwrap();
            let mut startup = vec![0; usize::try_from(u32::from_be_bytes(size) - 4).unwrap()];
            stream.read_exact(&mut startup).unwrap();
            if phase == "session" {
                stream
                    .write_all(b"R\0\0\0\x08\0\0\0\0Z\0\0\0\x05I")
                    .unwrap();
            }
            let fields =
                format!("SFATAL\0C28000\0M{SECRET}\0Dpostgres://user:{SECRET}@localhost/db\0\0");
            stream.write_all(b"E").unwrap();
            stream
                .write_all(&u32::try_from(fields.len() + 4).unwrap().to_be_bytes())
                .unwrap();
            stream.write_all(fields.as_bytes()).unwrap();
        });
        let url = format!("postgres://user:{SECRET}@127.0.0.1:{port}/db");
        let output = run(
            &[],
            &[
                ("BLKIT_API_DATABASE_URL", &url),
                ("BLKIT_REMOTE_TEST_DRIVER", phase),
            ],
        );
        server.join().unwrap();
        assert!(!output.status.success());
        let logs = String::from_utf8_lossy(&output.stdout);
        let termination = String::from_utf8_lossy(&output.stderr);
        assert!(logs.contains("remote startup failed") && logs.contains("postgres"));
        assert!(termination.contains("Error:") && termination.contains("postgres"));
        for text in [logs, termination] {
            assert!(!text.contains(SECRET) && !text.contains(&url));
        }
    }
}
