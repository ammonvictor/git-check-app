//! The report's user-visible contract, exercised through the built binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn configure(dir: &Path) {
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn init(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    configure(dir);
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

fn commit_dated(dir: &Path, file: &str, content: &str, message: &str, date: &str) {
    let path = dir.join(file);
    std::fs::write(path, content).unwrap();
    git(dir, &["add", "-A"]);
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit", "-qm", message])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .status()
        .expect("run git commit");
    assert!(status.success(), "dated commit failed");
}

fn config_file(dir: &Path) -> PathBuf {
    let path = dir.join("config.toml");
    std::fs::write(&path, "roots = []\n").unwrap();
    path
}

fn run(args: &[&str], config: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-check"))
        .args(args)
        .env("GIT_CHECK_CONFIG", config)
        .env("NO_COLOR", "1")
        .output()
        .expect("run git-check")
}

#[test]
fn quiet_repos_are_listed_by_name() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");

    let cold = root.join("cold-repo");
    init(&cold);
    commit_dated(&cold, "a.txt", "old\n", "old", "2026-01-01T00:00:00Z");

    let stashed = root.join("stash-repo");
    init(&stashed);
    commit(&stashed, "a.txt", "new\n", "init");
    std::fs::write(stashed.join("a.txt"), "changed\n").unwrap();
    git(&stashed, &["stash", "-q"]);

    let config = config_file(tmp.path());
    let out = run(
        &[
            "--root",
            root.to_str().unwrap(),
            "--depth",
            "1",
            "--no-color",
        ],
        &config,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("quiet"), "{stdout}");
    assert!(stdout.contains("cold-repo"), "{stdout}");
    assert!(stdout.contains("stash-repo"), "{stdout}");
    assert!(stdout.contains("cold "), "{stdout}");
    assert!(stdout.contains("stashed"), "{stdout}");
    assert!(stdout.contains("Nothing needs attention."), "{stdout}");
}

#[test]
fn exit_code_fails_for_an_unreadable_repo() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let broken = root.join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join(".git"), "gitdir: /nonexistent/gitdir\n").unwrap();

    let config = config_file(tmp.path());
    let args = [
        "--root",
        root.to_str().unwrap(),
        "--depth",
        "1",
        "--no-color",
    ];
    let plain = run(&args, &config);
    assert!(plain.status.success());

    let mut strict_args = args.to_vec();
    strict_args.push("--exit-code");
    let strict = run(&strict_args, &config);
    assert_eq!(strict.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&strict.stdout);
    assert!(stdout.contains("unreadable"), "{stdout}");
    assert!(stdout.contains("need attention"), "{stdout}");
}

#[test]
fn header_counts_repos_not_commits() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    git(
        tmp.path(),
        &["init", "-q", "--bare", "-b", "main", "remote.git"],
    );
    git(tmp.path(), &["clone", "-q", "remote.git", "work"]);
    let work = tmp.path().join("work");
    configure(&work);
    // A runner without a global init.defaultBranch leaves the empty clone on
    // master; name the branch explicitly so the upstream is main.
    git(&work, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    commit(&work, "a.txt", "one\n", "one");
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    commit(&work, "a.txt", "two\n", "two");
    commit(&work, "a.txt", "three\n", "three");

    let config = config_file(tmp.path());
    let out = run(
        &[
            "--root",
            work.to_str().unwrap(),
            "--depth",
            "0",
            "--no-color",
        ],
        &config,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("1 unpushed"), "{stdout}");
    assert!(
        !stdout.contains("2 unpushed"),
        "commit count leaked into the header: {stdout}"
    );
    assert!(stdout.contains("2 ahead"), "{stdout}");
}

#[test]
fn duplicate_folder_names_are_disambiguated() {
    if !git_available() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let oss = tmp.path().join("oss");
    let work = tmp.path().join("work");
    init(&oss.join("api"));
    init(&work.join("api"));
    commit(&oss.join("api"), "a.txt", "one\n", "one");
    commit(&work.join("api"), "a.txt", "one\n", "one");
    std::fs::write(oss.join("api").join("a.txt"), "dirty\n").unwrap();

    let config = config_file(tmp.path());
    let out = run(
        &[
            "--root",
            oss.to_str().unwrap(),
            "--root",
            work.to_str().unwrap(),
            "--depth",
            "1",
            "--no-color",
        ],
        &config,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("api (oss)"), "{stdout}");
    assert!(stdout.contains("api (work)"), "{stdout}");
}
