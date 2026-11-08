//! Integration tests against real repositories created with the `git` binary.
//!
//! Git is a hard requirement of the tool, and every CI runner ships it, so the
//! fixtures are built by git rather than by mocking. Tests skip themselves with
//! a message if git is absent.

use std::path::{Path, PathBuf};
use std::process::Command;

use git_check_core::config::{Config, Root};
use git_check_core::discover;
use git_check_core::inspect::{inspector, Backend, Depth, ScanCtx};
use git_check_core::metrics::Portfolio;

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn commit(dir: &Path, file: &str, content: &str, message: &str) {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", message]);
}

/// A repo with one commit and a local bare remote it has been pushed to.
fn repo_with_remote(dir: &Path) -> PathBuf {
    let remote = dir.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "--bare", "-b", "main"]);

    let repo = dir.join("proj");
    init_repo(&repo);
    commit(&repo, "README.md", "hello\n", "initial");
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    repo
}

fn inspect_one(repo: &Path) -> git_check_core::inspect::RepoStatus {
    let ins = inspector(Backend::Cli).unwrap();
    let now = chrono::Utc::now().timestamp();
    let ctx = ScanCtx {
        now,
        since: now - 366 * 86_400,
        depth: Depth::Full,
    };
    ins.inspect(repo, &ctx)
}

fn scan_dirs(roots: &[Root]) -> Vec<String> {
    let cfg = Config {
        roots: roots.to_vec(),
        ..Config::default()
    };
    let found = discover::discover(&cfg).expect("discovery works");
    found
        .repos
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn clean_repo_reports_clean() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_remote(tmp.path());
    let status = inspect_one(&repo);
    assert!(status.error.is_none(), "error: {:?}", status.error);
    assert_eq!(status.dirty(), 0);
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!(status.ahead, 0);
    assert_eq!(status.behind, 0);
    assert!(status.has_remote);
    assert!(status.last_commit_ts.is_some());
    assert_eq!(status.last_subject, "initial");
    assert_eq!(status.state(), "clean");
}

#[test]
fn dirty_counts_staged_unstaged_and_untracked() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_remote(tmp.path());

    commit(&repo, "README.md", "changed\n", "second");
    // Modify the tracked file again so the worktree has an unstaged change.
    std::fs::write(repo.join("README.md"), "modified after commit\n").unwrap();
    std::fs::write(repo.join("staged.txt"), "x\n").unwrap();
    git(&repo, &["add", "staged.txt"]);
    std::fs::write(repo.join("untracked.txt"), "y\n").unwrap();

    let status = inspect_one(&repo);
    assert_eq!(status.staged, 1, "one staged file");
    assert_eq!(status.unstaged, 1, "README.md modified in worktree");
    assert_eq!(status.untracked, 1, "untracked.txt");
    assert_eq!(status.dirty(), 3);
    assert_eq!(status.state(), "dirty");
}

#[test]
fn ahead_and_behind_follow_the_upstream() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_remote(tmp.path());

    commit(&repo, "README.md", "ahead\n", "local work");
    let status = inspect_one(&repo);
    assert_eq!(status.ahead, 1);
    assert_eq!(status.behind, 0);
    assert_eq!(status.state(), "unpushed");

    // Push the local commit, then rewind: the remote now has work we lack.
    git(&repo, &["push", "-q", "origin", "main"]);
    git(&repo, &["reset", "--hard", "-q", "HEAD~1"]);
    let status = inspect_one(&repo);
    assert_eq!(status.ahead, 0, "local rewound to the pushed commit");
    assert_eq!(status.behind, 1, "remote is one commit ahead of us");
    assert_eq!(status.state(), "behind");
}

#[test]
fn detached_head_is_flagged_and_has_no_upstream() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_remote(tmp.path());
    git(&repo, &["checkout", "-q", "--detach"]);

    let status = inspect_one(&repo);
    assert!(status.detached, "detached HEAD must be detected");
    assert!(status.branch.is_none());
    assert!(!status.has_upstream());
    assert!(status.flags().contains(&"detached".to_string()));
}

#[test]
fn discovery_respects_depth_and_excludes() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    init_repo(&root.join("top"));
    init_repo(&root.join("group").join("nested"));
    init_repo(&root.join("node_modules").join("ignored"));
    init_repo(&root.join("deep").join("a").join("b").join("too-far"));

    let depth3 = scan_dirs(&[Root::new(root.clone(), 3, None)]);
    assert!(depth3.contains(&"top".to_string()));
    assert!(depth3.contains(&"nested".to_string()));
    assert!(
        !depth3.contains(&"ignored".to_string()),
        "node_modules pruned"
    );
    assert!(
        !depth3.contains(&"too-far".to_string()),
        "depth 3 excludes 4 levels"
    );

    let depth1 = scan_dirs(&[Root::new(root.clone(), 1, None)]);
    assert_eq!(depth1, vec!["top".to_string()]);

    let deeper = scan_dirs(&[Root::new(root, 4, None)]);
    assert!(deeper.contains(&"too-far".to_string()));
}

#[test]
fn linked_worktree_with_git_file_is_discovered() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_remote(tmp.path());
    let worktree = tmp.path().join("wt");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );

    // The worktree checkout has a `.git` *file*, not a directory.
    assert!(worktree.join(".git").is_file());
    let found = scan_dirs(&[Root::new(tmp.path().to_path_buf(), 2, None)]);
    assert!(
        found.contains(&"wt".to_string()),
        "worktree must be discovered"
    );

    let status = inspect_one(&worktree);
    assert!(
        status.error.is_none(),
        "worktree inspect failed: {:?}",
        status.error
    );
    assert_eq!(status.branch.as_deref(), Some("feature"));
}

#[test]
fn portfolio_counters_and_day_alignment() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_remote(tmp.path());
    commit(&repo, "README.md", "second\n", "second");

    let status = inspect_one(&repo);
    let now = chrono::Utc::now().timestamp();
    let portfolio = Portfolio::new(vec![status], now, 30);
    let counters = portfolio.counters();

    assert_eq!(counters.repos, 1);
    assert_eq!(counters.ahead_commits, 1);
    assert_eq!(counters.dirty_repos, 0);
    // The headline count and the chart must be the same window.
    assert_eq!(
        counters.commits_30d,
        portfolio.daily_commits(30).iter().sum::<usize>()
    );
    assert_eq!(
        counters.commits_7d,
        portfolio.daily_commits(7).iter().sum::<usize>()
    );
    assert!(counters.commits_7d >= 2, "two commits today");
    assert_eq!(portfolio.tier(), git_check_core::metrics::Tier::Warn);
}
