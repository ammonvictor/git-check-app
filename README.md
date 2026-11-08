# git-check app

[![CI](https://github.com/ammonvictor/git-check-app/actions/workflows/ci.yml/badge.svg)](https://github.com/ammonvictor/git-check-app/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/git-check.svg)](https://crates.io/crates/git-check)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Answers one question for a whole folder of projects: **what needs attention?**

Point it at a directory and get every repository under it classified — dirty,
unpushed, behind, cold, detached, conflicted — with portfolio-wide counts, commit
activity and machine-readable output. Born as a 40-line bash loop, rebuilt in
Rust so the same engine can grow a tray icon and ship on Linux, macOS and Windows.

**Status:** 0.1.0 — headless core + CLI, working and tested. The tray icon and
metrics popup are next; see [Roadmap](#roadmap).

> **Names:** the repository is `git-check-app` because the tray app is the end
> goal; the command stays `git-check` so it reads well in a shell, and the
> package is published on crates.io as [`git-check`](https://crates.io/crates/git-check).

---

## Install

Requires `git` in `PATH`. Nothing else — no C toolchain, no GUI or system
libraries.

```sh
cargo install git-check
cargo install --path crates/git-check --locked   # from a clone
```

Prebuilt binaries are produced by the release workflow on tag — Linux x86_64,
macOS arm64/x86_64 and Windows x86_64, plus a `.deb` — but none has been
published yet.

From a checkout: `make build`, `make install`, `make check`.

## Quick start

```sh
# scan the configured roots (a default root is seeded on first run)
git-check

# scan one folder without touching the config
git-check --root ~/code --depth 2

# machine-readable, for scripts and the upcoming UI
git-check --json | jq '.counters'

# fail in CI when something is dirty or unpushed
git-check --root ~/code --exit-code
```

## What the report means

```
git-check  61 repos · 8 dirty · 6 cold  230 ms

  career-advisor         66 dirty
  deepseek-harness-demo  5 dirty
  ...
  kwano-platform         1 dirty

  6 without an upstream branch

8 of 61 repos need attention.
```

| Term | Meaning |
| --- | --- |
| **dirty** | changed paths: staged, unstaged, untracked or conflicted |
| **unpushed** | local commits ahead of the branch's upstream (never fetches) |
| **behind** | upstream commits the local branch does not have |
| **cold** | last commit older than `stale_after_days` (default 30) |
| **no upstream** | current branch has no remote-tracking branch |

`unpushed` and `behind` reflect the **last known** remote-tracking refs.
git-check never runs `git fetch`: no network, no credentials, no surprise
latency. That is a deliberate design decision, not an omission.

## Commands

| Command | Purpose |
| --- | --- |
| `git-check` / `git-check report` | Portfolio report (default) |
| `git-check --json` | Counters, 30-day trend, every repo |
| `git-check roots add DIR [--depth N --label NAME]` | Persist a new root |
| `git-check roots list` | Show roots, depths, labels, missing paths |
| `git-check roots remove LABEL\|PATH` | Remove a root |
| `git-check config path` / `config show` | Where config lives / effective config |

Global flags: `--root DIR` (repeatable, ad-hoc), `--depth N`, `--backend cli`,
`--scope quick|full`, `--stale-days N`, `--json`, `--no-color`, `--exit-code`,
`--config FILE`.

`--scope quick` skips history and stash inspection (40 ms vs 230 ms on 61 repos).

## Configuration

Run `git-check config path`, then edit:

```toml
roots = [
  { path = "/Projects", depth = 3 },
  { path = "/home/you/code/oss", depth = 2, label = "oss" },
]
interval_secs = 120
stale_after_days = 30
exclude = ["node_modules", ".venv", "vendor", "dist", "build", "target", ".tox"]
follow_nested = false
launch_at_login = false
```

- `depth` counts directory levels **below** the root: `0` = the root itself,
  `1` = its immediate subdirectories.
- `exclude` names are pruned; hidden directories are searched except `.git`.
- A directory is a repository when it contains `.git` as a **directory** (normal
  checkout) **or a file** (linked worktree / submodule checkout).
- `GIT_CHECK_CONFIG` and `GIT_CHECK_STATE` override the platform paths
  (`~/.config/git-check`, `~/.local/state/git-check` on Linux).

## JSON output

```jsonc
{
  "generated_at": 1794130620,
  "duration_ms": 230,
  "counters": { "repos": 61, "dirty_repos": 8, "dirty_files": 82,
                "ahead_commits": 0, "behind_commits": 0, "cold": 46,
                "commits_7d": 351, "commits_30d": 3986, ... },
  "tier": "warn",
  "trend_30d": [ /* 30 daily commit counts, oldest first */ ],
  "streak_days": 152,
  "discovery_errors": [],
  "repos": [ { "name": "career-advisor", "dirty": 66, "state": "dirty",
               "ahead": 0, "behind": 0, "last_commit_ts": 1782900543,
               "age_days": 11, "cold": false, ... } ]
}
```

`commits_since(n) == sum(trend_n)` by construction: both use the same day-aligned
window, so the headline number always matches the chart underneath it.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | scan completed (and, with `--exit-code`, nothing needs attention) |
| 1 | `--exit-code` and something is dirty / unpushed / conflicting |
| 2 | usage or configuration error |

## Design notes

- **`git status --porcelain=v2 --branch`** is the single source of working-tree
  state and ahead/behind, so one subprocess answers most of the question.
- **`--no-optional-locks`** plus `GIT_OPTIONAL_LOCKS=0`: inspection never competes
  with an editor or a running git command for `index.lock`.
- **Parallel per-repo inspection** via rayon; 61 repos with 12 months of history:
  ~230 ms (40 ms with `--scope quick`).
- **Discovery is not `*/.git`**: it walks to a configurable depth, dedupes, and
  finds repositories the original script missed (nested checkouts, worktrees).
- **The backend is behind a trait** (`Inspector`). The `git` CLI backend shipped
  in 0.1.0; an in-process libgit2 backend will follow for Windows process-spawn
  cost and richer status data.

## Logs

The tray cannot write to a terminal when it is started by the desktop or at
login, so it logs to `<state>/logs/git-check-tray.log` instead — rotated at 1 MiB
with three archives kept. `GIT_CHECK_LOG=debug` (or `trace`) raises verbosity and
`GIT_CHECK_LOG_DIR` puts the file elsewhere.

## Known limitations

- No fetch, so `behind` can be stale by design.
- Untracked directories are counted as git reports them
  (`--untracked-files=normal`), not file by file.
- Bare repositories are not reported (they have no working tree).
- Commit history window is 12 months; `streak_days` is bounded by it.
- `--backend libgit2` is not built in this release and exits with a clear error.

## Roadmap

The goal is a tray app you install once and forget, not a CLI you have to remember
to run.

| Milestone | Scope | State |
| --- | --- | --- |
| **M1** | tray icon, metrics popup, background scan loop, launch at login | next |
| **M4** | installers: `.deb` (done), `.dmg`, MSI/NSIS, file logging | after M1 |
| **M2** | SQLite history, dashboard window with charts, per-root rollups | after the MVP |
| **M3** | native folder picker, file watching | later |

The installable MVP is **M1 plus the packaging half of M4**: a tray icon with its
metrics popup that installs on Linux, macOS and Windows. The dashboard and history
are deliberately v1.1 features — they are not allowed to delay an installable app.

## Development

```sh
make check     # fmt --check, clippy -D warnings, tests
make test
make lint
```

Integration tests create real repositories in a temp directory with the `git`
binary, so they exercise the same porcelain parsing production uses. Tests skip
themselves when git is absent.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security reports go through
[SECURITY.md](SECURITY.md). This project follows the
[Contributor Covenant](CODE_OF_CONDUCT.md). Everyone who has helped is listed in
[CONTRIBUTORS.md](CONTRIBUTORS.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in this project by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional terms
or conditions.
