use super::PostgresStore;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};

// A plaintext-capable database must not make the legacy constructors downgrade.
#[tokio::test]
async fn secure_default_store_rejects_a_database_without_tls() {
    let node = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(node.get_host().await.unwrap().to_string());
    let url = format!(
        "postgres://postgres:postgres@{host}:{}/postgres",
        node.get_host_port_ipv4(5432).await.unwrap()
    );
    assert!(
        PostgresStore::connect(&url).await.is_err(),
        "secure default silently used plaintext"
    );
    assert!(
        PostgresStore::connect_with_pool_size(&url, 2)
            .await
            .is_err()
    );
}

#[path = "../../tests/support/postgres_tls.rs"]
mod fixture;
use crate::postgres_connection::PostgresConnectionOptions;
use fixture::{TlsPostgres, trusted_options};
use std::time::{Duration, Instant};

#[tokio::test]
async fn tls_pool_validates_real_server_certificates_and_keeps_lazy_bounded_capacity() {
    let database = TlsPostgres::start().await;
    let store = PostgresStore::connect_with_options(&database.url, 2, &trusted_options())
        .await
        .unwrap();
    assert_eq!(store.0.status().size, 1, "startup must remain lazy");
    let first = store.0.get().await.unwrap();
    let second = store.0.get().await.unwrap();
    for client in [&first, &second] {
        assert!(
            client
                .query_one(
                    "SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()",
                    &[]
                )
                .await
                .unwrap()
                .get::<_, bool>(0)
        );
    }
    assert_eq!(store.0.status().size, 2);
    let sessions: i64 = database
        .admin
        .query_one(
            "SELECT count(*) FROM pg_stat_activity WHERE application_name='blkit_tls_pool'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        sessions, 3,
        "two pooled sessions plus the fixture administrator"
    );
    let started = Instant::now();
    let error = store
        .get_worker("missing")
        .await
        .err()
        .expect("exhaustion must time out");
    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert!(started.elapsed() < Duration::from_secs(3));
    drop((first, second));
    assert!(store.get_worker("missing").await.unwrap().is_none());

    let untrusted = PostgresConnectionOptions::new(None, None).unwrap();
    assert!(
        PostgresStore::connect_with_options(&database.url, 2, &untrusted)
            .await
            .is_err()
    );
    let mismatch = database.url.replace("host=localhost", "host=wrong.example");
    assert!(
        PostgresStore::connect_with_options(&mismatch, 2, &trusted_options())
            .await
            .is_err()
    );
    database.use_certificate(true).await;
    assert!(
        PostgresStore::connect_with_options(&database.url, 2, &trusted_options())
            .await
            .is_err()
    );
    // The server still accepts plaintext: rejection above must not be an outage false-positive.
    let plaintext = PostgresConnectionOptions::new(None, Some("true")).unwrap();
    let plaintext_store = PostgresStore::connect_with_options(&database.url, 1, &plaintext)
        .await
        .unwrap();
    assert!(
        !plaintext_store
            .0
            .get()
            .await
            .unwrap()
            .query_one(
                "SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
}

#[tokio::test]
async fn tls_pool_replacements_fail_closed_and_reuse_the_original_trust_snapshot() {
    let database = TlsPostgres::start().await;
    let path = std::env::temp_dir().join(format!("blkit-pool-ca-{}", uuid::Uuid::new_v4()));
    std::fs::copy(fixture::certificate("ca.pem"), &path).unwrap();
    let options = PostgresConnectionOptions::new(Some(&path), None).unwrap();
    let store = PostgresStore::connect_with_options(&database.url, 1, &options)
        .await
        .unwrap();
    std::fs::remove_file(path).unwrap();
    let client = store.0.get().await.unwrap();
    let old_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    database.use_certificate(true).await;
    database.terminate(old_pid).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !client.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(client);
    assert!(
        store.get_worker("missing").await.is_err(),
        "replacement accepted an expired certificate or downgraded"
    );
    database.use_certificate(false).await;
    let client = store.0.get().await.unwrap();
    let new_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    assert_ne!(old_pid, new_pid);
    assert!(
        client
            .query_one(
                "SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    assert_eq!(
        store.0.status().size,
        1,
        "failed replacement cannot consume capacity"
    );
}

#[cfg(feature = "api-server")]
#[tokio::test]
async fn tls_listener_reconnects_without_downgrade_and_resyncs_before_readiness() {
    use crate::{distributed::DistributedControl, store::Instance};
    use serde_json::json;
    let database = TlsPostgres::start().await;
    let path = std::env::temp_dir().join(format!("blkit-listener-ca-{}", uuid::Uuid::new_v4()));
    std::fs::copy(fixture::certificate("ca.pem"), &path).unwrap();
    let options = PostgresConnectionOptions::new(Some(&path), None).unwrap();
    let store = PostgresStore::connect_with_options(&database.url, 2, &options)
        .await
        .unwrap();
    std::fs::remove_file(path).unwrap();
    store
        .create(&Instance::new("watched", "ns", "1", "p", json!({})))
        .await
        .unwrap();
    let control = DistributedControl::new(store.clone());
    let (snapshot, mut receiver, subscription) = control.subscribe("watched").await.unwrap();
    assert_eq!(snapshot["status"], "pending");
    let pid = database.listener_pid().await.expect("dedicated listener");
    assert!(
        database
            .admin
            .query_one("SELECT ssl FROM pg_stat_ssl WHERE pid=$1", &[&pid])
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let count: i64 = database
        .admin
        .query_one(
            "SELECT count(*) FROM pg_stat_activity WHERE application_name LIKE 'blkit%'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        count <= 4,
        "two pooled sessions plus admin and separate listener: {count}"
    );
    // Notifications still cause persisted-state hints before any outage.
    database
        .admin
        .execute(
            "UPDATE instances SET status='completed' WHERE id='watched'",
            &[],
        )
        .await
        .unwrap();
    let hint = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hint, json!({"id": "watched"}));
    database.use_certificate(true).await;
    database.terminate(pid).await;
    // No successful reconnect or plaintext fallback while the new leaf is invalid.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(database.listener_pid().await.is_none());
    assert!(control.subscribe("watched").await.is_err());
    database
        .admin
        .execute(
            "UPDATE instances SET status='cancelled' WHERE id='watched'",
            &[],
        )
        .await
        .unwrap();
    // Block re-registration: readiness must not be published before resynchronization.
    let mut blocker = store.0.get().await.unwrap();
    let transaction = blocker.transaction().await.unwrap();
    transaction
        .execute(
            "UPDATE instances SET status='cancelled' WHERE id='watched'",
            &[],
        )
        .await
        .unwrap();
    database.use_certificate(false).await;
    assert!(
        control.subscribe("watched").await.is_err(),
        "listener became ready ahead of its locked resync"
    );
    transaction.commit().await.unwrap();
    drop(blocker);
    let resync = tokio::time::timeout(Duration::from_secs(6), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resync["status"], "cancelled");
    let (snapshot, _receiver, registration) = control.subscribe("watched").await.unwrap();
    assert_eq!(snapshot["status"], "cancelled");
    let new_pid = database.listener_pid().await.unwrap();
    assert_ne!(new_pid, pid);
    assert!(
        database
            .admin
            .query_one("SELECT ssl FROM pg_stat_ssl WHERE pid=$1", &[&new_pid])
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    drop((registration, subscription));
}

fn child_test(name: &str, flag: &str) -> Option<std::process::Command> {
    if std::env::var_os(flag).is_some() {
        return None;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &format!("postgres_store::tls_tests::{name}"),
            "--nocapture",
        ])
        .env(flag, "1");
    Some(command)
}

#[tokio::test]
async fn secure_default_pools_accept_platform_trust_without_reading_blkit_settings() {
    if let Some(mut child) = child_test(
        "secure_default_pools_accept_platform_trust_without_reading_blkit_settings",
        "BLKIT_TEST_POOL_NATIVE_CHILD",
    ) {
        let output = child
            .env("SSL_CERT_FILE", fixture::certificate("ca.pem"))
            .env_remove("SSL_CERT_DIR")
            .env("BLKIT_POSTGRES_INSECURE_DEV", "true")
            .env("BLKIT_POSTGRES_CA_FILE", "not-a-real-ca-file")
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
    let database = TlsPostgres::start().await;
    let store = PostgresStore::connect(&database.url).await.unwrap();
    assert_eq!(store.0.status().max_size, 4);
    assert_eq!(store.0.status().size, 1);
    let client = store.0.get().await.unwrap();
    assert!(
        client
            .query_one(
                "SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()",
                &[]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let custom = PostgresStore::connect_with_pool_size(&database.url, 2)
        .await
        .unwrap();
    assert_eq!(custom.0.status().max_size, 2);
    assert_eq!(custom.0.status().size, 1);
}

#[tokio::test]
async fn pool_notices_and_connection_failures_do_not_expose_server_secrets() {
    if let Some(mut child) = child_test(
        "pool_notices_and_connection_failures_do_not_expose_server_secrets",
        "BLKIT_TEST_POOL_LOG_CHILD",
    ) {
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    struct Capture(std::sync::Mutex<Vec<String>>);
    impl log::Log for Capture {
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
    static CAPTURE: Capture = Capture(std::sync::Mutex::new(Vec::new()));
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("pool log capture active");
    let database = TlsPostgres::start().await;
    let options = trusted_options();
    let store = PostgresStore::connect_with_options(&database.url, 1, &options)
        .await
        .unwrap();
    let client = store.0.get().await.unwrap();
    client
        .batch_execute("DO $$ BEGIN RAISE NOTICE 'sentinel-postgres-pool-secret'; END $$")
        .await
        .unwrap();
    let pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    database.terminate(pid).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !client.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(client);
    let invalid = database
        .url
        .replace("user=postgres", "user=sentinel-postgres-pool-secret");
    let error = PostgresStore::connect_with_options(&invalid, 1, &options)
        .await
        .err()
        .expect("unknown user must be rejected");
    assert!(!error.contains("sentinel-postgres-pool-secret"), "{error}");
    assert!(error.contains("credentials"));
    let logs = CAPTURE.0.lock().unwrap().join("\n");
    assert!(logs.contains("pool log capture active"));
    assert!(
        !logs.contains("sentinel-postgres-pool-secret"),
        "raw server notice leaked: {logs}"
    );
}
