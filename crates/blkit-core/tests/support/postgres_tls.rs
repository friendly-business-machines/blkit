use std::{net::ToSocketAddrs, path::PathBuf, time::Duration};

use crate::postgres_connection::{PostgresConnection, PostgresConnectionOptions};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio_postgres::Client;

#[path = "../../../blkit-cli/tests/support/postgres.rs"]
mod preparation;

pub fn certificate(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../blkit-core/tests/fixtures/postgres-tls")
        .join(name)
}

pub fn trusted_options() -> PostgresConnectionOptions {
    PostgresConnectionOptions::new(Some(&certificate("ca.pem")), None).unwrap()
}

pub struct TlsPostgres {
    _node: ContainerAsync<Postgres>,
    pub url: String,
    pub admin: Client,
}

impl TlsPostgres {
    pub async fn start() -> Self {
        let mut database = Self::start_unprepared().await;
        preparation::prepare_fresh_database(&mut database.admin)
            .await
            .unwrap();
        database
    }

    /// Empty application schema for tests exercising actual CLI-managed provisioning.
    pub async fn start_unprepared() -> Self {
        // Copy the disposable key without displaying it; PostgreSQL owns its private copy.
        let node = Postgres::default()
            .with_tag("17.6-alpine")
            .with_env_var("POSTGRES_HOST_AUTH_METHOD", "scram-sha-256")
            .with_env_var(
                "POSTGRES_INITDB_ARGS",
                "--auth-host=scram-sha-256 --auth-local=trust",
            )
            .with_copy_to("/tmp/blkit-tls/server.key", certificate("server.key"))
            .with_copy_to("/tmp/blkit-tls/server.pem", certificate("server.pem"))
            .with_copy_to("/tmp/blkit-tls/expired.pem", certificate("expired.pem"))
            .with_copy_to(
                "/docker-entrypoint-initdb.d/000-tls.sh",
                br#"
cp /tmp/blkit-tls/server.key "$PGDATA/server.key"
chmod 600 "$PGDATA/server.key"
cat >> "$PGDATA/postgresql.auto.conf" <<'CONFIG'
ssl = on
ssl_cert_file = '/tmp/blkit-tls/server.pem'
ssl_key_file = 'server.key'
CONFIG
"#
                .to_vec(),
            )
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(node.get_host().await.unwrap().to_string());
        let address = (host.as_str(), 5432)
            .to_socket_addrs()
            .unwrap()
            .find(|address| address.is_ipv4())
            .unwrap()
            .ip();
        // hostaddr reaches Docker while host remains the certificate's verified identity.
        let url = format!(
            "host=localhost hostaddr={address} port={} user=postgres password=postgres dbname=postgres application_name=blkit_tls_pool",
            node.get_host_port_ipv4(5432).await.unwrap()
        );
        let resolved = PostgresConnection::with_options(&url, &trusted_options()).unwrap();
        let (admin, driver) = resolved.connect().await.unwrap();
        tokio::spawn(driver);
        let version: String = admin
            .query_one("SHOW server_version_num", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            version, "170006",
            "TLS fixtures must exercise PostgreSQL 17.6"
        );
        Self {
            _node: node,
            url,
            admin,
        }
    }

    pub async fn use_certificate(&self, expired: bool) {
        let path = if expired {
            "/tmp/blkit-tls/expired.pem"
        } else {
            "/tmp/blkit-tls/server.pem"
        };
        self.admin
            .batch_execute(&format!("ALTER SYSTEM SET ssl_cert_file = '{path}'"))
            .await
            .unwrap();
        self.admin
            .simple_query("SELECT pg_reload_conf()")
            .await
            .unwrap();
        // Wait for the postmaster to finish the asynchronous certificate reload.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let connection =
                    PostgresConnection::with_options(&self.url, &trusted_options()).unwrap();
                let connected = match connection.connect().await {
                    Ok((client, driver)) => {
                        let task = tokio::spawn(driver);
                        drop(client);
                        task.await.unwrap().unwrap();
                        true
                    }
                    Err(_) => false,
                };
                if connected != expired {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .expect("certificate reload did not take effect");
    }

    #[cfg(feature = "api-server")]
    pub async fn listener_pid(&self) -> Option<i32> {
        self.admin.query_opt("SELECT pid FROM pg_stat_activity WHERE application_name LIKE 'blkit\\_%' ESCAPE '\\' AND application_name <> 'blkit_tls_pool'", &[]).await.unwrap().map(|row| row.get(0))
    }

    pub async fn terminate(&self, pid: i32) {
        assert!(
            self.admin
                .query_one("SELECT pg_terminate_backend($1)", &[&pid])
                .await
                .unwrap()
                .get::<_, bool>(0)
        );
    }
}
