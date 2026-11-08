//! Repository discovery.
//!
//! Traversal rules:
//! * `depth` counts directory levels *below* the root (root itself = 0).
//! * A directory is a repository when it contains `.git`, as a **directory**
//!   (normal checkout) or a **file** (linked worktree / submodule checkout).
//! * Excluded directory names and `node_modules`-style noise are pruned.
//! * Symbolic links are not followed, which rules out cycles.
//! * Once a repository is found, its subtree is not searched further unless
//!   `follow_nested` is set.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{Config, Root};
use crate::error::Result;

#[derive(Debug, Default, Clone)]
pub struct Discovery {
    pub repos: Vec<PathBuf>,
    /// Paths that could not be read, with the reason. Never fatal.
    pub errors: Vec<String>,
}

impl Discovery {
    pub fn is_empty(&self) -> bool {
        self.repos.is_empty()
    }
}

/// Walk every root and collect repositories, sorted by name.
pub fn discover(config: &Config) -> Result<Discovery> {
    let roots = config.existing_roots();
    discover_roots(&roots, &config.exclude, config.follow_nested)
}

pub fn discover_roots(
    roots: &[Root],
    exclude: &[String],
    follow_nested: bool,
) -> Result<Discovery> {
    if roots.is_empty() {
        return Err(crate::error::Error::NoRoots {
            tried: "any configured root".to_string(),
        });
    }

    let excluded: HashSet<&str> = exclude.iter().map(String::as_str).collect();
    let mut out = Discovery::default();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for root in roots {
        let base = Config::expand(&root.path);
        if !base.is_dir() {
            out.errors
                .push(format!("{}: not a directory, skipped", base.display()));
            continue;
        }
        let mut stack: Vec<(PathBuf, usize)> = vec![(base, 0)];

        while let Some((dir, level)) = stack.pop() {
            if level > root.depth {
                continue;
            }
            if is_repo(&dir) {
                let key = dir.canonicalize().unwrap_or_else(|_| dir.clone());
                if seen.insert(key) {
                    out.repos.push(dir.clone());
                }
                if !follow_nested {
                    continue;
                }
            }
            if level == root.depth {
                continue;
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(err) => {
                    out.errors.push(format!("{}: {err}", dir.display()));
                    continue;
                }
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') && name != "." {
                    // Hidden directories are scanned (some people keep repos in
                    // .config), but never the git database itself.
                    if name == ".git" {
                        continue;
                    }
                }
                if excluded.contains(name.as_ref()) {
                    continue;
                }
                let path = entry.path();
                match entry.file_type() {
                    Ok(ft) if ft.is_dir() => stack.push((path, level + 1)),
                    Ok(ft) if ft.is_symlink() => {}
                    _ => {}
                }
            }
        }
    }

    out.repos.sort_by_key(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    Ok(out)
}

/// A repository is any directory holding `.git` (dir or file).
pub fn is_repo(dir: &Path) -> bool {
    let git = dir.join(".git");
    match std::fs::symlink_metadata(&git) {
        Ok(meta) => meta.is_dir() || meta.is_file(),
        Err(_) => false,
    }
}
