use std::time::Duration;

// Compile the CLI's canonical preparation helper in core's unit tests too.
#[cfg(all(test, feature = "remote-persistence"))]
extern crate self as blkit_core;
#[cfg(all(test, feature = "remote-persistence"))]
#[path = "../../blkit-cli/tests/support/postgres.rs"]
mod postgres_preparation;

#[cfg(all(
    any(feature = "attachment-local", feature = "attachment-cloud"),
    any(feature = "local-persistence", feature = "remote-persistence")
))]
pub mod attachment_reader;
#[cfg(any(feature = "attachment-local", feature = "attachment-cloud"))]
pub mod attachment_store;
pub mod compiled_graph;
pub mod dictionary;
#[cfg(feature = "remote-persistence")]
pub mod distributed;
pub mod evaluation;
pub mod list_ops;
#[cfg(feature = "logging")]
pub mod logging;
#[cfg(all(
    feature = "api-server",
    any(feature = "attachment-local", feature = "attachment-cloud")
))]
pub mod multipart;
pub mod number_ops;
#[cfg(feature = "remote-persistence")]
pub mod postgres_config;
#[cfg(feature = "remote-persistence")]
pub mod postgres_connection;
#[cfg(feature = "remote-persistence")]
pub mod postgres_schema;
#[cfg(feature = "remote-persistence")]
pub mod postgres_store;
#[cfg(feature = "remote-persistence")]
pub mod remote_executable;
#[cfg(feature = "worker")]
pub mod runtime;
#[cfg(all(
    feature = "api-server",
    any(
        feature = "remote-persistence",
        all(feature = "worker", feature = "local-persistence")
    )
))]
pub mod server;
mod store;
pub use store::Attachment;
pub mod string_ops;
pub mod table;
pub mod temporal;
pub use table::{Cell, CellType, Table};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub retry_for: Duration,
    pub retry_delay: Duration,
    pub backoff: &'static str,
}

#[derive(Debug)]
pub struct DeadlinePolicy {
    pub origin: &'static str,
    pub duration: Duration,
}
