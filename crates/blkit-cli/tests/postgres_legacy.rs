use std::{fs, path::Path};

use blkit_core::compiled_graph::GraphCheckpoint;
use serde_json::Value;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};
use tokio_postgres::{Client, Config, NoTls};

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/postgres-legacy")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("missing legacy fixture {name}: {error}"))
}

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    client
}

async fn data(client: &Client) -> Value {
    let mut tables = serde_json::Map::new();
    for table in [
        "instances",
        "input_requests",
        "instance_attachments",
        "instance_interests",
        "workers",
        "worker_capabilities",
        "process_policies",
    ] {
        // Identifiers are exclusively the fixed table names above.
        let row = client
            .query_one(
                &format!(
                    "SELECT COALESCE(jsonb_agg(to_jsonb(t)), '[]'::jsonb)::text FROM public.{table} t"
                ),
                &[],
            )
            .await
            .unwrap();
        let mut rows: Vec<Value> = serde_json::from_str(row.get(0)).unwrap();
        rows.sort_by_key(Value::to_string);
        tables.insert(table.into(), Value::Array(rows));
    }
    Value::Object(tables)
}

#[tokio::test]
async fn legacy_fixture_independently_preserves_exact_data_and_checkpoint_activations() {
    let schema_sql = fixture("schema.sql");
    let data_sql = fixture("data.sql");
    let mut expected: Value = serde_json::from_str(&fixture("expected-data.json")).unwrap();
    for rows in expected.as_object_mut().unwrap().values_mut() {
        rows.as_array_mut().unwrap().sort_by_key(Value::to_string);
    }

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
    let legacy = connect(&config).await;
    legacy.batch_execute("SET TIME ZONE 'UTC'").await.unwrap();
    // This database must be loadable without any runtime constructor/migration.
    legacy.batch_execute(&schema_sql).await.unwrap();
    legacy.batch_execute(&data_sql).await.unwrap();
    assert_eq!(data(&legacy).await, expected, "standalone persisted rows");

    for row in legacy
        .query(
            "SELECT id, checkpoint FROM instances WHERE checkpoint IS NOT NULL ORDER BY id",
            &[],
        )
        .await
        .unwrap()
    {
        let checkpoint: GraphCheckpoint = serde_json::from_str(row.get(1)).unwrap();
        let stored = serde_json::to_value(&checkpoint).unwrap();
        for request in checkpoint.requests() {
            assert!(
                stored["ready"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|activation| {
                        activation["id"].as_u64().unwrap().to_string() == request.id
                    }),
                "instance {} request {} has no matching ready activation",
                row.get::<_, &str>(0),
                request.id
            );
        }
    }
}
