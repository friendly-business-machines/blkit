#![cfg(feature = "remote-persistence")]

// Compile the same preparation contract against core's test-only dependencies.
#[path = "../../blkit-cli/tests/postgres_preparation.rs"]
mod canonical_preparation;
