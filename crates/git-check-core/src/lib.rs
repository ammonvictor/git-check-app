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
pub use inspect::{Backend, Commit, Depth, RepoStatus};
pub use metrics::Portfolio;

/// One complete inspection run over the configured roots.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub portfolio: Portfolio,
    pub duration_ms: u128,
    pub discovery_errors: Vec<String>,
}

/// Run one complete inspection over the configured roots.
///
/// This is the single entry point the CLI and the tray application share:
/// discovery, parallel inspection and portfolio aggregation in one call.
pub fn scan(config: &config::Config, backend: Backend, depth: Depth) -> Result<Snapshot> {
    let started = std::time::Instant::now();
    let discovery = discover::discover(config)?;
    let inspector = inspect::inspector(backend)?;
    let now = chrono::Utc::now().timestamp();
    let ctx = inspect::ScanCtx {
        now,
        since: now - 366 * 86_400,
        depth,
    };
    let repos = inspect::inspect_all(&discovery.repos, inspector.as_ref(), &ctx);
    Ok(Snapshot {
        portfolio: Portfolio::new(repos, now, config.stale_after_days),
        duration_ms: started.elapsed().as_millis(),
        discovery_errors: discovery.errors,
    })
}
