//! Actual generated processes: argument/configuration failures must precede side effects.
use std::{
    ffi::OsStr,
    fs,
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

const SENTINEL: &str = "PRIVATE_REMOTE_ENTRYPOINT_SENTINEL";

#[path = "../../../tools/test-support/generated_remote.rs"]
mod generated_remote;
use generated_remote::Generated;

fn run(binary: &Path, args: &[impl AsRef<OsStr>], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("TOKIO_WORKER_THREADS", "2")
        .args(args)
        .envs(env.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("generated process did not exit before the side-effect deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

fn text(output: &Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !text.contains(SENTINEL),
        "generated process disclosed a sentinel (output withheld)"
    );
    text
}

fn rejected(binary: &Path, args: &[&str], env: &[(&str, &str)], expected: &str) {
    let output = run(binary, args, env);
    let text = text(&output);
    assert!(!output.status.success(), "unexpected successful startup");
    assert!(text.contains(expected), "expected {expected:?}; got {text}");
}

#[test]
fn generated_remote_entrypoints_validate_arguments_and_configuration_before_io() {
    // Build each independently selected role, including both split packages and compatibility worker.
    let projects: Vec<_> = [
        "api-only",
        "worker-only",
        "api-worker",
        "api-worker-split",
        "worker",
    ]
    .into_iter()
    .map(Generated::new)
    .collect();
    let mut roles = Vec::new();
    for (index, project) in projects.iter().enumerate() {
        let target = project.build(false);
        let mode = match project.build_target {
            "api-only" => "api",
            "api-worker" => "api-worker",
            _ => "worker",
        };
        roles.push((index, target.join(format!("{}-{mode}", project.name)), mode));
        if project.build_target == "api-worker-split" {
            roles.push((
                index,
                project.build(true).join(format!("{}-api", project.name)),
                "api",
            ));
        }
    }
    for (index, binary, mode) in &roles {
        let root = &projects[*index].root;
        let malformed = root.join(SENTINEL);
        fs::write(&malformed, format!("not dotenv {SENTINEL} '\n")).unwrap();
        let malformed = malformed.to_str().unwrap();
        let log = root.join("must-not-create.log");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let api = format!(
            "postgres://api:{SENTINEL}@{}/db",
            listener.local_addr().unwrap()
        );
        let worker = format!(
            "postgres://worker:{SENTINEL}@{}/db",
            listener.local_addr().unwrap()
        );
        let poison = [
            ("BLKIT_API_DATABASE_URL", api.as_str()),
            ("BLKIT_WORKER_DATABASE_URL", worker.as_str()),
            ("BLKIT_API_DATABASE_URL_FILE", malformed),
            ("BLKIT_WORKER_DATABASE_URL_FILE", malformed),
            ("BLKIT_LOG_OUTPUTS", "file"),
            ("BLKIT_LOG_FILE", log.to_str().unwrap()),
            ("GOOGLE_APPLICATION_CREDENTIALS", malformed),
        ];
        let help = run(binary, &["--help"], &poison);
        assert!(help.status.success());
        let help = text(&help);
        assert!(
            help.contains("[--env-file PATH]"),
            "legacy help layout: {help}"
        );
        assert!(!help.contains("POSTGRES_URL"));
        for args in [
            vec![api.as_str()],
            vec!["--password", SENTINEL],
            vec!["--help", SENTINEL],
            vec!["--env-file", malformed, api.as_str()],
            vec!["--env-file", malformed, "--database-url", SENTINEL],
        ] {
            rejected(binary, &args, &poison, "invalid remote arguments");
        }
        assert!(!log.exists(), "invalid args initialized logging");
        #[cfg(unix)]
        {
            let fifo = root.join("must-not-read.fifo");
            if !fifo.exists() {
                assert!(
                    Command::new("mkfifo")
                        .arg(&fifo)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            let fifo = fifo.to_str().unwrap();
            let no_reads = [
                ("BLKIT_API_DATABASE_URL_FILE", fifo),
                ("BLKIT_WORKER_DATABASE_URL_FILE", fifo),
                ("BLKIT_POSTGRES_CA_FILE", fifo),
                ("BLKIT_LOG_OUTPUTS", "file"),
                ("BLKIT_LOG_FILE", log.to_str().unwrap()),
            ];
            assert!(run(binary, &["--help"], &no_reads).status.success());
            rejected(
                binary,
                &["--env-file", fifo, &api],
                &no_reads,
                "invalid remote arguments",
            );
            rejected(
                binary,
                &["--env-file", fifo, "--password", SENTINEL],
                &no_reads,
                "invalid remote arguments",
            );
            use std::{ffi::OsString, os::unix::ffi::OsStringExt};
            let mut invalid = SENTINEL.as_bytes().to_vec();
            invalid.push(0xff);
            let output = run(binary, &[OsString::from_vec(invalid)], &no_reads);
            assert!(!output.status.success());
            assert!(text(&output).contains("invalid remote arguments"));
            assert!(!log.exists());
        }
        rejected(binary, &["--env-file", malformed], &[], "env-file");
        let operands: &[&str] = match *mode {
            "api" => &["localhost:0"],
            "worker" => &["7", "6001"],
            _ => &["7", "6001", "[::1]:0"],
        };
        let required = if *mode == "worker" {
            "BLKIT_WORKER_DATABASE_URL"
        } else {
            "BLKIT_API_DATABASE_URL"
        };
        rejected(binary, operands, &[], required);
        rejected(binary, &[], &[], required);
        rejected(
            binary,
            operands,
            &[("BLKIT_ADMIN_DATABASE_URL", &api), ("DATABASE_URL", &api)],
            required,
        );
        let invalid_operands: &[&str] = if *mode == "api" {
            &["invalid-bind"]
        } else {
            &["0", "5000"]
        };
        rejected(
            binary,
            invalid_operands,
            &poison,
            "invalid remote arguments",
        );
        // Selecting dotenv is accepted, even with non-secret operands; malformed selected data is not ignored.
        let mut args = vec!["--env-file", malformed];
        args.extend_from_slice(operands);
        rejected(binary, &args, &[], "env-file");
        rejected(
            binary,
            &args,
            &[
                ("BLKIT_API_DATABASE_URL", &api),
                ("BLKIT_WORKER_DATABASE_URL", &worker),
            ],
            "env-file",
        );
        let dotenv = root.join("selected.env");
        fs::write(&dotenv, format!("BLKIT_API_DATABASE_URL={api}\nBLKIT_WORKER_DATABASE_URL={worker}\nBLKIT_POSTGRES_INSECURE_DEV=true\nBLKIT_ATTACHMENT_BACKEND=s3\nBLKIT_ATTACHMENT_BUCKET=\n")).unwrap();
        let mut selected = vec!["--env-file", dotenv.to_str().unwrap()];
        selected.extend_from_slice(operands);
        rejected(binary, &selected, &[], "attachment initialization failed");
        // Validation errors must not announce an enabled transport that was never selected successfully.
        let invalid_attachment = run(binary, &selected, &[]);
        assert!(!text(&invalid_attachment).contains("insecure plaintext"));
        let file_setting = format!("{required}_FILE");
        rejected(
            binary,
            &selected,
            &[(file_setting.as_str(), malformed)],
            "both set",
        );
        let secret = root.join("mounted-url");
        fs::write(
            &secret,
            format!("{}\r\n", if *mode == "worker" { &worker } else { &api }),
        )
        .unwrap();
        let mut mounted = vec![
            (file_setting.as_str(), secret.to_str().unwrap()),
            ("BLKIT_POSTGRES_INSECURE_DEV", "true"),
        ];
        if *mode == "api-worker" {
            mounted.push(("BLKIT_WORKER_DATABASE_URL", &worker));
        }
        rejected(
            binary,
            operands,
            &mounted,
            "attachment initialization failed",
        );
        for contents in [SENTINEL, ""] {
            fs::write(&secret, contents).unwrap();
            rejected(binary, operands, &mounted, &file_setting);
        }
        fs::remove_file(&secret).unwrap();
        rejected(binary, operands, &mounted, &file_setting);
        let inactive = if *mode == "worker" {
            "BLKIT_API_DATABASE_URL_FILE"
        } else {
            "BLKIT_WORKER_DATABASE_URL_FILE"
        };
        let active = [
            (
                required,
                if *mode == "worker" {
                    worker.as_str()
                } else {
                    api.as_str()
                },
            ),
            ("BLKIT_POSTGRES_INSECURE_DEV", "true"),
            ("BLKIT_ADMIN_DATABASE_URL_FILE", malformed),
            ("DATABASE_URL", SENTINEL),
            (inactive, malformed),
        ];
        if *mode != "api-worker" {
            let inactive_pool = if *mode == "worker" {
                "BLKIT_API_DB_POOL_MAX"
            } else {
                "BLKIT_WORKER_DB_POOL_MAX"
            };
            let mut active = active.to_vec();
            active.push((inactive_pool, SENTINEL));
            rejected(
                binary,
                operands,
                &active,
                "attachment initialization failed",
            );
        } else {
            rejected(binary, operands, &active[..4], "BLKIT_WORKER_DATABASE_URL");
            rejected(
                binary,
                operands,
                &[
                    ("BLKIT_API_DATABASE_URL", &api),
                    ("BLKIT_WORKER_DATABASE_URL", &api),
                    ("BLKIT_POSTGRES_INSECURE_DEV", "true"),
                ],
                "different login user",
            );
            let other = worker.replace("/db", "/other");
            rejected(
                binary,
                operands,
                &[
                    ("BLKIT_API_DATABASE_URL", &api),
                    ("BLKIT_WORKER_DATABASE_URL", &other),
                    ("BLKIT_POSTGRES_INSECURE_DEV", "true"),
                ],
                "same",
            );
        }
        let pool = if *mode == "worker" {
            "BLKIT_WORKER_DB_POOL_MAX"
        } else {
            "BLKIT_API_DB_POOL_MAX"
        };
        rejected(binary, &selected, &[(pool, SENTINEL)], pool);
        let pool_file = root.join("pool.env");
        fs::write(
            &pool_file,
            format!(
                "{}\n{pool}={SENTINEL}\n",
                fs::read_to_string(&dotenv).unwrap()
            ),
        )
        .unwrap();
        rejected(
            binary,
            &["--env-file", pool_file.to_str().unwrap()],
            &[],
            pool,
        );
        if *mode == "api-worker" {
            rejected(
                binary,
                &selected,
                &[("BLKIT_WORKER_DB_POOL_MAX", SENTINEL)],
                "BLKIT_WORKER_DB_POOL_MAX",
            );
        }
        rejected(
            binary,
            &selected,
            &[
                ("BLKIT_POSTGRES_INSECURE_DEV", "false"),
                ("BLKIT_POSTGRES_CA_FILE", malformed),
            ],
            "BLKIT_POSTGRES_CA_FILE",
        );
        // No ambient Azure account: provider initialization fails locally, after all BLKIT validation.
        let provider = [
            ("BLKIT_ATTACHMENT_BACKEND", "azure"),
            ("BLKIT_ATTACHMENT_CONTAINER", "attachments"),
        ];
        let failure = run(binary, &selected, &provider);
        assert!(!failure.status.success());
        let failure_text = text(&failure);
        assert!(
            failure_text.contains("attachment initialization failed"),
            "{failure_text}"
        );
        assert_eq!(
            String::from_utf8_lossy(&failure.stderr)
                .matches("insecure plaintext")
                .count(),
            1
        );
        if *mode != "api" {
            // A directory cannot be opened as a log sink; paths and raw IO errors stay private.
            let directory = root.join(format!("{SENTINEL}-directory"));
            fs::create_dir_all(&directory).unwrap();
            let mut logging_failure = provider.to_vec();
            logging_failure.extend_from_slice(&[
                ("BLKIT_LOG_OUTPUTS", "file"),
                ("BLKIT_LOG_FILE", directory.to_str().unwrap()),
            ]);
            rejected(
                binary,
                &selected,
                &logging_failure,
                "logging initialization failed",
            );
        }
        rejected(
            binary,
            &selected,
            &[("BLKIT_POSTGRES_INSECURE_DEV", SENTINEL)],
            "BLKIT_POSTGRES_INSECURE_DEV",
        );
        rejected(binary, &selected, &[(required, SENTINEL)], required);
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "invalid configuration opened a database connection"
        );
        println!(
            "validated {} ({mode}): {}",
            projects[*index].build_target,
            binary.display()
        );
    }
}
