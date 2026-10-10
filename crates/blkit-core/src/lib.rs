use std::time::Duration;

pub mod compiled_graph;
pub mod dictionary;
#[cfg(feature = "remote-persistence")]
pub mod distributed;
pub mod evaluation;
#[cfg(feature = "logging")]
pub mod logging;
pub mod number_ops;
#[cfg(feature = "remote-persistence")]
pub mod postgres_store;
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
pub mod string_ops;
pub mod temporal;

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
