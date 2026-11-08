//! Per-repository inspection.

pub mod cli;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

/// Which implementation reads the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    /// Shell out to `git`; matches what the shell sees, needs no C toolchain.
    #[default]
    Cli,
    /// In-process libgit2; much faster on Windows, needs `cmake` to build.
    Libgit2,
}

impl Backend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Backend::Cli => "cli",
            Backend::Libgit2 => "libgit2",
        }
    }
}

/// How much work to do per repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// Working-tree state and ahead/behind only: enough for a tray icon.
    Quick,
    /// Adds last commit, 90-day commit history, stashes and remote presence.
    Full,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanCtx {
    pub now: i64,
    /// Lower bound (unix seconds) for the commit-history window.
    pub since: i64,
    pub depth: Depth,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RepoStatus {
    pub path: PathBuf,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub stashes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_stash_days: Option<i64>,
    pub detached: bool,
    pub has_remote: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_commit_ts: Option<i64>,
    pub last_subject: String,
    #[serde(skip)]
    pub commit_ts: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RepoStatus {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            ..Default::default()
        }
    }

    /// Changed paths: index, worktree, untracked and conflicts.
    pub fn dirty(&self) -> usize {
        self.staged + self.unstaged + self.untracked + self.conflicted
    }

    pub fn has_upstream(&self) -> bool {
        self.upstream.is_some()
    }

    pub fn diverged(&self) -> bool {
        self.ahead > 0 && self.behind > 0
    }

    /// Days since the last commit; `None` for an unborn branch.
    pub fn age_days(&self, now: i64) -> Option<i64> {
        self.last_commit_ts.map(|ts| (now - ts).max(0) / 86_400)
    }

    pub fn is_cold(&self, now: i64, stale_after_days: i64) -> bool {
        self.age_days(now)
            .map(|d| d > stale_after_days)
            .unwrap_or(false)
    }

    /// Ranking weight used to order "needs attention" rows.
    pub fn score(&self) -> u64 {
        let mut score = 0u64;
        score += self.conflicted as u64 * 100;
        score += (self.dirty().min(50) as u64) * 4;
        score += (self.ahead.min(20) as u64) * 3;
        score += (self.behind.min(20) as u64) * 2;
        score += if self.detached { 5 } else { 0 };
        score += if self.error.is_some() { 2 } else { 0 };
        score
    }

    /// Human-readable problems, in priority order.
    pub fn flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        if self.conflicted > 0 {
            flags.push(format!("{} conflict", self.conflicted));
        }
        if self.dirty() > 0 {
            flags.push(format!("{} dirty", self.dirty()));
        }
        if self.ahead > 0 {
            flags.push(format!("{} ahead", self.ahead));
        }
        if self.behind > 0 {
            flags.push(format!("{} behind", self.behind));
        }
        if self.stashes > 0 {
            flags.push(format!("{} stashed", self.stashes));
        }
        if self.detached {
            flags.push("detached".to_string());
        }
        if self.error.is_some() {
            flags.push("error".to_string());
        }
        flags
    }

    pub fn state(&self) -> &'static str {
        if self.error.is_some() {
            "error"
        } else if self.conflicted > 0 {
            "conflict"
        } else if self.dirty() > 0 {
            "dirty"
        } else if self.ahead > 0 {
            "unpushed"
        } else if self.behind > 0 {
            "behind"
        } else if self.detached {
            "detached"
        } else {
            "clean"
        }
    }
}

/// A source of [`RepoStatus`] values.
pub trait Inspector: Send + Sync {
    fn inspect(&self, path: &Path, ctx: &ScanCtx) -> RepoStatus;
}

pub fn inspector(backend: Backend) -> Result<Box<dyn Inspector>> {
    match backend {
        Backend::Cli => Ok(Box::new(cli::CliInspector)),
        Backend::Libgit2 => Err(Error::Unsupported {
            what: "the libgit2 backend is planned for M1 and not part of this build; \
                   use --backend cli"
                .to_string(),
        }),
    }
}

/// Inspect every repository, in parallel.
pub fn inspect_all(repos: &[PathBuf], inspector: &dyn Inspector, ctx: &ScanCtx) -> Vec<RepoStatus> {
    use rayon::prelude::*;
    let mut statuses: Vec<RepoStatus> = repos
        .par_iter()
        .map(|path| inspector.inspect(path, ctx))
        .collect();
    statuses.sort_by_key(|s| s.name.to_lowercase());
    statuses
}
