# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

### Changed

- MSRV raised from 1.85 to 1.90, required by `tray-icon` 0.26's ksni backend.

### Planned

- Metrics popup (native menu): KPI cells, 30-day sparkline, worst-offender rows,
  folder picker.
- Dashboard window with charts and SQLite-backed history.
- Installers for Linux, macOS and Windows.

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
