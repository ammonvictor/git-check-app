//! Portfolio-level aggregation.

use serde::Serialize;

use crate::inspect::RepoStatus;

/// Health tier driving the tray icon colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Ok,
    Warn,
    Alert,
}

impl Tier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::Ok => "ok",
            Tier::Warn => "warn",
            Tier::Alert => "alert",
        }
    }
}

/// Counters shown in the tray popup and the dashboard header.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Counters {
    pub repos: usize,
    pub dirty_repos: usize,
    pub dirty_files: usize,
    pub ahead_repos: usize,
    pub ahead_commits: usize,
    pub behind_repos: usize,
    pub behind_commits: usize,
    pub conflicts: usize,
    pub stashes: usize,
    pub no_upstream: usize,
    pub detached: usize,
    pub errors: usize,
    pub cold: usize,
    pub diverged: usize,
    pub commits_7d: usize,
    pub commits_30d: usize,
    pub active_repos_7d: usize,
}

#[derive(Debug, Clone)]
pub struct Portfolio {
    pub repos: Vec<RepoStatus>,
    pub generated_at: i64,
    pub stale_after_days: i64,
}

impl Portfolio {
    pub fn new(repos: Vec<RepoStatus>, generated_at: i64, stale_after_days: i64) -> Self {
        Self {
            repos,
            generated_at,
            stale_after_days,
        }
    }

    pub fn counters(&self) -> Counters {
        let mut c = Counters {
            repos: self.repos.len(),
            ..Default::default()
        };
        for repo in &self.repos {
            if repo.dirty() > 0 {
                c.dirty_repos += 1;
            }
            c.dirty_files += repo.dirty();
            if repo.ahead > 0 {
                c.ahead_repos += 1;
            }
            c.ahead_commits += repo.ahead;
            if repo.behind > 0 {
                c.behind_repos += 1;
            }
            c.behind_commits += repo.behind;
            if repo.conflicted > 0 {
                c.conflicts += 1;
            }
            c.stashes += repo.stashes;
            if !repo.has_upstream() {
                c.no_upstream += 1;
            }
            if repo.detached {
                c.detached += 1;
            }
            if repo.error.is_some() {
                c.errors += 1;
            }
            if repo.is_cold(self.generated_at, self.stale_after_days) {
                c.cold += 1;
            }
            if repo.diverged() {
                c.diverged += 1;
            }
        }
        c.commits_7d = self.commits_since(7);
        c.commits_30d = self.commits_since(30);
        c.active_repos_7d = self.active_repos(7);
        c
    }

    /// Start of the window covering the last `days` whole UTC days (today included).
    fn day_cutoff(&self, days: i64) -> i64 {
        let today = self.generated_at.div_euclid(86_400);
        (today - (days - 1).max(0)) * 86_400
    }

    /// Commits reachable from HEAD within the last `days` whole UTC days.
    ///
    /// Day-aligned on purpose: `commits_since(n) == daily_commits(n).sum()`, so
    /// the headline number always matches the chart underneath it.
    pub fn commits_since(&self, days: i64) -> usize {
        let cutoff = self.day_cutoff(days);
        self.repos
            .iter()
            .flat_map(|r| r.commit_ts.iter())
            .filter(|ts| **ts >= cutoff)
            .count()
    }

    pub fn active_repos(&self, days: i64) -> usize {
        let cutoff = self.day_cutoff(days);
        self.repos
            .iter()
            .filter(|r| r.commit_ts.iter().any(|ts| *ts >= cutoff))
            .count()
    }

    /// Commits per UTC day for the last `days` days, oldest first.
    pub fn daily_commits(&self, days: usize) -> Vec<usize> {
        let days = days.max(1);
        let today = self.generated_at.div_euclid(86_400);
        let start = today - (days as i64 - 1);
        let mut buckets = vec![0usize; days];
        for repo in &self.repos {
            for ts in &repo.commit_ts {
                let day = ts.div_euclid(86_400);
                if day >= start {
                    let idx = (day - start) as usize;
                    if idx < days {
                        buckets[idx] += 1;
                    }
                }
            }
        }
        buckets
    }

    /// Consecutive days with at least one commit, ending today (or yesterday
    /// when today is still empty). Bounded by the scan window.
    pub fn streak(&self, window_days: usize) -> usize {
        let buckets = self.daily_commits(window_days);
        let mut run = 0usize;
        for (idx, count) in buckets.iter().rev().enumerate() {
            if *count == 0 {
                // A single empty day at the very end (today) does not break it.
                if idx == 0 {
                    continue;
                }
                break;
            }
            run += 1;
        }
        run
    }

    /// Problem repositories, worst first.
    pub fn ranked(&self) -> Vec<&RepoStatus> {
        let mut rows: Vec<&RepoStatus> = self.repos.iter().filter(|r| r.score() > 0).collect();
        rows.sort_by(|a, b| b.score().cmp(&a.score()).then_with(|| a.name.cmp(&b.name)));
        rows
    }

    pub fn tier(&self) -> Tier {
        let c = self.counters();
        if c.conflicts > 0 || c.errors > 0 {
            Tier::Alert
        } else if c.dirty_repos > 0 || c.ahead_repos > 0 {
            Tier::Warn
        } else {
            Tier::Ok
        }
    }

    /// Full machine-readable form: counters, trend and every repo.
    pub fn to_json(&self, duration_ms: u128, discovery_errors: &[String]) -> serde_json::Value {
        let counters = self.counters();
        // Derived values (dirty, state, age, cold) are computed here so the JSON
        // carries the same view the report and the tray use, not raw internals.
        let repos: Vec<serde_json::Value> = self
            .repos
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "path": r.path,
                    "branch": r.branch,
                    "upstream": r.upstream,
                    "ahead": r.ahead,
                    "behind": r.behind,
                    "staged": r.staged,
                    "unstaged": r.unstaged,
                    "untracked": r.untracked,
                    "dirty": r.dirty(),
                    "conflicted": r.conflicted,
                    "stashes": r.stashes,
                    "oldest_stash_days": r.oldest_stash_days,
                    "detached": r.detached,
                    "has_remote": r.has_remote,
                    "head": r.head,
                    "last_commit_ts": r.last_commit_ts,
                    "last_subject": r.last_subject,
                    "age_days": r.age_days(self.generated_at),
                    "cold": r.is_cold(self.generated_at, self.stale_after_days),
                    "state": r.state(),
                    "error": r.error,
                })
            })
            .collect();

        serde_json::json!({
            "generated_at": self.generated_at,
            "duration_ms": duration_ms,
            "counters": counters,
            "tier": self.tier().as_str(),
            "trend_30d": self.daily_commits(30),
            "streak_days": self.streak(366),
            "discovery_errors": discovery_errors,
            "repos": repos,
        })
    }
}
