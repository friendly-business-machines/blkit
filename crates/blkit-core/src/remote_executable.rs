//! URL-free remote executable arguments and safe terminal/log diagnostics.
//! Local Turso arguments and CLI command dispatch are deliberately separate.

use crate::{postgres_config::PostgresConfigError, postgres_connection::PostgresConnectionError};
use std::{ffi::OsString, fmt, path::PathBuf};

/// Selects the generated remote layout; compatibility worker uses `Worker` too.
#[derive(Clone, Copy, Debug)]
pub enum RemoteExecutable {
    Api,
    Worker,
    Combined,
}

impl RemoteExecutable {
    pub fn usage(self) -> &'static str {
        match self {
            Self::Api => "usage: api [--env-file PATH] [BIND_ADDRESS]",
            Self::Worker => "usage: worker [--env-file PATH] [MAX_TASKS] [LEASE_MS]",
            Self::Combined => {
                "usage: api-worker [--env-file PATH] [MAX_TASKS] [LEASE_MS] [BIND_ADDRESS]"
            }
        }
    }
}

/// Validated non-secret operands. Debug also redacts potentially sensitive file paths.
pub struct RemoteArgs {
    pub env_file: Option<PathBuf>,
    pub max_tasks: usize,
    pub lease_ms: i64,
    pub bind: String,
}

impl fmt::Debug for RemoteArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteArgs").finish_non_exhaustive()
    }
}

impl RemoteArgs {
    /// Parses operands only (exclude argv[0]), without environment, files, DNS or DB access.
    /// `Ok(None)` means standalone `--help`; extra arguments still fail validation.
    /// Validate required role/pool/attachment settings before opening any database.
    ///
    /// # Errors
    /// Rejects unknown flags, legacy URLs, invalid operands and extra arguments without
    /// echoing them. Bind accepts hostname:port, IPv4 and bracketed IPv6, including port 0.
    pub fn parse(
        mode: RemoteExecutable,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Option<Self>, ExecutableError> {
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        if args.len() == 1 && args[0] == "--help" {
            return Ok(None);
        }
        let invalid = |operand| {
            ExecutableError(format!(
                "invalid remote arguments: {operand}; {}",
                mode.usage()
            ))
        };
        let mut parsed = Self {
            env_file: None,
            max_tasks: 32,
            lease_ms: 5000,
            bind: "127.0.0.1:3000".into(),
        };
        let mut operands = Vec::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--env-file" {
                let path = args
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| invalid("--env-file requires PATH"))?;
                if parsed.env_file.is_some()
                    || path
                        .to_str()
                        .is_some_and(|value| value.starts_with('-') || value.contains("://"))
                {
                    return Err(invalid("invalid or duplicate --env-file"));
                }
                parsed.env_file = Some(path.into());
            } else {
                let operand = arg
                    .into_string()
                    .map_err(|_| invalid("operands must be UTF-8"))?;
                if operand.starts_with('-') || operand.contains("://") {
                    return Err(invalid("unsupported option or legacy credential operand"));
                }
                operands.push(operand);
            }
        }
        let max_operands = match mode {
            RemoteExecutable::Api => 1,
            RemoteExecutable::Worker => 2,
            RemoteExecutable::Combined => 3,
        };
        if operands.len() > max_operands {
            return Err(invalid("too many operands"));
        }
        let bind_index = match mode {
            RemoteExecutable::Api => Some(0),
            RemoteExecutable::Worker | RemoteExecutable::Combined => {
                if let Some(value) = operands.first() {
                    parsed.max_tasks = value
                        .parse()
                        .ok()
                        .filter(|value| *value > 0)
                        .ok_or_else(|| invalid("MAX_TASKS must be a positive integer"))?;
                }
                if let Some(value) = operands.get(1) {
                    parsed.lease_ms = value
                        .parse()
                        .ok()
                        .filter(|value| *value > 0)
                        .ok_or_else(|| invalid("LEASE_MS must be a positive integer"))?;
                }
                matches!(mode, RemoteExecutable::Combined).then_some(2)
            }
        };
        if let Some(bind) = bind_index.and_then(|index| operands.get(index)) {
            if !valid_bind(bind) {
                return Err(invalid("invalid BIND_ADDRESS"));
            }
            parsed.bind.clone_from(bind);
        }
        Ok(Some(parsed))
    }
}

fn valid_bind(bind: &str) -> bool {
    if bind.parse::<std::net::SocketAddr>().is_ok() {
        return true;
    }
    let Some((host, port)) = bind.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && !host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ":[]/@=\\?#".contains(c))
        && port.parse::<u16>().is_ok()
}

/// Closed operation categories: arbitrary raw error displays are never trusted.
#[derive(Clone, Copy, Debug)]
pub enum ExecutableOperation {
    Logging,
    Attachments,
    Database,
    Bind,
    Serve,
    Worker,
}

/// Safe in Display, Debug, tracing and Rust's boxed-error termination path.
/// No raw errors, input values or source chains can be attached.
#[derive(Debug)]
pub struct ExecutableError(String);
impl ExecutableError {
    pub fn operation(operation: ExecutableOperation) -> Self {
        Self(match operation {
            ExecutableOperation::Logging => "logging initialization failed; check logging settings",
            ExecutableOperation::Attachments => "attachment initialization failed; check attachment settings and access",
            ExecutableOperation::Database => "database operation failed; check configuration, availability and schema compatibility; if the schema is missing or incompatible, prepare it with the matching CLI using blkit db init or blkit db migrate",
            ExecutableOperation::Bind => "cannot bind server address; check BIND_ADDRESS and availability",
            ExecutableOperation::Serve => "API server failed",
            ExecutableOperation::Worker => "worker operation failed",
        }.into())
    }
}
impl fmt::Display for ExecutableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ExecutableError {}
impl From<PostgresConfigError> for ExecutableError {
    fn from(error: PostgresConfigError) -> Self {
        Self(error.to_string())
    }
}
impl From<PostgresConnectionError> for ExecutableError {
    fn from(error: PostgresConnectionError) -> Self {
        Self(error.to_string())
    }
}
