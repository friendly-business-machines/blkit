mod support {
    pub mod postgres;
}

use serde_json::Value;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};
use tokio_postgres::{Client, Config, NoTls};

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    client
}

async fn schema(client: &Client) -> Vec<Value> {
    client
        .query(include_str!("support/postgres_schema_snapshot.sql"), &[])
        .await
        .unwrap()
        .iter()
        .map(|row| serde_json::from_str::<Value>(row.get(0)).unwrap())
        .filter(|object| !object["name"].as_str().unwrap().starts_with("blkit_"))
        .collect()
}

// Missing tables, indexes, trigger definitions, or compatibility records must fail
// before a runtime constructor can hide a broken preparation asset.
#[tokio::test]
async fn canonical_preparation_installs_application_schema_and_compatibility_metadata() {
    let postgres = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(postgres.get_host().await.unwrap().to_string());
    let mut config = Config::new();
    config
        .host(&host)
        .port(postgres.get_host_port_ipv4(5432).await.unwrap())
        .user("postgres")
        .password("postgres")
        .dbname("postgres");
    let mut fresh = connect(&config).await;
    support::postgres::prepare_fresh_database(&mut fresh)
        .await
        .unwrap();

    let history = fresh
        .query(
            "SELECT version, identity, checksum, applied_at <= clock_timestamp() FROM public.blkit_schema_migrations ORDER BY version",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].get::<_, i64>(0), 1);
    assert_eq!(history[0].get::<_, &str>(1), "blkit-postgres-v1");
    let checksum: &str = history[0].get(2);
    assert_eq!(checksum.len(), 64);
    assert!(checksum.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(history[0].get::<_, bool>(3));
    let deployment = fresh
        .query(
            "SELECT singleton, role_prefix, owner_role, api_role, worker_role FROM public.blkit_deployment",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(deployment.len(), 1);
    assert!(deployment[0].get::<_, bool>(0));
    for (index, expected) in [
        "blkit_test",
        "blkit_test_owner",
        "blkit_test_api",
        "blkit_test_worker",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(deployment[0].get::<_, &str>(index + 1), *expected);
    }

    fresh
        .batch_execute("CREATE DATABASE legacy_reference")
        .await
        .unwrap();
    config.dbname("legacy_reference");
    let legacy = connect(&config).await;
    // Historical baseline is an independent oracle, never the preparation input.
    legacy
        .batch_execute(include_str!("fixtures/postgres-legacy/schema.sql"))
        .await
        .unwrap();
    let expected = schema(&legacy).await;
    assert!(!expected.is_empty());
    assert_eq!(schema(&fresh).await, expected);
}

// A metadata collision must not leave application DDL from a failed preparation.
#[tokio::test]
async fn canonical_preparation_rolls_back_schema_when_metadata_creation_fails() {
    let postgres = Postgres::default()
        .with_tag("17.6-alpine")
        .start()
        .await
        .unwrap();
    let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
        .unwrap_or(postgres.get_host().await.unwrap().to_string());
    let mut config = Config::new();
    config
        .host(&host)
        .port(postgres.get_host_port_ipv4(5432).await.unwrap())
        .user("postgres")
        .password("postgres")
        .dbname("postgres");
    let mut fresh = connect(&config).await;
    fresh
        .batch_execute("CREATE TABLE public.blkit_deployment (unrelated TEXT)")
        .await
        .unwrap();
    let error = support::postgres::prepare_fresh_database(&mut fresh)
        .await
        .unwrap_err();
    assert_eq!(
        error.code(),
        Some(&tokio_postgres::error::SqlState::DUPLICATE_TABLE)
    );
    let row = fresh
        .query_one(
            "SELECT to_regclass('public.instances') IS NULL, to_regclass('public.blkit_schema_migrations') IS NULL, to_regclass('public.blkit_deployment') IS NOT NULL",
            &[],
        )
        .await
        .unwrap();
    assert!(row.get::<_, bool>(0));
    assert!(row.get::<_, bool>(1));
    assert!(row.get::<_, bool>(2));
}
