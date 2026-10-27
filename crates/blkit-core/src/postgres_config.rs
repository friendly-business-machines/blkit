//! Explicit executable PostgreSQL configuration (remote persistence only).
//!
//! Only the selected dotenv file is parsed, as literal data without interpolation.
//! Process values overlay it by setting name; neither loading nor resolution changes
//! the process environment. All paths are relative to the process working directory.

use std::{collections::HashMap, ffi::OsString, fmt, net::IpAddr, path::Path};

use crate::{
    postgres_connection::{PostgresConnection, PostgresConnectionOptions},
    postgres_store::PostgresStore,
};
use tokio_postgres::config::Host;

/// Selects exactly one credential pair; no shared or administrative fallback exists.
#[derive(Clone, Copy, Debug)]
pub enum DatabaseRole {
    Api,
    Worker,
    Admin,
}

impl DatabaseRole {
    fn settings(self) -> (&'static str, &'static str) {
        match self {
            Self::Api => ("BLKIT_API_DATABASE_URL", "BLKIT_API_DATABASE_URL_FILE"),
            Self::Worker => (
                "BLKIT_WORKER_DATABASE_URL",
                "BLKIT_WORKER_DATABASE_URL_FILE",
            ),
            Self::Admin => ("BLKIT_ADMIN_DATABASE_URL", "BLKIT_ADMIN_DATABASE_URL_FILE"),
        }
    }
}

/// Immutable settings snapshot. Values are available only through explicit lookup;
/// `Debug` never prints names or values. Unused roles are not resolved or validated.
pub struct EffectivePostgresConfig {
    values: HashMap<OsString, OsString>,
}

impl fmt::Debug for EffectivePostgresConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectivePostgresConfig")
            .finish_non_exhaustive()
    }
}

impl EffectivePostgresConfig {
    /// Snapshots process settings over an optional, explicitly selected dotenv file.
    /// No `.env` discovery occurs. Native `SSL_CERT_*` trust discovery remains a
    /// platform-library concern; dotenv values do not modify the native environment.
    ///
    /// # Errors
    /// Rejects any unreadable or malformed selected dotenv, even if env is sufficient.
    pub fn from_env(env_file: Option<&Path>) -> Result<Self, PostgresConfigError> {
        Self::from_iter(env_file, std::env::vars_os())
    }

    /// Builds the same snapshot using supplied process values, without ambient reads.
    /// Empty process values still override file values. Dollar expressions remain
    /// literal; dotenv quoting, escapes, comments and `export` use the parser's syntax.
    ///
    /// # Errors
    /// Rejects unreadable or malformed selected dotenv files with redacted diagnostics.
    pub fn from_iter<K: Into<OsString>, V: Into<OsString>>(
        env_file: Option<&Path>,
        environment: impl IntoIterator<Item = (K, V)>,
    ) -> Result<Self, PostgresConfigError> {
        let mut values = HashMap::new();
        if let Some(path) = env_file {
            let file = std::fs::File::open(path).map_err(|_| {
                PostgresConfigError("env-file must be readable UTF-8 dotenv data".into())
            })?;
            let parsed = dotenv::EnvLoader::with_reader(file)
                .sequence(dotenv::EnvSequence::InputOnly)
                .substitution(false)
                .load()
                .map_err(|_| {
                    PostgresConfigError("env-file must be readable, valid UTF-8 dotenv data".into())
                })?;
            values.extend(
                parsed
                    .into_iter()
                    .map(|(name, value)| (setting_key(name.into()), OsString::from(value))),
            );
        }
        values.extend(
            environment
                .into_iter()
                .map(|(name, value)| (setting_key(name.into()), value.into())),
        );
        Ok(Self { values })
    }

    /// Explicit lookup for consumers such as pool and attachment configuration.
    /// Empty and absent remain distinct. Treat returned values as potentially secret.
    ///
    /// # Errors
    /// Rejects non-UTF-8 values only when looked up, not for unused settings.
    pub fn get(&self, name: &str) -> Result<Option<&str>, PostgresConfigError> {
        self.values
            .get(&setting_key(name.into()))
            .map(|value| {
                value.to_str().ok_or_else(|| {
                    PostgresConfigError("requested configuration setting must be UTF-8".into())
                })
            })
            .transpose()
    }

    /// Resolves the API pool limit (default 4) without reading worker settings.
    ///
    /// # Errors
    /// Rejects a configured value unless it is a positive UTF-8 integer.
    pub fn api_pool_max(&self) -> Result<usize, PostgresConfigError> {
        self.pool_max("BLKIT_API_DB_POOL_MAX")
    }

    /// Resolves the worker pool limit (default 4) without reading API settings.
    ///
    /// # Errors
    /// Rejects a configured value unless it is a positive UTF-8 integer.
    pub fn worker_pool_max(&self) -> Result<usize, PostgresConfigError> {
        self.pool_max("BLKIT_WORKER_DB_POOL_MAX")
    }

    fn pool_max(&self, setting: &'static str) -> Result<usize, PostgresConfigError> {
        self.get(setting)
            .map_err(|_| setting_error(setting, "must be UTF-8"))?
            .map_or(Ok(4), |value| {
                value
                    .parse::<usize>()
                    .ok()
                    .filter(|max| *max > 0)
                    .ok_or_else(|| setting_error(setting, "must be a positive integer"))
            })
    }

    /// Resolves shared transport/trust once for all required roles from this overlay.
    ///
    /// # Errors
    /// Uses the connector's strict CA and insecure-development validation unchanged.
    pub fn connection_options(&self) -> Result<PostgresConnectionOptions, PostgresConfigError> {
        let ca = self
            .get("BLKIT_POSTGRES_CA_FILE")
            .map_err(|_| setting_error("BLKIT_POSTGRES_CA_FILE", "must be UTF-8"))?;
        let insecure = self
            .get("BLKIT_POSTGRES_INSECURE_DEV")
            .map_err(|_| setting_error("BLKIT_POSTGRES_INSECURE_DEV", "must be UTF-8"))?;
        PostgresConnectionOptions::new(ca.map(Path::new), insecure)
            .map_err(|error| PostgresConfigError(error.to_string()))
    }

    /// Resolves only the requested URL or mounted secret using shared options.
    /// Requires an explicit nonempty parsed user and host, never OS-user/host guesses.
    /// Missing dbname follows the driver default (the parsed username).
    ///
    /// # Errors
    /// Rejects missing, empty, conflicting or invalid required inputs without echoing
    /// values, paths or raw parser/driver errors. A secret allows one trailing LF/CRLF.
    pub fn resolve_role(
        &self,
        role: DatabaseRole,
        options: &PostgresConnectionOptions,
    ) -> Result<RoleDatabase, PostgresConfigError> {
        let (direct, file) = role.settings();
        let has_direct = self.values.contains_key(&setting_key(direct.into()));
        let has_file = self.values.contains_key(&setting_key(file.into()));
        if has_direct && has_file {
            return Err(setting_error(
                direct,
                "and its _FILE counterpart are both set",
            ));
        }
        let contents;
        let (url, setting) = if has_file {
            let path = self
                .get(file)
                .map_err(|_| setting_error(file, "must be UTF-8"))?
                .filter(|path| !path.is_empty())
                .ok_or_else(|| setting_error(file, "must be a nonempty secret-file path"))?;
            contents = std::fs::read_to_string(path)
                .map_err(|_| setting_error(file, "must name a readable UTF-8 secret file"))?;
            let url = contents
                .strip_suffix("\r\n")
                .or_else(|| contents.strip_suffix('\n'))
                .unwrap_or(&contents);
            (url, file)
        } else {
            let url = self
                .get(direct)
                .map_err(|_| setting_error(direct, "must be UTF-8"))?
                .ok_or_else(|| setting_error(direct, "or its _FILE counterpart is required"))?;
            (url, direct)
        };
        if !(url.starts_with("postgres://") || url.starts_with("postgresql://"))
            || url.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(setting_error(
                setting,
                "must contain one nonempty PostgreSQL URL; encode special characters",
            ));
        }
        let connection = PostgresConnection::with_options(url, options)
            .map_err(|error| PostgresConfigError(format!("{setting}: {error}")))?;
        let parsed = &connection.config;
        if parsed.get_user().is_none_or(str::is_empty) {
            return Err(setting_error(setting, "must specify a nonempty login user"));
        }
        if parsed.get_dbname().is_some_and(str::is_empty) {
            return Err(setting_error(setting, "must identify a nonempty database"));
        }
        let hosts = parsed.get_hosts();
        if hosts.is_empty()
            || hosts.iter().any(|host| match host {
                Host::Tcp(name) => name.is_empty(),
                #[cfg(unix)]
                Host::Unix(path) => path.as_os_str().is_empty(),
            })
        {
            return Err(setting_error(setting, "must specify a nonempty host"));
        }
        // The driver defers these shape checks until connect; fail before database access.
        if (!parsed.get_hostaddrs().is_empty() && parsed.get_hostaddrs().len() != hosts.len())
            || (parsed.get_ports().len() > 1 && parsed.get_ports().len() != hosts.len())
        {
            return Err(setting_error(
                setting,
                "has inconsistent host, hostaddr or port counts",
            ));
        }
        Ok(RoleDatabase { role, connection })
    }

    /// Resolves both runtime identities for the same endpoint/database.
    ///
    /// # Errors
    /// Rejects missing roles, equivalent login identities or differing endpoints.
    pub fn resolve_combined(
        &self,
        options: &PostgresConnectionOptions,
    ) -> Result<(RoleDatabase, RoleDatabase), PostgresConfigError> {
        let api = self.resolve_role(DatabaseRole::Api, options)?;
        let worker = self.resolve_role(DatabaseRole::Worker, options)?;
        if api.username() == worker.username() {
            return Err(setting_error(
                DatabaseRole::Worker.settings().0,
                "must use a different login user from BLKIT_API_DATABASE_URL",
            ));
        }
        api.ensure_same_database(&worker)?;
        Ok((api, worker))
    }
}

fn setting_key(name: OsString) -> OsString {
    #[cfg(windows)]
    {
        name.to_ascii_uppercase()
    }
    #[cfg(not(windows))]
    {
        name
    }
}

/// A role's parsed credentials plus immutable transport. No raw URL is retained or
/// exposed. `Debug` redacts even usernames/endpoints, which can contain sensitive data.
pub struct RoleDatabase {
    role: DatabaseRole,
    connection: PostgresConnection,
}

impl fmt::Debug for RoleDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoleDatabase")
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

impl RoleDatabase {
    /// Safe connector for administration; it exposes neither raw Config nor URL.
    pub fn connection(&self) -> &PostgresConnection {
        &self.connection
    }

    /// Explicit credential access for provisioning login identities. Do not log it.
    pub fn username(&self) -> &str {
        self.connection
            .config
            .get_user()
            .expect("role resolution validates user")
    }

    /// Explicit secret access for creating a login later. Do not log it.
    pub fn password(&self) -> Option<&[u8]> {
        self.connection.config.get_password()
    }

    fn database(&self) -> &str {
        self.connection
            .config
            .get_dbname()
            .unwrap_or_else(|| self.username())
    }

    /// Checks parsed hosts, hostaddrs, effective ports and database without network
    /// access. DNS case, IP spelling, default ports and URL encoding may differ;
    /// host order and explicit hostaddr remain significant. No aliases are resolved.
    ///
    /// # Errors
    /// Reports differing endpoints by role setting name only.
    pub fn ensure_same_database(&self, other: &Self) -> Result<(), PostgresConfigError> {
        let left = &self.connection.config;
        let right = &other.connection.config;
        let hosts_match = left.get_hosts().len() == right.get_hosts().len()
            && left
                .get_hosts()
                .iter()
                .zip(right.get_hosts())
                .enumerate()
                .all(|(index, (a, b))| {
                    same_host(a, b) && effective_port(left, index) == effective_port(right, index)
                });
        if !hosts_match
            || left.get_hostaddrs() != right.get_hostaddrs()
            || self.database() != other.database()
        {
            return Err(setting_error(
                other.role.settings().0,
                "must identify the same parsed endpoint and database",
            ));
        }
        Ok(())
    }

    /// Connects an existing store using the already resolved credentials and trust.
    ///
    /// # Errors
    /// Rejects zero pool size or returns the store's sanitized connection errors.
    pub async fn connect_store(&self, max: usize) -> Result<PostgresStore, String> {
        if max == 0 {
            return Err("postgres pool maximum must be positive".into());
        }
        PostgresStore::connect_resolved(self.connection.clone(), max).await
    }
}

fn same_host(left: &Host, right: &Host) -> bool {
    match (left, right) {
        (Host::Tcp(a), Host::Tcp(b)) => match (a.parse::<IpAddr>(), b.parse::<IpAddr>()) {
            (Ok(a), Ok(b)) => a == b,
            _ => a.eq_ignore_ascii_case(b),
        },
        #[cfg(unix)]
        (Host::Unix(a), Host::Unix(b)) => a == b,
        #[cfg(unix)]
        _ => false,
    }
}

fn effective_port(config: &tokio_postgres::Config, index: usize) -> u16 {
    config
        .get_ports()
        .get(index)
        .or_else(|| config.get_ports().first())
        .copied()
        .unwrap_or(5432)
}

fn setting_error(setting: &'static str, category: &'static str) -> PostgresConfigError {
    PostgresConfigError(format!("{setting} {category}"))
}

/// Safe configuration diagnostic; intentionally omits underlying errors and sources.
#[derive(Debug)]
pub struct PostgresConfigError(String);
impl fmt::Display for PostgresConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PostgresConfigError {}

#[cfg(test)]
#[path = "postgres_config/tests.rs"]
mod tests;
