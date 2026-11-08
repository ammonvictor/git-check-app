//! `git` command-line backend.
//!
//! Uses `--no-optional-locks` (git >= 2.15) so inspection never competes with an
//! editor or a running command for `index.lock`, and `--porcelain=v2 --branch`
//! so ahead/behind come from the same call as the working-tree state.

use std::path::Path;
use std::process::Command;

use super::{Depth, Inspector, RepoStatus, ScanCtx};

pub struct CliInspector;

fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("--no-optional-locks")
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
        })?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(err.trim().to_string());
    }
    Ok(output.stdout)
}

fn iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

impl Inspector for CliInspector {
    fn inspect(&self, path: &Path, ctx: &ScanCtx) -> RepoStatus {
        let mut status = RepoStatus::new(path);

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
