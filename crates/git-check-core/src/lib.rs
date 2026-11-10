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
pub use inspect::{Attention, Backend, Commit, Depth, RepoStatus};
pub use metrics::{Portfolio, Tier};

/// One complete inspection run over the configured roots.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub portfolio: Portfolio,
    pub duration_ms: u128,
    pub discovery_errors: Vec<String>,
}

impl Snapshot {
    /// Overall tier, including paths that could not be read.
    pub fn tier(&self) -> Tier {
        if self.discovery_errors.is_empty() {
            self.portfolio.tier()
        } else {
            Tier::Alert
        }
    }

    /// Whether `--exit-code` should fail and the tray should alert.
    pub fn needs_attention(&self) -> bool {
        !self.discovery_errors.is_empty() || self.portfolio.counters().needs_attention()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(discovery_errors: Vec<String>) -> Snapshot {
        Snapshot {
            portfolio: Portfolio::new(Vec::new(), 0, 30),
            duration_ms: 0,
            discovery_errors,
        }
    }

    #[test]
    fn discovery_errors_make_the_tier_alert() {
        let snapshot = snapshot(vec!["/secret: permission denied".to_string()]);
        assert_eq!(snapshot.tier(), Tier::Alert);
        assert!(snapshot.needs_attention());
    }

    #[test]
    fn a_clean_empty_portfolio_needs_nothing() {
        let snapshot = snapshot(Vec::new());
        assert_eq!(snapshot.tier(), Tier::Ok);
        assert!(!snapshot.needs_attention());
    }
}
