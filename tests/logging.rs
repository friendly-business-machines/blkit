use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn logging_probe() {
    if std::env::var_os("BLKIT_LOG_PROBE").is_none() {
        return;
    }
    let _guard = blkit::logging::init("orders-server").unwrap_or_else(|error| {
        eprintln!("logging configuration: {error}");
        std::process::exit(2);
    });
    blkit::logging::tracing::info!("ready");
    blkit::logging::tracing::error!(instance_id = "instance-42", "failed");
}

fn run(vars: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg("logging_probe")
        .arg("--nocapture")
        .env("BLKIT_LOG_PROBE", "1");
    for key in [
        "BLKIT_LOG_LEVEL",
        "BLKIT_LOG_OUTPUTS",
        "BLKIT_LOG_FILE",
        "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
    ] {
        command.env_remove(key);
    }
    for (key, value) in vars {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[test]
fn default_stdout_and_info() {
    let output = run(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("ready") && text.contains("failed"), "{text}");
}

#[test]
fn file_appends_without_console_output() {
    let file = std::env::temp_dir().join(format!("blkit-log-{}.txt", std::process::id()));
    fs::write(&file, "previous\n").unwrap();
    let path = file.to_str().unwrap();
    for _ in 0..2 {
        let output = run(&[("BLKIT_LOG_OUTPUTS", "file"), ("BLKIT_LOG_FILE", path)]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("ready"));
    }
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("previous\n") && text.matches("ready").count() == 2,
        "{text}"
    );
    fs::remove_file(file).unwrap();
}

#[test]
fn mixed_outputs_filter_by_level() {
    let file = std::env::temp_dir().join(format!("blkit-mixed-{}.txt", std::process::id()));
    let output = run(&[
        ("BLKIT_LOG_OUTPUTS", "stdout,file"),
        ("BLKIT_LOG_FILE", file.to_str().unwrap()),
        ("BLKIT_LOG_LEVEL", "error"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for text in [
        String::from_utf8_lossy(&output.stdout).into_owned(),
        fs::read_to_string(&file).unwrap(),
    ] {
        assert!(
            text.contains("instance-42") && !text.contains("ready"),
            "{text}"
        );
    }
    fs::remove_file(file).unwrap();
}

#[test]
fn otlp_exports_logs_with_service_identity_alongside_console_and_file() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1/logs", listener.local_addr().unwrap());
    let receiver = std::thread::spawn(move || {
        let started = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut buf = [0; 8192];
                    loop {
                        match socket.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                request.extend_from_slice(&buf[..n]);
                                if request
                                    .windows(b"instance-42".len())
                                    .any(|part| part == b"instance-42")
                                {
                                    break;
                                }
                            }
                        }
                    }
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                        .unwrap();
                    return request;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && started.elapsed() < Duration::from_secs(5) =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("OTLP receiver: {e}"),
            }
        }
    });
    let file = std::env::temp_dir().join(format!("blkit-otlp-{}.txt", std::process::id()));
    let output = run(&[
        ("BLKIT_LOG_OUTPUTS", "stdout,file,otlp"),
        ("BLKIT_LOG_FILE", file.to_str().unwrap()),
        ("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT", &endpoint),
    ]);
    let request = receiver.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("instance-42"));
    assert!(fs::read_to_string(&file).unwrap().contains("instance-42"));
    assert!(
        request
            .windows(b"orders-server".len())
            .any(|part| part == b"orders-server"),
        "missing service name in OTLP request"
    );
    assert!(
        request
            .windows(b"instance-42".len())
            .any(|part| part == b"instance-42"),
        "missing event in OTLP request"
    );
    fs::remove_file(file).unwrap();
}

#[cfg(unix)]
#[test]
fn non_unicode_level_or_outputs_fail_configuration() {
    use std::os::unix::ffi::OsStringExt;
    for key in ["BLKIT_LOG_LEVEL", "BLKIT_LOG_OUTPUTS"] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "logging_probe", "--nocapture"])
            .env("BLKIT_LOG_PROBE", "1")
            .env_remove("BLKIT_LOG_LEVEL")
            .env_remove("BLKIT_LOG_OUTPUTS")
            .env(key, std::ffi::OsString::from_vec(vec![0xff]));
        let output = command.output().unwrap();
        assert!(
            !output.status.success(),
            "{key}: invalid setting was accepted"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains(key));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("ready"));
    }
}

#[test]
fn invalid_configuration_reports_error_before_events() {
    for (vars, expected) in [
        (vec![("BLKIT_LOG_LEVEL", "verbose")], "BLKIT_LOG_LEVEL"),
        (
            vec![("BLKIT_LOG_OUTPUTS", "stdout,stdout")],
            "BLKIT_LOG_OUTPUTS",
        ),
        (vec![("BLKIT_LOG_OUTPUTS", "")], "BLKIT_LOG_OUTPUTS"),
        (vec![("BLKIT_LOG_OUTPUTS", "file")], "BLKIT_LOG_FILE"),
        (
            vec![
                ("BLKIT_LOG_OUTPUTS", "file"),
                ("BLKIT_LOG_FILE", "/no-such-blkit-dir/log"),
            ],
            "BLKIT_LOG_FILE",
        ),
        (
            vec![("BLKIT_LOG_OUTPUTS", "otlp")],
            "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
        ),
        (
            vec![
                ("BLKIT_LOG_OUTPUTS", "otlp"),
                ("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT", "file:///bad"),
            ],
            "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
        ),
    ] {
        let output = run(&vars);
        assert!(!output.status.success(), "{vars:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{vars:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("ready"));
    }
}
