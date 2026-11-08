//! Configuration file handling.
//!
//! Location follows `directories::ProjectDirs` so the same code lands in
//! `~/.config/git-check/config.toml` (Linux), `~/Library/Application Support/
//! org.kwano.git-check/config.toml` (macOS) and `%APPDATA%\kwano\git-check\
//! config.toml` (Windows). `GIT_CHECK_CONFIG` overrides the path, which also
//! makes the CLI testable without touching the real config.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const APP_ORG: &str = "kwano";
pub const APP_NAME: &str = "git-check";

/// A folder to search for repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub path: PathBuf,
    /// How many directory levels below `path` are searched. 0 = only `path`
    /// itself, 1 = its immediate children, and so on.
    #[serde(default = "default_depth")]
    pub depth: usize,
    /// Optional display name (`work`, `oss`) used by the dashboard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

fn default_depth() -> usize {
    3
}

impl Root {
    pub fn new(path: impl Into<PathBuf>, depth: usize, label: Option<String>) -> Self {
        Self {
            path: path.into(),
            depth,
            label,
        }
    }

    pub fn display(&self) -> String {
        match &self.label {
            Some(label) => format!("{} ({label})", self.path.display()),
            None => self.path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_roots")]
    pub roots: Vec<Root>,
    /// How often a tray application rescans, in seconds.
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    /// Repos whose last commit is older than this are "cold".
    #[serde(default = "default_stale")]
    pub stale_after_days: i64,
    /// Directory names that are never descended into.
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
    /// Also search inside repositories (monorepos with vendored checkouts).
    #[serde(default)]
    pub follow_nested: bool,
    #[serde(default)]
    pub launch_at_login: bool,
}

fn default_interval() -> u64 {
    120
}

fn default_stale() -> i64 {
    30
}

fn default_exclude() -> Vec<String> {
    [
        "node_modules",
        ".venv",
        "venv",
        "vendor",
        "dist",
        "build",
        "target",
        ".tox",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_roots() -> Vec<Root> {
    let home = dirs_home();
    let candidates: Vec<PathBuf> = ["Documents/Projects", "Projects", "code", "src"]
        .iter()
        .map(|candidate| home.join(candidate))
        .filter(|path| path.is_dir())
        .collect();

    // A directory that exists but holds nothing is not a projects folder.
    // Seeding it silently produced an empty portfolio on machines that keep an
    // empty ~/Documents/Projects around; the tray now asks for a folder instead.
    let mut roots: Vec<Root> = candidates
        .iter()
        .filter(|path| !is_empty_directory(path))
        .map(|path| Root::new(path.clone(), 3, None))
        .collect();

    if roots.is_empty() && candidates.is_empty() {
        roots.push(Root::new(home, 2, None));
    }
    roots
}

fn is_empty_directory(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: default_roots(),
            interval_secs: default_interval(),
            stale_after_days: default_stale(),
            exclude: default_exclude(),
            follow_nested: false,
            launch_at_login: false,
        }
    }
}

impl Config {
    /// Resolve the config path: `GIT_CHECK_CONFIG`, else the platform dir.
    pub fn path() -> PathBuf {
        if let Some(custom) = std::env::var_os("GIT_CHECK_CONFIG") {
            return PathBuf::from(custom);
        }
        directories::ProjectDirs::from("org", APP_ORG, APP_NAME)
            .map(|dirs| dirs.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("git-check.toml"))
    }

    /// Directory for state that is not configuration (history DB, logs).
    pub fn state_dir() -> PathBuf {
        if let Some(custom) = std::env::var_os("GIT_CHECK_STATE") {
            return PathBuf::from(custom);
        }
        directories::ProjectDirs::from("org", APP_ORG, APP_NAME)
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// Load from `path`, or return defaults when the file does not exist.
    pub fn load_from(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(Error::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        toml::from_str(&text).map_err(|err| Error::Toml {
            path: path.to_path_buf(),
            message: err.to_string(),
        })
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = toml::to_string_pretty(self).map_err(|err| Error::Toml {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        let header =
            "# git-check configuration\n# roots: folders searched for git repositories\n\n";
        std::fs::write(path, format!("{header}{body}")).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path())
    }

    /// Expand a leading `~` and make the path absolute.
    pub fn expand(path: &Path) -> PathBuf {
        let raw = path.to_string_lossy();
        let expanded = if raw == "~" {
            dirs_home()
        } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
            dirs_home().join(rest)
        } else {
            path.to_path_buf()
        };
        std::path::absolute(&expanded).unwrap_or(expanded)
    }

    /// Roots that actually exist right now.
    pub fn existing_roots(&self) -> Vec<Root> {
        self.roots
            .iter()
            .filter(|root| Self::expand(&root.path).is_dir())
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_directories_are_recognised() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_empty_directory(dir.path()));
        std::fs::write(dir.path().join("file"), b"x").unwrap();
        assert!(!is_empty_directory(dir.path()));
    }

    #[test]
    fn missing_directories_are_not_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_empty_directory(&dir.path().join("nope")));
    }
}
