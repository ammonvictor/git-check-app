//! Portfolio-wide git inspection.
//!
//! The crate is UI-free on purpose: the CLI, the tray application and future
//! integrations all consume the same [`Portfolio`] value.

pub mod config;
pub mod discover;
pub mod error;
pub mod inspect;
pub mod metrics;

pub use config::{Config, Root};
pub use error::{Error, Result};
pub use inspect::{Backend, Depth, RepoStatus};
pub use metrics::Portfolio;

/// One complete inspection run over the configured roots.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub portfolio: Portfolio,
    pub duration_ms: u128,
    pub discovery_errors: Vec<String>,
}
