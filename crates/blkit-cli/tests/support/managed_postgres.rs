use blkit_core::postgres_config::EffectivePostgresConfig;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio_postgres::Client;

pub struct Database {
    _container: ContainerAsync<Postgres>,
    pub client: Client,
    pub url: String,
    driver: tokio::task::JoinHandle<()>,
}

impl Database {
    pub async fn start() -> Self {
        let container = Postgres::default()
            .with_tag("17.6-alpine")
            .with_env_var("POSTGRES_HOST_AUTH_METHOD", "scram-sha-256")
            .with_env_var(
                "POSTGRES_INITDB_ARGS",
                "--auth-host=scram-sha-256 --auth-local=trust",
            )
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(container.get_host().await.unwrap().to_string());
        let url = format!(
            "postgres://postgres:postgres@{host}:{}/postgres",
            container.get_host_port_ipv4(5432).await.unwrap()
        );
        let (client, driver) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        let driver = tokio::spawn(async move { driver.await.unwrap() });
        Self {
            _container: container,
            client,
            url,
            driver,
        }
    }

    /// Access only this fixture's container for disposable backup/restore utilities.
    #[allow(dead_code)]
    pub fn container(&self) -> &ContainerAsync<Postgres> {
        &self._container
    }

    pub fn settings(&self) -> EffectivePostgresConfig {
        let endpoint = self.url.rsplit_once('@').unwrap().1;
        EffectivePostgresConfig::from_iter(
            None,
            [
                ("BLKIT_ADMIN_DATABASE_URL", self.url.clone()),
                (
                    "BLKIT_API_DATABASE_URL",
                    format!("postgres://blkit_api:startup-test@{endpoint}"),
                ),
                (
                    "BLKIT_WORKER_DATABASE_URL",
                    format!("postgres://blkit_worker:startup-test@{endpoint}"),
                ),
                ("BLKIT_POSTGRES_INSECURE_DEV", "true".into()),
            ],
        )
        .unwrap()
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        // Stop polling before owned container shutdown closes the inspection connection.
        self.driver.abort();
    }
}
