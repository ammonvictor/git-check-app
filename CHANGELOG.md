# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Commit totals, the 30-day trend and the streak count each commit once. Linked
  worktrees and duplicate clones no longer double-count shared history.
- `git-check roots remove` matches an exact label or the same directory only.
  The old suffix match let `remove code` also remove `/home/you/oss-code`; a
  selector that matches several roots is now an error that lists them.
- The tray re-reads the config before saving, so roots added with the CLI or an
  editor while it runs are no longer overwritten. A config file that does not
  parse is reported instead of replaced.
- The tray's "Choose folder…" recognises an existing root however its path is
  spelled (`~`, trailing `.`, symlinks).

### Security

- Every git call passes `-c core.fsmonitor=false -c log.showSignature=false`.
  Previously a repository's `.git/config` could name a program that `git status`
  or `git log` ran on every scan, including every tray rescan.
- A repository whose local config defines a content filter (`filter.*.clean`,
  `.smudge` or `.process`, including one pulled in through `include.path`) is now
  reported and not inspected: `git status` would otherwise run that program while
  comparing a file.

### Planned

- Dashboard window with SQLite-backed history and per-root rollups.
- Watch mode.

## [0.2.0] - 2026-11-09

Second release: the tray application. The same engine now drives an icon, a
native-menu popup and installers on Linux, macOS and Windows.

### Added

- `git-check-tray`: a tray binary whose icon is coloured by portfolio health
  (ok / warn / alert), with a tooltip carrying live counts, a "Scan now" item and
  a rescan loop driven by `interval_secs`.
- Platform tray fit: on macOS the icon is a template image, the app is an
  accessory (no Dock icon) and the title is dropped; on Windows the icon is
  generated at 64 px and the tooltip carries the last scan time.
- Launch at login through each platform's native registration (XDG autostart
  entry, macOS LaunchAgent, per-user Windows Run key), toggled from a
  "Launch at login" checkbox and persisted as `launch_at_login`.
- A single-instance guard: the tray holds a lock in the state directory, so a
  second launch reports "already running" and exits.
- Metrics popup as a native menu: the KPI line (`9 dirty · 2 unpushed · 46
  cold`), a 30-day unicode sparkline and the three worst offenders, rebuilt after
  every scan.
- A per-user Windows MSI built in CI: both binaries under
  `%LOCALAPPDATA%\Programs\git-check` plus a Start Menu shortcut, no admin
  rights needed.
- A macOS packaging path: `scripts/macos-bundle.sh` turns a universal tray
  binary into `git-check.app` (LSUIElement, ad-hoc signed) and a `.dmg` with an
  /Applications symlink; the release workflow builds it and `workflow_dispatch`
  can exercise it without a tag.
- A "Choose folder…" tray action: a native picker (xdg-portal on Linux, so
  still GTK-free) adds a root, saves it to the config and rescans at once.
- First run no longer latches onto an empty default root; with no repositories
  the popup says "No repositories found" and points at the folder chooser, and
  the CLI prints how to add a root.
- The tray writes a rotated log to `<state>/logs/git-check-tray.log` (1 MiB,
  three archives); `GIT_CHECK_LOG=debug` raises verbosity and
  `GIT_CHECK_LOG_DIR` overrides the location.

### Changed

- MSRV raised from 1.85 to 1.90, required by `tray-icon` 0.26's ksni backend.

## [0.1.0] - 2026-11-08

First release: the headless core and CLI, replacing the original `git-check.sh`
loop.

### Added

- Repository discovery for one or more roots, with configurable depth, exclusions,
  nested-checkout support and linked-worktree support (`.git` as a file).
- Per-repository inspection via `git status --porcelain=v2 --branch`: staged,
  unstaged, untracked and conflicted paths, ahead/behind against upstream,
  detached HEAD, missing upstream, stashes, last commit and 12 months of commit
  history.
- Portfolio metrics: dirty repos and files, unpushed and behind commits,
  conflicts, cold repos, diverged branches, commits over 7/30 days, active repos
  and commit streak, all aligned to whole UTC days.
- `git-check` CLI: report (default), `--json`, `--root`, `--depth`, `--scope
  quick|full`, `--backend cli`, `--stale-days`, `--exit-code`, `--no-color`.
- `git-check roots add|list|remove` and `git-check config path|show` for
  persistent, multi-root configuration in TOML.
- Platform-correct config/state paths on Linux, macOS and Windows.
- Integration tests over real git fixtures: clean, dirty, ahead, behind,
  detached, depth/excludes and linked worktrees.

[Unreleased]: https://github.com/ammonvictor/git-check-app/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ammonvictor/git-check-app/releases/tag/v0.1.0
