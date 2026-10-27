use blkit_core::postgres_schema::{SUPPORTED_SCHEMA_IDENTITY, SUPPORTED_SCHEMA_VERSION};
use tokio_postgres::Client;

/// Prepare only a disposable, fresh test database from CLI-owned assets.
/// Recorded test role names do not provision roles or establish managed-role readiness.
pub async fn prepare_fresh_database(client: &mut Client) -> Result<(), tokio_postgres::Error> {
    let transaction = client.transaction().await?;
    transaction
        .batch_execute("SET LOCAL search_path = public")
        .await?;
    transaction
        .batch_execute(include_str!("../../postgres/0001_initial.sql"))
        .await?;
    let checksum = include_str!("../../postgres/0001_initial.sha256")
        .split_whitespace()
        .next()
        .expect("canonical migration checksum");
    transaction
        .execute(
            "INSERT INTO public.blkit_schema_migrations (version, identity, checksum) VALUES ($1, $2, $3)",
            &[&SUPPORTED_SCHEMA_VERSION, &SUPPORTED_SCHEMA_IDENTITY, &checksum],
        )
        .await?;
    transaction
        .execute(
            "INSERT INTO public.blkit_deployment (singleton, role_prefix, owner_role, api_role, worker_role) VALUES (TRUE, $1, $2, $3, $4)",
            &[&"blkit_test", &"blkit_test_owner", &"blkit_test_api", &"blkit_test_worker"],
        )
        .await?;
    transaction.commit().await
}

/// Plaintext is selected explicitly only by disposable test fixtures.
#[allow(dead_code)]
pub fn plaintext_options() -> blkit_core::postgres_connection::PostgresConnectionOptions {
    blkit_core::postgres_connection::PostgresConnectionOptions::new(None, Some("true")).unwrap()
}

/// Call exactly once after creating a fresh disposable database, never on reconnect.
#[allow(dead_code)]
pub async fn prepare_fresh_database_url(url: &str) {
    let (mut client, driver) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::pin!(driver);
    tokio::select! {
        result = prepare_fresh_database(&mut client) => result.unwrap(),
        _ = &mut driver => panic!("fresh fixture preparation driver stopped"),
    }
}
