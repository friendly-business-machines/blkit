use super::*;
use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const SECRET: &str = "sentinel-postgres-secret";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/postgres-tls")
        .join(name)
}

fn secure_options() -> PostgresConnectionOptions {
    PostgresConnectionOptions::new(Some(&fixture("ca.pem")), None).unwrap()
}

#[test]
fn secure_defaults_require_tls_without_weak_url_modes() {
    for url in [
        "postgres://localhost/db",
        "postgresql://localhost/db",
        "host=localhost dbname=db",
    ] {
        let connection = PostgresConnection::new(url).unwrap();
        assert_eq!(connection.config.get_ssl_mode(), SslMode::Require);
    }
    for url in [
        "postgres://localhost/db?sslmode=disable",
        "postgres://localhost/db?sslmode=prefer",
        "postgres://localhost/db?sslmode=pr%65fer",
        "postgres://localhost/db?%73slmode=prefer",
        "host=localhost sslmode=disable",
        "host=localhost sslmode='prefer'",
    ] {
        assert!(
            PostgresConnection::new(url).is_err(),
            "accepted weakening mode: {url}"
        );
    }
    assert!(PostgresConnection::new("postgres://localhost/db?sslmode=require").is_ok());
    // A password's punctuation must not be mistaken for URL parameters.
    assert!(PostgresConnection::new("postgres://user:what?sslmode=prefer@localhost/db").is_ok());
}

#[test]
fn secure_mode_rejects_socket_only_and_missing_hosts() {
    for url in [
        "host=/tmp dbname=db",
        "postgres:///db?host=%2Ftmp",
        "dbname=db",
        "hostaddr=127.0.0.1 dbname=db",
    ] {
        assert!(
            PostgresConnection::new(url).is_err(),
            "accepted no verified host: {url}"
        );
    }
    assert!(PostgresConnection::new("host=localhost hostaddr=127.0.0.1 dbname=db").is_ok());
}

#[test]
fn insecure_dev_is_strict_and_conflicts_fail() {
    for value in [None, Some("false")] {
        let options = PostgresConnectionOptions::new(None, value).unwrap();
        assert!(options.warning().is_none());
        let connection = PostgresConnection::with_options("host=localhost", &options).unwrap();
        assert_eq!(connection.config.get_ssl_mode(), SslMode::Require);
    }
    for value in ["", "TRUE", "False", "1", "0", " true", "true\n", SECRET] {
        let error = PostgresConnectionOptions::new(None, Some(value)).unwrap_err();
        assert!(!format!("{error:?} {error}").contains(SECRET));
        assert!(error.to_string().contains("BLKIT_POSTGRES_INSECURE_DEV"));
    }
    let options = PostgresConnectionOptions::new(None, Some("true")).unwrap();
    for url in [
        "host=localhost",
        "host=/tmp",
        "postgres://localhost/db?sslmode=disable",
        "postgres://localhost/db?sslmode=prefer",
    ] {
        assert_eq!(
            PostgresConnection::with_options(url, &options)
                .unwrap()
                .config
                .get_ssl_mode(),
            SslMode::Disable
        );
    }
    assert!(PostgresConnection::with_options("host=localhost sslmode=require", &options).is_err());
    assert!(PostgresConnectionOptions::new(Some(&fixture("ca.pem")), Some("true")).is_err());
}

#[test]
fn explicit_ca_must_be_readable_nonempty_and_entirely_valid() {
    assert!(PostgresConnectionOptions::new(Some(&fixture(SECRET)), None).is_err());
    assert!(PostgresConnectionOptions::new(Some(&fixture(".")), None).is_err());
    let path = std::env::temp_dir().join(format!("blkit-ca-{}", uuid::Uuid::new_v4()));
    let valid = std::fs::read(fixture("ca.pem")).unwrap();
    for contents in [
        Vec::new(),
        b" \n".to_vec(),
        SECRET.as_bytes().to_vec(),
        b"-----BEGIN CERTIFICATE-----\ninvalid!\n-----END CERTIFICATE-----\n".to_vec(),
        b"-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----\n".to_vec(),
        [valid.as_slice(), b"-----BEGIN CERTIFICATE-----\nbroken\n"].concat(),
        [valid.as_slice(), b"\nsentinel-postgres-secret\n"].concat(),
        [
            valid.as_slice(),
            b"\n-----BEGIN PRIVATE KEY-----\nYWJj\n-----END PRIVATE KEY-----\n",
        ]
        .concat(),
    ] {
        std::fs::write(&path, contents).unwrap();
        let error = PostgresConnectionOptions::new(Some(&path), None).unwrap_err();
        assert!(!format!("{error:?} {error}").contains(SECRET));
        assert!(error.to_string().contains("BLKIT_POSTGRES_CA_FILE"));
    }
    std::fs::write(&path, [valid.as_slice(), valid.as_slice()].concat()).unwrap();
    assert!(PostgresConnectionOptions::new(Some(&path), None).is_ok());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn debug_and_error_chains_never_include_connection_inputs() {
    let connection = PostgresConnection::new(&format!(
        "postgres://{SECRET}:{SECRET}@localhost/{SECRET}?application_name={SECRET}"
    ))
    .unwrap();
    assert!(!format!("{connection:?} {:?}", secure_options()).contains(SECRET));
    for url in [
        format!("postgres://user:{SECRET}@localhost/db?{SECRET}=x"),
        format!("host=localhost password={SECRET} {SECRET}=value"),
        format!("postgres://user:{SECRET}@localhost:{SECRET}/db"),
    ] {
        let error = PostgresConnection::new(&url).unwrap_err();
        assert!(!format!("{error:?} {error}").contains(SECRET));
        assert!(std::error::Error::source(&error).is_none());
    }
}

#[derive(Clone)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);
impl Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn plaintext_resolution_emits_a_safe_warning() {
    let capture = LogCapture(Arc::default());
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let options = PostgresConnectionOptions::new(None, Some("true")).unwrap();
        PostgresConnection::with_options(
            &format!("postgres://user:{SECRET}@localhost/db"),
            &options,
        )
        .unwrap();
    });
    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("WARN") && logs.contains("insecure") && logs.contains("plaintext"),
        "{logs}"
    );
    assert!(!logs.contains(SECRET));
}

// Minimal PostgreSQL wire fixture: negotiate SSL, authenticate without a password,
// then wait for termination. TLS is real rustls over loopback, not a mock verifier.
async fn startup<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) {
    let size = stream.read_u32().await.unwrap();
    let mut message = vec![0; usize::try_from(size - 4).unwrap()];
    stream.read_exact(&mut message).await.unwrap();
    stream
        .write_all(b"R\0\0\0\x08\0\0\0\0Z\0\0\0\x05I")
        .await
        .unwrap();
    stream.flush().await.unwrap();
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest).await;
}

async fn server(
    cert: Option<&str>,
    tls_supported: bool,
    direct_tls: bool,
) -> (u16, tokio::task::JoinHandle<bool>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let tls = cert.map(|name| {
        let certificates = CertificateDer::pem_file_iter(fixture(name))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = rustls::pki_types::PrivateKeyDer::from_pem_file(fixture("server.key")).unwrap();
        let mut config = rustls::ServerConfig::builder_with_provider(
            rustls::crypto::ring::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .unwrap();
        if direct_tls {
            config.alpn_protocols = vec![b"postgresql".to_vec()];
        }
        tokio_rustls::TlsAcceptor::from(Arc::new(config))
    });
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        if direct_tls {
            let mut stream = tls.unwrap().accept(stream).await.unwrap();
            assert_eq!(
                stream.get_ref().1.alpn_protocol(),
                Some(b"postgresql".as_slice()),
                "direct TLS must negotiate PostgreSQL ALPN before startup"
            );
            startup(&mut stream).await;
            return true;
        }
        if tls_supported {
            let mut request = [0; 8];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(request, [0, 0, 0, 8, 4, 210, 22, 47]);
            stream
                .write_all(if tls.is_some() { b"S" } else { b"N" })
                .await
                .unwrap();
            if let Some(tls) = tls {
                match tls.accept(stream).await {
                    Ok(mut stream) => {
                        startup(&mut stream).await;
                        return true;
                    }
                    Err(_) => return false,
                }
            }
            // No fallback: the client must close without sending a startup packet.
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
            false
        } else {
            startup(&mut stream).await;
            true
        }
    });
    (port, task)
}

async fn connect_fixture(
    options: &PostgresConnectionOptions,
    certificate: Option<&str>,
    hostname: &str,
    expected: bool,
) {
    let (port, server) = server(certificate, true, false).await;
    let connection = PostgresConnection::with_options(&format!("host={hostname} hostaddr=127.0.0.1 port={port} user={SECRET} password={SECRET} dbname={SECRET}"), options).unwrap();
    let result = connection.connect().await;
    if expected {
        let (client, driver) = result.unwrap();
        let driver = tokio::spawn(driver);
        drop(client);
        driver.await.unwrap().unwrap();
    } else {
        let error = match result {
            Ok(_) => panic!("unverified connection succeeded"),
            Err(error) => error,
        };
        assert!(!format!("{error:?} {error}").contains(SECRET));
        assert!(std::error::Error::source(&error).is_none());
    }
    assert_eq!(server.await.unwrap(), expected);
}

#[tokio::test]
async fn connector_validates_ca_hostname_expiry_and_requires_tls() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let options = secure_options();
        connect_fixture(&options, Some("server.pem"), "localhost", true).await;
        connect_fixture(&options, Some("server.pem"), "wrong.example", false).await;
        connect_fixture(&options, Some("server.pem"), "127.0.0.1", false).await;
        connect_fixture(&options, Some("expired.pem"), "localhost", false).await;
        connect_fixture(&options, None, "localhost", false).await;
        let untrusted = PostgresConnectionOptions::new(None, None).unwrap();
        connect_fixture(&untrusted, Some("server.pem"), "localhost", false).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn trust_is_snapshotted_not_reloaded_from_ca_file() {
    let path = std::env::temp_dir().join(format!("blkit-ca-{}", uuid::Uuid::new_v4()));
    std::fs::copy(fixture("ca.pem"), &path).unwrap();
    let options = PostgresConnectionOptions::new(Some(&path), None).unwrap();
    std::fs::remove_file(path).unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        connect_fixture(&options, Some("server.pem"), "localhost", true),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn explicit_plaintext_connects_without_ssl_negotiation() {
    let options = PostgresConnectionOptions::new(None, Some("true")).unwrap();
    let (port, server) = server(None, false, false).await;
    let connection = PostgresConnection::with_options(
        &format!("host=127.0.0.1 port={port} user=test"),
        &options,
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        let (client, driver) = connection.connect().await.unwrap();
        let driver = tokio::spawn(driver);
        drop(client);
        driver.await.unwrap().unwrap();
        assert!(server.await.unwrap());
    })
    .await
    .unwrap();
}

#[test]
fn platform_trust_is_snapshotted_augmented_and_failures_are_safe() {
    const CHILD: &str = "BLKIT_TEST_NATIVE_ROOT_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        if mode == "invalid" {
            let error = PostgresConnectionOptions::new(None, None).unwrap_err();
            assert!(!format!("{error:?} {error}").contains(SECRET));
            assert!(std::error::Error::source(&error).is_none());
            return;
        }
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let options = PostgresConnectionOptions::new(None, None).unwrap();
            // This supplemental leaf cannot validate server.pem without the native CA.
            let augmented =
                PostgresConnectionOptions::new(Some(&fixture("expired.pem")), None).unwrap();
            std::fs::remove_file(std::env::var_os("SSL_CERT_FILE").unwrap()).unwrap();
            tokio::time::timeout(Duration::from_secs(15), async {
                connect_fixture(&options, Some("server.pem"), "localhost", true).await;
                connect_fixture(&augmented, Some("server.pem"), "localhost", true).await;
            })
            .await
            .unwrap();
        });
        return;
    }
    let path = std::env::temp_dir().join(format!("{SECRET}-{}", uuid::Uuid::new_v4()));
    std::fs::copy(fixture("ca.pem"), &path).unwrap();
    // The valid child deletes its trust file after resolution; the invalid child
    // subsequently proves native-loader errors cannot leak that secret-bearing path.
    for mode in ["valid", "invalid"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "postgres_connection::tests::platform_trust_is_snapshotted_augmented_and_failures_are_safe",
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("BLKIT_POSTGRES_INSECURE_DEV", "true")
            .env("BLKIT_POSTGRES_CA_FILE", SECRET)
            .env("SSL_CERT_FILE", &path)
            .env_remove("SSL_CERT_DIR")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn server_errors_are_redacted_during_startup_and_session_driving() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let options = PostgresConnectionOptions::new(None, Some("true")).unwrap();
        for after_ready in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let size = stream.read_u32().await.unwrap();
                let mut startup = vec![0; usize::try_from(size - 4).unwrap()];
                stream.read_exact(&mut startup).await.unwrap();
                if after_ready {
                    stream
                        .write_all(b"R\0\0\0\x08\0\0\0\0Z\0\0\0\x05I")
                        .await
                        .unwrap();
                }
                let fields = format!("SFATAL\0C28000\0M{SECRET}\0\0");
                stream.write_u8(b'E').await.unwrap();
                stream
                    .write_u32(u32::try_from(fields.len() + 4).unwrap())
                    .await
                    .unwrap();
                stream.write_all(fields.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            });
            let connection = PostgresConnection::with_options(
                &format!("host=127.0.0.1 port={port} user={SECRET} password={SECRET}"),
                &options,
            )
            .unwrap();
            let error = if after_ready {
                let (_client, driver) = connection.connect().await.unwrap();
                driver.await.unwrap_err()
            } else {
                match connection.connect().await {
                    Ok(_) => panic!("accepted fatal startup error"),
                    Err(error) => error,
                }
            };
            assert!(!format!("{error:?} {error}").contains(SECRET));
            assert!(std::error::Error::source(&error).is_none());
            server.await.unwrap();
        }
    })
    .await
    .unwrap();
}

struct DriverLogCapture(Mutex<Vec<String>>);

impl log::Log for DriverLogCapture {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

#[tokio::test]
async fn startup_and_session_notices_never_reach_the_log_facade() {
    const CHILD: &str = "BLKIT_TEST_NOTICE_LOG_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Isolate the process-global log facade from other tests/subscribers.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "postgres_connection::tests::startup_and_session_notices_never_reach_the_log_facade",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    static CAPTURE: DriverLogCapture = DriverLogCapture(Mutex::new(Vec::new()));
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("notice regression log capture active");

    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let size = stream.read_u32().await.unwrap();
            let mut startup = vec![0; usize::try_from(size - 4).unwrap()];
            stream.read_exact(&mut startup).await.unwrap();
            stream.write_all(b"R\0\0\0\x08\0\0\0\0").await.unwrap();
            for phase in ["startup", "session"] {
                if phase == "session" {
                    stream.write_all(b"Z\0\0\0\x05I").await.unwrap();
                    // Require actual query progress while the notice-safe driver runs.
                    assert_eq!(stream.read_u8().await.unwrap(), b'Q');
                    let size = stream.read_u32().await.unwrap();
                    let mut query = vec![0; usize::try_from(size - 4).unwrap()];
                    stream.read_exact(&mut query).await.unwrap();
                    assert_eq!(query, b"\0");
                }
                let fields = format!("SNOTICE\0C00000\0M{SECRET}-{phase}\0\0");
                stream.write_u8(b'N').await.unwrap();
                stream
                    .write_u32(u32::try_from(fields.len() + 4).unwrap())
                    .await
                    .unwrap();
                stream.write_all(fields.as_bytes()).await.unwrap();
            }
            stream.write_all(b"I\0\0\0\x04Z\0\0\0\x05I").await.unwrap();
            let mut termination = [0; 5];
            stream.read_exact(&mut termination).await.unwrap();
            assert_eq!(termination, *b"X\0\0\0\x04");
        });
        let options = PostgresConnectionOptions::new(None, Some("true")).unwrap();
        let connection = PostgresConnection::with_options(
            &format!("host=127.0.0.1 port={port} user={SECRET} password={SECRET}"),
            &options,
        )
        .unwrap();
        let (client, driver) = connection.connect().await.unwrap();
        let driver = tokio::spawn(driver);
        client.batch_execute("").await.unwrap();
        drop(client);
        driver.await.unwrap().unwrap();
        server.await.unwrap();
    })
    .await
    .unwrap();

    let logs = CAPTURE.0.lock().unwrap();
    assert!(
        logs.iter()
            .any(|message| message == "notice regression log capture active")
    );
    let startup_leaked = logs
        .iter()
        .any(|message| message.contains(&format!("{SECRET}-startup")));
    let session_leaked = logs
        .iter()
        .any(|message| message.contains(&format!("{SECRET}-session")));
    assert_eq!(
        (startup_leaked, session_leaked),
        (false, false),
        "server notice contents must not reach the log facade (startup, session)"
    );
}

#[tokio::test]
async fn direct_tls_negotiates_postgresql_alpn() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (port, server) = server(Some("server.pem"), true, true).await;
        let connection = PostgresConnection::with_options(
            &format!(
                "host=localhost hostaddr=127.0.0.1 port={port} user=test sslnegotiation=direct"
            ),
            &secure_options(),
        )
        .unwrap();
        let (client, driver) = connection.connect().await.unwrap();
        let driver = tokio::spawn(driver);
        drop(client);
        driver.await.unwrap().unwrap();
        assert!(server.await.unwrap());
    })
    .await
    .unwrap();
}
#[test]
fn target_session_attrs_rejects_notice_unsafe_modes_before_network_access() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    for insecure in [None, Some("true")] {
        let options = PostgresConnectionOptions::new(None, insecure).unwrap();
        for mode in ["read-write", "read-only"] {
            let url = format!(
                "postgres://{SECRET}:{SECRET}@localhost:{port}/db?target_session_attrs={mode}"
            );
            let error = PostgresConnection::with_options(&url, &options).unwrap_err();
            assert!(error.to_string().contains("target_session_attrs"));
            assert!(!format!("{error:?} {error}").contains(SECRET));
            assert!(std::error::Error::source(&error).is_none());
        }
        for suffix in ["", "?target_session_attrs=any"] {
            let url = format!("postgres://user:{SECRET}@localhost:{port}/db{suffix}");
            assert!(PostgresConnection::with_options(&url, &options).is_ok());
        }
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
