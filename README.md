# git-check

Answers one question for a whole folder of projects: **what needs attention?**

Point it at a directory, get every repository under it classified — dirty,
unpushed, behind, cold, detached, conflicted — with portfolio-wide counts, commit
activity and machine-readable output. Born as a 40-line bash loop
(`git-check.sh`) and rebuilt in Rust so the same tool can grow a tray icon and
ship on Linux, macOS and Windows.

**Status:** M0 — headless core + CLI, working and tested. The tray icon and the
metrics popup are M1 (see [PLAN.md](PLAN.md)).

---

## Install

Requires `git` in `PATH`. Nothing else — no C toolchain, no system libraries.

```sh
cargo install --path crates/git-check-cli --locked
# binary lands in ~/.cargo/bin/git-check
```

Or build a release binary and copy it anywhere:

```sh
make build            # target/release/git-check
make install          # cargo install --path crates/git-check-cli --locked
```

Windows and macOS are supported by the same code path; `git` is the only
requirement.

## Quick start

```sh
# scan the configured roots (first run seeds ~/Documents/Projects)
git-check

# scan one folder without touching the config
git-check --root ~/code --depth 2

# machine-readable, for scripts and future UI
git-check --json | jq '.counters'

# make it fail in CI when something is unpushed or dirty
git-check --root ~/code --exit-code
```

## What the report means

```
git-check  61 repos · 8 dirty · 6 cold  170 ms

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

`unpushed`/`behind` reflect the **last known** remote-tracking refs. git-check
never runs `git fetch`: no network, no credentials, no surprise latency.

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

`git-check config path`, then edit:

```toml
roots = [
  { path = "/home/you/Documents/Projects", depth = 3 },
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
  "duration_ms": 170,
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

`commits_since(n) == sum(trend_n)` by construction: both use the same
day-aligned window, so the headline number always matches the chart.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | scan completed (and, with `--exit-code`, nothing needs attention) |
| 1 | `--exit-code` and something is dirty / unpushed / conflicting |
| 2 | usage or configuration error |

## Design notes

- **`git status --porcelain=v2 --branch`** is the single source of working-tree
  state and ahead/behind, so one subprocess answers most of the question.
- **`--no-optional-locks`** + `GIT_OPTIONAL_LOCKS=0`: inspection never competes
  with an editor or a running git command for `index.lock`.
- **Parallel per-repo inspection** via rayon; 61 repos with 12 months of history: ~230 ms
  (40 ms with `--scope quick`).
- **Discovery is not `*/.git`**: it walks to a configurable depth, dedupes, and
  finds repos the original script missed (nested checkouts, worktrees).
- **`--backend cli` is the M0 backend**; an in-process libgit2 backend is planned
  for M1 behind the same `Inspector` trait, for Windows spawn cost and for
  richer status data.

## Known limitations

- No fetch, therefore `behind` can be stale by design.
- Untracked directories are counted as git reports them (`--untracked-files=normal`),
  not file-by-file.
- Bare repositories are not reported (they have no working tree).
- Commit history window is 12 months; `streak_days` is bounded by it.

## Development

```sh
cargo test          # 7 integration tests over real git fixtures
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

Fixtures are created with the `git` binary in a temp dir, so they exercise the
same porcelain parsing production uses. Tests skip themselves when git is absent.

## Roadmap

- **M1** tray icon + metrics popup (native menu, `tray-icon`/`muda`)
- **M2** dashboard window with charts (`eframe`/`egui`) and SQLite history
- **M3** folder picker, watch mode
- **M4** installers (`.deb`, `.AppImage`, `.dmg`, NSIS) and release CI

See [PLAN.md](PLAN.md) for the platform constraints behind those choices.
