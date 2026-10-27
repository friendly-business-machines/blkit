//! Immutable PostgreSQL transport and trust policy, available only with remote persistence.
//!
//! Resolve options once, then reuse the connection for all sessions. No BLKIT settings
//! are read here. Native trust discovery honors rustls-native-certs' standard
//! `SSL_CERT_FILE`/`SSL_CERT_DIR` overrides; connecting never reloads trust or files.

use std::{
    fmt,
    future::{Future, poll_fn},
    path::Path,
};

use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use tokio_postgres::{
    Client, Config,
    config::{Host, SslMode, TargetSessionAttrs},
};
use tokio_postgres_rustls::MakeRustlsConnect;

/// Snapshotted trust and transport settings shared by all connections for a role.
#[derive(Clone)]
pub struct PostgresConnectionOptions {
    tls: MakeRustlsConnect,
    insecure_dev: bool,
}

impl fmt::Debug for PostgresConnectionOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresConnectionOptions")
            .field("insecure_dev", &self.insecure_dev)
            .finish_non_exhaustive()
    }
}

impl PostgresConnectionOptions {
    /// Executable entrypoints must print this once to stderr after validation,
    /// even without a tracing subscriber. Contains no endpoint or credential values.
    pub fn warning(&self) -> Option<&'static str> {
        self.insecure_dev.then_some(
            "postgres transport is insecure plaintext; use BLKIT_POSTGRES_INSECURE_DEV=true only for development/testing",
        )
    }

    /// Loads platform roots plus an optional certificate-only PEM bundle once.
    /// `insecure_dev` accepts only absent/`false` (verified TLS) or exactly `true`
    /// (plaintext development transport, incompatible with a CA bundle).
    ///
    /// # Errors
    /// Fails on invalid settings or unreadable, empty, or malformed trust material.
    /// Paths, credentials, and underlying driver/native-loader errors are redacted.
    pub fn new(
        ca_file: Option<&Path>,
        insecure_dev: Option<&str>,
    ) -> Result<Self, PostgresConnectionError> {
        let insecure_dev = match insecure_dev {
            None | Some("false") => false,
            Some("true") => true,
            _ => {
                return Err(PostgresConnectionError(
                    "BLKIT_POSTGRES_INSECURE_DEV must be exactly true or false",
                ));
            }
        };
        if insecure_dev && ca_file.is_some() {
            return Err(PostgresConnectionError(
                "BLKIT_POSTGRES_CA_FILE conflicts with plaintext insecure development transport",
            ));
        }
        let mut roots = RootCertStore::empty();
        if !insecure_dev {
            let native = rustls_native_certs::load_native_certs();
            if !native.errors.is_empty() {
                return Err(PostgresConnectionError(
                    "postgres platform trust could not be loaded; check the native certificate store and SSL_CERT_FILE/SSL_CERT_DIR",
                ));
            }
            for certificate in native.certs {
                roots.add(certificate).map_err(|_| {
                    PostgresConnectionError(
                        "postgres platform trust contains an invalid certificate",
                    )
                })?;
            }
            if let Some(path) = ca_file {
                let pem = std::fs::read_to_string(path).map_err(|_| {
                    PostgresConnectionError(
                        "BLKIT_POSTGRES_CA_FILE must be a readable PEM CA bundle",
                    )
                })?;
                add_ca_bundle(&mut roots, &pem)?;
            }
            if roots.is_empty() {
                return Err(PostgresConnectionError(
                    "postgres trust is empty; configure platform roots or BLKIT_POSTGRES_CA_FILE",
                ));
            }
        }
        // Explicit provider avoids process-global provider ambiguity in downstream binaries.
        let mut config =
            ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
                .with_safe_default_protocol_versions()
                .expect("ring supports the default TLS protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
        config.alpn_protocols = vec![b"postgresql".to_vec()];
        Ok(Self {
            tls: MakeRustlsConnect::new(config),
            insecure_dev,
        })
    }
}

fn add_ca_bundle(roots: &mut RootCertStore, pem: &str) -> Result<(), PostgresConnectionError> {
    let invalid = PostgresConnectionError(
        "BLKIT_POSTGRES_CA_FILE must contain a nonempty, valid certificate-only PEM CA bundle",
    );
    let mut remaining = pem.trim();
    if remaining.is_empty() {
        return Err(invalid);
    }
    // PEM readers skip garbage and unknown blocks; explicit trust must not silently do so.
    while !remaining.is_empty() {
        if !remaining.starts_with("-----BEGIN CERTIFICATE-----") {
            return Err(invalid);
        }
        let end = remaining.find("-----END CERTIFICATE-----").ok_or(invalid)?
            + "-----END CERTIFICATE-----".len();
        let certificate =
            CertificateDer::from_pem_slice(&remaining.as_bytes()[..end]).map_err(|_| invalid)?;
        roots.add(certificate).map_err(|_| invalid)?;
        remaining = remaining[end..].trim();
    }
    Ok(())
}

/// A parsed endpoint plus immutable verified-TLS (or explicit plaintext) policy.
/// Clones retain the same trust snapshot; no URL or secret is exposed by `Debug`.
#[derive(Clone)]
pub struct PostgresConnection {
    pub(crate) config: Config,
    pub(crate) tls: MakeRustlsConnect,
}

impl fmt::Debug for PostgresConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresConnection")
            .field("ssl_mode", &self.config.get_ssl_mode())
            .finish_non_exhaustive()
    }
}

impl PostgresConnection {
    /// Resolves an endpoint using verified TLS and platform roots.
    ///
    /// # Errors
    /// Rejects invalid endpoints, weakening TLS modes, socket-only configurations,
    /// and invalid platform trust. Errors never contain the supplied endpoint.
    pub fn new(url: &str) -> Result<Self, PostgresConnectionError> {
        Self::with_options(url, &PostgresConnectionOptions::new(None, None)?)
    }

    /// Resolves a PostgreSQL URL or driver keyword configuration using existing options.
    ///
    /// # Errors
    /// Rejects malformed configurations and conflicts with the resolved transport policy.
    /// Only default/explicit `target_session_attrs=any` is supported: the pinned driver
    /// logs server notices before returning connections when checking other modes.
    pub fn with_options(
        url: &str,
        options: &PostgresConnectionOptions,
    ) -> Result<Self, PostgresConnectionError> {
        let default_mode = if options.insecure_dev {
            "disable"
        } else {
            "require"
        };
        // Let the driver's parser handle escapes/duplicates. Prepending a default
        // distinguishes an absent mode from its otherwise opportunistic Prefer default.
        let configured = if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            // Match the driver's credentials-first parsing, including '?' in passwords.
            let after_credentials = url.find('@').map_or(0, |index| index + 1);
            match url[after_credentials..].find('?') {
                Some(index) => {
                    let index = after_credentials + index;
                    format!(
                        "{}?sslmode={default_mode}&{}",
                        &url[..index],
                        &url[index + 1..]
                    )
                }
                None => format!("{url}?sslmode={default_mode}"),
            }
        } else {
            format!("sslmode={default_mode} {url}")
        };
        let mut config: Config = configured.parse().map_err(|_| {
            PostgresConnectionError(
                "invalid postgres configuration; check the database URL and supported connection options",
            )
        })?;
        // Non-Any modes poll the driver's notice-logging Future inside connect().
        if config.get_target_session_attrs() != TargetSessionAttrs::Any {
            return Err(PostgresConnectionError(
                "postgres target_session_attrs must be any; other modes cannot safely suppress server notices",
            ));
        }
        if options.insecure_dev {
            if config.get_ssl_mode() == SslMode::Require {
                return Err(PostgresConnectionError(
                    "postgres required TLS conflicts with BLKIT_POSTGRES_INSECURE_DEV=true",
                ));
            }
            config.ssl_mode(SslMode::Disable);
            tracing::warn!(
                "postgres transport is insecure plaintext; use BLKIT_POSTGRES_INSECURE_DEV=true only for development/testing"
            );
        } else {
            if config.get_ssl_mode() != SslMode::Require {
                return Err(PostgresConnectionError(
                    "postgres verified TLS rejects sslmode=disable or prefer; omit sslmode or use require",
                ));
            }
            if !config
                .get_hosts()
                .iter()
                .any(|host| matches!(host, Host::Tcp(name) if !name.is_empty()))
            {
                return Err(PostgresConnectionError(
                    "postgres verified TLS requires a TCP server hostname; Unix-socket-only configurations are not supported",
                ));
            }
        }
        Ok(Self {
            config,
            tls: options.tls.clone(),
        })
    }

    /// Opens a session using the resolved transport and returns its driver future.
    /// The caller must poll/spawn the future for the lifetime of the client.
    /// Server notices are discarded rather than logged with potentially sensitive contents.
    ///
    /// # Errors
    /// Connection and driver errors are sanitized, including server-provided messages.
    pub async fn connect(
        &self,
    ) -> Result<
        (
            Client,
            impl Future<Output = Result<(), PostgresConnectionError>> + Send + 'static,
        ),
        PostgresConnectionError,
    > {
        self.connect_session().await.map_err(|_| {
            PostgresConnectionError(
                "postgres connection failed; check endpoint, credentials, TLS support, and certificate trust/hostname/validity",
            )
        })
    }

    // deadpool requires the driver's opaque error type; sanitize it at store checkout boundaries.
    async fn connect_session(
        &self,
    ) -> Result<
        (
            Client,
            impl Future<Output = Result<(), PostgresConnectionError>> + Send + 'static,
        ),
        tokio_postgres::Error,
    > {
        let (client, mut connection) = self.connect_raw().await?;
        Ok((client, async move {
            // The driver's Future logs raw notices, including queued startup notices.
            while let Some(message) = poll_fn(|cx| connection.poll_message(cx)).await {
                message.map_err(|_| {
                    PostgresConnectionError(
                        "postgres session failed; check database availability and transport",
                    )
                })?;
            }
            Ok(())
        }))
    }

    // Listeners poll notifications themselves; pool drivers use connect() to discard notices.
    pub(crate) async fn connect_raw(
        &self,
    ) -> Result<
        (
            Client,
            tokio_postgres::Connection<
                tokio_postgres::Socket,
                <MakeRustlsConnect as tokio_postgres::tls::MakeTlsConnect<
                    tokio_postgres::Socket,
                >>::Stream,
            >,
        ),
        tokio_postgres::Error,
    > {
        self.config.connect(self.tls.clone()).await
    }
}

// Keep deadpool's raw-error trait private; public connections expose only safe errors.
pub(crate) struct PostgresPoolConnector(pub(crate) PostgresConnection);

impl deadpool_postgres::Connect for PostgresPoolConnector {
    fn connect(
        &self,
        _config: &Config,
    ) -> std::pin::Pin<
        Box<
            dyn Future<
                    Output = Result<(Client, tokio::task::JoinHandle<()>), tokio_postgres::Error>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let (client, driver) = self.0.connect_session().await?;
            let task = tokio::spawn(async move {
                if let Err(error) = driver.await {
                    tracing::warn!(%error, "postgres pooled session disconnected");
                }
            });
            Ok((client, task))
        })
    }
}

/// A safe diagnostic category, intentionally without raw driver errors or sources.
#[derive(Debug, Clone, Copy)]
pub struct PostgresConnectionError(&'static str);

impl fmt::Display for PostgresConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PostgresConnectionError {}

#[cfg(test)]
#[path = "postgres_connection/tests.rs"]
mod tests;
