//! `git` command-line backend.
//!
//! Uses `--no-optional-locks` (git >= 2.15) so inspection never competes with an
//! editor or a running command for `index.lock`, and `--porcelain=v2 --branch`
//! so ahead/behind come from the same call as the working-tree state.
//!
//! Repository-local config is untrusted. `HARDENING` disables the fsmonitor
//! hook and signature verification for every call, and a repository whose
//! local config defines a content filter is reported and not inspected:
//! `git status` and `git log` would otherwise run a program from the repo.

use std::path::Path;
use std::process::Command;

use super::{Depth, Inspector, RepoStatus, ScanCtx};

pub struct CliInspector;

/// `-c` overrides applied to every git call; command-line config beats repo config.
///
/// `core.fsmonitor` names a program that `git status` executes to learn which
/// files changed, and `log.showSignature` makes `git log` run `gpg.program`.
/// Anyone who can write a repository's `.git/config` (an unpacked archive, a
/// shared drive) could otherwise run code on every tray rescan.
const HARDENING: &[&str] = &[
    "-c",
    "core.fsmonitor=false",
    "-c",
    "log.showSignature=false",
];

fn git_output(path: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("--no-optional-locks")
        .args(HARDENING)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                "git executable not found".to_string()
            } else {
                err.to_string()
            }
        })
}

fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = git_output(path, args)?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(err.trim().to_string());
    }
    Ok(output.stdout)
}

/// Keys that can make `git status` run a program, plus the switch that moves
/// them into a per-worktree config file.
const FILTER_KEY_RE: &str = r"^(filter\..*\.(clean|smudge|process)|extensions\.worktreeConfig)$";

/// Read matching keys from one config scope. An empty result means no key
/// matched; any other failure is returned so the caller can refuse the repo.
fn config_entries(path: &Path, scope: &str) -> Result<Vec<(String, String)>, String> {
    let output = git_output(
        path,
        &["config", scope, "--includes", "--get-regexp", FILTER_KEY_RE],
    )?;
    match output.status.code() {
        // 1 is "no key matched", which is the normal case.
        Some(0) => {}
        Some(1) => return Ok(Vec::new()),
        _ => {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(err.trim().to_string());
        }
    }

    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if !key.is_empty() {
            entries.push((key.to_string(), value.trim().to_string()));
        }
    }
    Ok(entries)
}

/// Filter driver names in a set of config entries.
fn filter_names(entries: &[(String, String)]) -> Vec<String> {
    let mut names = Vec::new();
    for (key, _) in entries {
        let Some(rest) = key.strip_prefix("filter.") else {
            continue;
        };
        let Some((name, kind)) = rest.rsplit_once('.') else {
            continue;
        };
        if !matches!(kind, "clean" | "smudge" | "process") {
            continue;
        }
        if !names.iter().any(|seen| seen == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// Content-filter drivers named in the repository's own config.
///
/// `git status` runs a filter's `clean` (or `process`) command when it has to
/// compare a file whose stat information is stale ("racily clean"). The command
/// comes from the repository's `.git/config`, so a repository could otherwise
/// run code just by being scanned. `--includes` matters: a file pulled in with
/// `include.path` is just as untrusted as the main config. So is
/// `config.worktree`, which git reads instead of the common config when
/// `extensions.worktreeConfig` is enabled.
fn local_filter_drivers(path: &Path) -> Result<Vec<String>, String> {
    let local = config_entries(path, "--local")?;
    // Git treats an empty value as true; only an explicit false turns the
    // extension off. Treat anything unrecognised as on, conservatively.
    let worktree_config = local.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("extensions.worktreeconfig")
            && !matches!(
                value.to_ascii_lowercase().as_str(),
                "false" | "no" | "off" | "0"
            )
    });

    let mut names = filter_names(&local);
    if worktree_config {
        for name in filter_names(&config_entries(path, "--worktree")?) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    Ok(names)
}

fn iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

impl Inspector for CliInspector {
    fn inspect(&self, path: &Path, ctx: &ScanCtx) -> RepoStatus {
        let mut status = RepoStatus::new(path);

        // Refuse rather than run: git would execute the filter while comparing
        // this repository's files, and a filter command is arbitrary code.
        match local_filter_drivers(path) {
            Ok(drivers) if !drivers.is_empty() => {
                status.error = Some(format!(
                    "repository config defines content filter{} ({}); not inspected",
                    if drivers.len() == 1 { "" } else { "s" },
                    drivers.join(", ")
                ));
                return status;
            }
            Ok(_) => {}
            Err(err) => {
                status.error = Some(err);
                return status;
            }
        }

        let raw = match git(
            path,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=normal",
            ],
        ) {
            Ok(raw) => raw,
            Err(err) => {
                status.error = Some(err);
                return status;
            }
        };

        for record in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            if let Some(line) = record.strip_prefix(b"# ") {
                if let Some(value) = line.strip_prefix(b"branch.oid ") {
                    let value = String::from_utf8_lossy(value);
                    if value != "(initial)" {
                        status.head = Some(value.into_owned());
                    }
                } else if let Some(value) = line.strip_prefix(b"branch.head ") {
                    let value = String::from_utf8_lossy(value);
                    if value == "(detached)" {
                        status.detached = true;
                        status.branch = None;
                    } else {
                        status.branch = Some(value.into_owned());
                    }
                } else if let Some(value) = line.strip_prefix(b"branch.upstream ") {
                    status.upstream = Some(String::from_utf8_lossy(value).into_owned());
                } else if let Some(value) = line.strip_prefix(b"branch.ab ") {
                    let text = String::from_utf8_lossy(value);
                    let mut parts = text.split_whitespace();
                    if let Some(ahead) = parts.next().and_then(|v| v.strip_prefix('+')) {
                        status.ahead = ahead.parse().unwrap_or(0);
                    }
                    if let Some(behind) = parts.next().and_then(|v| v.strip_prefix('-')) {
                        status.behind = behind.parse().unwrap_or(0);
                    }
                }
                continue;
            }

            match record.first().copied() {
                Some(b'1') | Some(b'2') if record.len() >= 4 => {
                    let xy = &record[2..4];
                    if xy[0] != b'.' {
                        status.staged += 1;
                    }
                    if xy[1] != b'.' {
                        status.unstaged += 1;
                    }
                }
                Some(b'u') => status.conflicted += 1,
                Some(b'?') => status.untracked += 1,
                _ => {}
            }
        }

        if ctx.depth == Depth::Quick {
            return status;
        }

        if let Ok(stdout) = git(path, &["log", "-1", "--format=%ct%x00%s", "HEAD"]) {
            let mut parts = stdout.splitn(2, |b| *b == 0);
            if let Some(ts) = parts.next() {
                if let Ok(text) = std::str::from_utf8(ts) {
                    status.last_commit_ts = text.trim().parse().ok();
                }
            }
            if let Some(subject) = parts.next() {
                status.last_subject = String::from_utf8_lossy(subject)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }

        if let Some(ts) = status.last_commit_ts {
            let since = iso(ctx.since.min(ts));
            if let Ok(stdout) = git(path, &["log", &format!("--since={since}"), "--format=%ct"]) {
                status.commit_ts = String::from_utf8_lossy(&stdout)
                    .lines()
                    .filter_map(|line| line.trim().parse::<i64>().ok())
                    .collect();
            }
        }

        if let Ok(stdout) = git(path, &["stash", "list", "--format=%ct"]) {
            let stamps: Vec<i64> = String::from_utf8_lossy(&stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<i64>().ok())
                .collect();
            status.stashes = stamps.len();
            status.oldest_stash_days = stamps
                .iter()
                .min()
                .map(|oldest| (ctx.now - oldest).max(0) / 86_400);
        }

        if let Ok(stdout) = git(path, &["remote"]) {
            status.has_remote = !String::from_utf8_lossy(&stdout).trim().is_empty();
        }

        status
    }
}
