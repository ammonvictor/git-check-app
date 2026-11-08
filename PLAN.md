# git-check — implementation plan

Status: **M0 delivered** — headless core + CLI, 7 integration tests, clippy clean
(see §14 for measured results). M1–M4 outstanding.

Decisions taken after review: **build M0 first** (CLI only, no tray yet) and
**target a public release**, which moves signing/notarization/updating into scope
for later milestones but not for M0.

Goal: turn `~/Documents/Projects/git-check.sh` into an installable, cross-platform
desktop tool (Linux/Ubuntu, macOS, Windows) that watches a user-chosen set of
project folders, shows a tray icon, and exposes portfolio-wide git metrics and
analytics from the tray popup.

---

## 1. What the tool does

| Capability | Detail |
| --- | --- |
| Point at folders | One or more project roots, chosen by CLI flag, config file, or native folder picker in the tray menu |
| Scan | Discover every git repo under those roots (depth-limited), inspect working tree + history |
| Tray icon | Colour-coded by portfolio health: green / amber / red, with a numeric badge |
| Tray popup | The anchored menu: KPI counts, a 30-day commit sparkline, the worst repos, actions |
| Dashboard | A real window with charts, a sortable repo table, and filters |
| History | Daily snapshots in SQLite so trends survive restarts and cover months |
| Autostart | Optional "launch at login", per-OS native mechanism |
| CLI | `git-check` alone prints the same report as the bash script today — usable over SSH, in scripts, in CI |

Non-goals (v1): no git *write* operations (no commit/push/fetch), no GitHub/GitLab
API calls, no team dashboards, no remote/agent/server mode.

**Local-only truth is a deliberate decision**: the tool never runs `git fetch`.
"Ahead/behind" therefore means *relative to the last known remote-tracking ref*.
Fetching would drag in credentials, network state, and surprise latency. A
"Fetch all" action can be considered later, explicitly opt-in.

---

## 2. This machine, today (measured)

| Item | Value | Consequence |
| --- | --- | --- |
| OS | Ubuntu 26.04, GNOME, **Wayland** | affects popup positioning (see §5) |
| Rust | cargo/rustc 1.98.1 via rustup | good, no upgrade needed |
| cmake | missing | vendored libgit2 cannot build here → libgit2 backend deferred to M1; M0 ships the `git` CLI backend |
| GTK 3 dev | missing | fine if we use the `ksni` backend |
| WebKitGTK dev | missing | rules out Tauri without `apt install` |
| AppIndicator GIR | missing (lib present) | irrelevant for `ksni`; needed only for the GTK fallback |
| Tray support | `ubuntu-appindicators@ubuntu.com` **enabled** | SNI/dbusmenu indicators render |
| Portfolio size | 60 repos at depth 1 under `~/Documents/Projects` | 0.95 s sequential `git status`; in-process libgit2 should be well under that |
| sudo | no passwordless | any apt installs are yours to run |

Shadowing `git` or running as root is not required anywhere in this plan.

---

## 3. Platform matrix: what "tray" means on each OS

| | Linux (GNOME/Wayland, KDE, X11) | macOS | Windows |
| --- | --- | --- | --- |
| Icon host | StatusNotifierItem over D-Bus | `NSStatusItem` | `Shell_NotifyIcon` |
| Anchored popup | native menu (compositor positions it) | native menu | native menu |
| Rich popup window anchored to icon | **not possible on Wayland** (no client window positioning); X11 only | possible (`NSPopover`) | possible |
| Icon tinting | ship light/dark-safe PNGs | must be a **template image** (`set_icon_as_template(true)`) | ship multi-size `.ico` |
| Text badge next to icon | supported by most hosts via label/title | `NSStatusItem` title works (macOS-only API) | no label — badge must be drawn into the icon |
| Tray requires | GNOME: AppIndicator shell extension (**stock GNOME has none**); Ubuntu ships it | none | none |

**Design consequence (the important one):** the *popup* is the **native menu**, on
all three platforms. That is the only thing that is guaranteed to appear attached
to the icon everywhere, including Wayland. Charts live in a separate dashboard
window that the menu opens. We do not try to position a window at the tray icon —
on Wayland that is impossible, and pretending otherwise produces a tool that
feels broken on the user's own desktop.

---

## 4. Stack decision

### Options considered

| | A. Native menu + egui dashboard | B. Tauri 2 (WebView popup) | C. Native menu only |
| --- | --- | --- | --- |
| UI language | Rust | Rust + HTML/CSS/JS | Rust |
| Linux build deps | **none** (`ksni` backend, winit; no GTK) | `libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`, `libgtk-3-dev` | none |
| Binary size | ~15–25 MB | ~10 MB + WebKitGTK runtime (~100 MB+ RSS) | ~5 MB |
| Installers | cargo-packager: deb, AppImage, dmg, NSIS, MSI | tauri bundler: same set, more polished | same as A |
| Rich analytics | egui + egui_plot: good enough | best (any JS chart lib) | text rows only |
| Wayland popup | menu anchored; dashboard is a normal window | same limitation (positioner plugin cannot place windows on Wayland) | n/a |
| Build/CI complexity | low | high (WebKit on every target, longer builds) | lowest |
| Autostart / single instance / logging | separate small crates | plugins provided | separate crates |

### Recommendation: **A — pure Rust, `tray-icon` (ksni on Linux) + `muda` menu + `eframe/egui` dashboard**

Reasons:

1. **No system GUI libraries.** `tray-icon` 0.26 has a `ksni` feature: on Linux it
   speaks StatusNotifierItem/dbusmenu directly and runs its own worker thread, so
   no GTK, no AppIndicator dev package, no `gtk::main()`. That removes the single
   most fragile part of every Rust tray app, and makes Linux CI and cross-building
   trivial.
2. **One menu model, three platforms.** `muda` menus are native on all three; the
   popup content is built from a plain data structure (`Vec<MenuItem>`), so the
   menu code is written once.
3. **A WebView buys nothing for the popup.** The popup is a native menu either way;
   the WebView would only serve the dashboard, at the cost of WebKitGTK on every
   Linux user's machine and notarization friction on macOS.
4. **Installable is a solved problem.** `cargo-packager` emits `.deb`, `.AppImage`,
   `.dmg`, `.app`, NSIS `.exe`, `.msi` from one `Packager.toml`.

Fallback if the native menu turns out too cramped for the analytics you want:
move only the dashboard to Tauri later. The core crate does not change.

### Crate inventory (versions checked against crates.io today)

| Crate | Version | Role | Notes |
| --- | --- | --- | --- |
| `tray-icon` | 0.26.1 | tray icon | `default-features = false, features = ["ksni"]` on Linux; default backend on macOS/Windows |
| `muda` | 0.21.2 | native menus | pulled in by `tray-icon`; use directly for the menu model |
| `eframe` / `egui` | 0.37.0 | dashboard window | winit event loop on the main thread satisfies macOS/Windows tray requirements |
| `egui_plot` | version-matched | charts | optional; bar chart is ~30 lines of painter code if we skip it |
| `git2` | 0.21.0 | repo inspection (**deferred to M1**) | `vendored-libgit2` needs **cmake**, which this machine lacks; M0 uses the `git` CLI backend behind the same `Inspector` trait |
| `rusqlite` | bundled | snapshot history | `bundled` feature → no system SQLite |
| `notify` | current | watch mode (M3) | debounced FS events → targeted rescan |
| `auto-launch` | 0.6.0 | launch at login | XDG autostart / LaunchAgent / `HKCU\…\Run` |
| `single-instance` | 0.3.3 | one tray process | second launch focuses the dashboard |
| `rfd` | current | native folder picker | use the `xdg-portal` feature to stay GTK-free on Linux |
| `directories` | current | config/state paths | `ProjectDirs` → `~/.config/git-check`, `~/Library/Application Support/…`, `%APPDATA%\…` |
| `serde` + `toml` | current | config | human-editable, comment-preserving on write |
| `tracing` + `tracing-appender` | current | logging | tray apps have no terminal; write to state dir |
| `cargo-packager` | 0.11.8 | installer generation | dev-only |

Explicitly **not** used: `tauri` (2.x stable, 3.x alpha — heavier than needed),
`gix` (pure-Rust alternative to `git2`; revisit when its status API stabilises),
`winit` directly (eframe owns the loop).

---

## 5. Repository layout

```
git-check/
├── Cargo.toml                 # workspace
├── Packager.toml              # M4: cargo-packager (deb, appimage, dmg, nsis, msi)
├── Makefile                   # build / test / lint / install
├── README.md
├── crates/
│   ├── git-check-core/        # no UI — DELIVERED in M0
│   │   ├── src/lib.rs
│   │   ├── src/config.rs      # roots, XDG/platform paths, TOML
│   │   ├── src/discover.rs    # depth-limited walk, worktrees, excludes
│   │   ├── src/inspect/       # Inspector trait + cli backend (libgit2 in M1)
│   │   ├── src/metrics.rs     # Portfolio, counters, trend, ranking
│   │   └── tests/scan.rs      # 7 integration tests over real git fixtures
│   ├── git-check-cli/         # bin: git-check — DELIVERED in M0
│   └── git-check-app/         # M1: bin: git-check-tray (tray icon, menu, dashboard)
├── assets/                    # M1/M4: icons (png/ico/icns), desktop entry
└── .github/workflows/release.yml   # M4: 3-OS matrix (or Forgejo equivalent)
```

`core` has zero GUI dependencies, so it is unit-testable on a headless CI runner and
reusable by the CLI.

---

## 6. Data model (core)

```rust
struct Portfolio {
    generated_at: SystemTime,
    roots: Vec<Root>,            // { path, repos_found }
    repos: Vec<RepoStatus>,
    scan_duration_ms: u128,
    errors: Vec<ScanError>,      // { path, kind }
}

struct RepoStatus {
    path: PathBuf, name: String,
    branch: Option<String>, upstream: Option<String>,
    ahead: usize, behind: usize,
    staged: usize, unstaged: usize, untracked: usize, conflicted: usize,
    stashes: usize, oldest_stash_age_days: Option<u32>,
    detached: bool,
    has_remote: bool,
    head: Option<String>,
    last_commit_ts: Option<i64>, last_subject: String,
    commit_ts_90d: Vec<i64>,      // for trend buckets, capped at 90 days
    error: Option<String>,
}
```

Derived, not stored: `dirty = staged+unstaged+untracked+conflicted`,
`diverged = ahead>0 && behind>0`, `cold = age > stale_after`,
`score` (ranking weight below).

---

## 7. Metrics and analytics spec

### Definitions (must match the CLI, the menu and the dashboard exactly)

| Metric | Definition |
| --- | --- |
| repos | directories containing `.git` (directory **or** file: worktrees/submodules) |
| dirty repos | repos with ≥1 changed path (index, worktree, untracked, or conflict) |
| dirty files | total changed paths across the portfolio |
| unpushed commits | Σ `ahead` over repos (local commits not on `@{upstream}`) |
| behind commits | Σ `behind` |
| diverged | repos with both ahead and behind > 0 |
| conflicts | repos with unmerged index entries |
| stashes | Σ stashes; plus the age of the oldest one |
| no upstream | repos whose current branch has no remote-tracking branch |
| detached | repos with detached HEAD |
| cold | repos whose last commit is older than `stale_after` (default 30 d) |
| commits/7d, /30d, /90d | commits reachable from HEAD within the window |
| active repos | repos with ≥1 commit in the window |
| streak | consecutive days with ≥1 commit anywhere |
| scan health | duration, repo count, repos that failed to open |

### Ranking

`score = 100·conflicts + 4·min(dirty,50) + 3·min(ahead,20) + 2·min(behind,20) + 5·detached + 2·error`

Used to pick the "needs attention" rows and to order the dashboard table.

### Where each thing appears

| | Menu popup | Dashboard |
| --- | --- | --- |
| KPI counts | 4 cells: dirty, unpushed, behind, cold | 8 cards incl. conflicts, stashes, no upstream |
| Activity | 30-day unicode sparkline + "18 commits/7d · 14 active" | 90-day bar chart, weekday/hour histogram, per-root rollup |
| Per-repo | top 6 by score, one line each | full sortable table, filter box |
| Trend vs yesterday | "+3 dirty" delta from history | 30/90-day dirty & unpushed lines |
| Actions | Dashboard…, Rescan, Add folder…, Launch at login, Quit | copy summary, open repo, add/remove root |

---

## 8. Popup spec (the anchored menu)

```
 Git portfolio                    60 repos · 12:04
 ┌─────────┬─────────┬─────────┬─────────┐
 │    8    │   12    │    3    │    6    │
 │  dirty  │ unpushed│ behind  │  cold   │
 └─────────┴─────────┴─────────┴─────────┘
 ▁▂▄▃▅▇▆▂▃▁▁▂▅▃▆  ← 30-day commits (unicode blocks)
 18 commits/7d · 14 active repos · 3 stashes   (+3 dirty vs yesterday)
 ────────────────────────────────────────────
 NEEDS ATTENTION
 ● kwanoyot-backend      7 dirty · 2 ahead
 ● hermes-dkyc           1 conflict
 ● kwano-platform        3 ahead · 1 stash
 ● figma-deepseek-mcp    cold 64d
 ● …                     (max 6 rows, click = open terminal in that repo)
 ────────────────────────────────────────────
 Dashboard…                        ⌘/Ctrl D
 Rescan now                        ⌘/Ctrl R
 Add folder…
 ✓ Launch at login
 Quit
```

Implementation notes:

- Unicode block sparkline (`▁▂▃▄▅▆▇`) inside a disabled menu item: renders in the
  native menu on all three platforms, needs no window, and is the one "chart" that
  is legitimately possible in a native menu. Height-normalised, last 30 days.
- Menu is rebuilt wholesale on each scan (`muda` menus are cheap to rebuild) and
  swapped in; no incremental diffing.
- Clicking a repo row opens the terminal at that repo (`ptyxis`, `kgx`,
  `gnome-terminal`, `alacritty`, `foot`, `Terminal.app`, `wt`/`powershell` — probed
  per OS). Fallback: open the folder in the file manager.
- Empty state: `✓ all 60 repos clean and pushed` in green.
- The badge number next to the icon is Linux/macOS only; on Windows the colour tier
  carries the signal and the number lives in the tooltip + menu.

---

## 9. Dashboard spec

Single window, 900×620, three scrollable sections (no tabs to start):

1. **KPI cards** — 8 counters, colour-coded, with yesterday's delta.
2. **Activity** — 90-day commit bar chart; a weekday×hour heatmap (cheap, from the
   same timestamps); per-root breakdown (repos, dirty, unpushed).
3. **Repos table** — columns: repo, branch, dirty, ahead, behind, stash, last commit
   age, state. Sortable by any column, filter box, click a row → open repo.
   Section filter chips: All / Dirty / Unpushed / Diverged / Cold / Errors.

Refresh: on scan completion; `Rescan` button; optional live watch (M3).

---

## 10. "Point at a specific folder" — UX and config

Three entry points, one stored truth:

```toml
# ~/.config/git-check/config.toml  (macOS/Windows: platform config dir)
roots = [
  { path = "~/Documents/Projects", depth = 3, label = "work" },
  { path = "~/code/oss",           depth = 2, label = "oss"  },
]
interval_secs = 120
stale_after_days = 30
exclude = ["node_modules", ".venv", "vendor", "dist", "build", "target"]
follow_nested = false
launch_at_login = false
```

- **CLI**: `git-check roots add ~/code --depth 2 --label oss`, `roots list`,
  `roots remove oss`; ad-hoc run without saving: `git-check report --root ~/code`.
- **Tray**: `Add folder…` → `rfd` native folder picker → appends a root (depth 3
  default) → rescan. Second dialog step (or dashboard) lets you set depth/label.
- **First run** (no config): seed with the existing default (`~/Documents/Projects`
  on Linux/macOS; `%USERPROFILE%\source\repos` is a sane Windows default) and show
  the dashboard once so the user sees what was found.

Discovery details: recurse to `depth`, prune `exclude` names and `.git`, detect repo
by `.git` presence, dedupe by `(dev, inode)` / canonical path, skip bare repos,
record unreadable directories as non-fatal errors.

---

## 11. Scan engine

- **Backend trait** with two impls: `Libgit2` (default) and `Cli` (`git` binary, the
  current script's semantics, escape hatch via `git-check --backend cli`). Keeps us
  honest when libgit2 disagrees with git.
- Per repo: `Repository::open` → statuses (`include_untracked`, `recurse_untracked_dirs=false`),
  upstream via `branch.upstream()` + `graph_ahead_behind`, stash count via
  `stash_foreach`, last commit + 90-day timestamps via a time-bounded `revwalk`.
- Parallel across repos with `rayon`. Target: 60 repos < 300 ms warm, < 1 s cold.
- Never writes to the repo; libgit2 status honours `.gitignore`, so untracked noise
  matches what `git status` shows.
- Traps to handle explicitly: worktrees and submodule checkouts (`.git` is a file),
  detached HEAD, unborn branches (no commits yet), repos with no remote, permission
  errors, very large untracked trees (bound with a timeout + `--untracked-files=no`
  fallback after N ms), and network filesystems (per-repo timeout, mark slow).

---

## 12. Persistence

SQLite in the state dir (`rusqlite`, bundled):

```sql
CREATE TABLE snapshot (
  day TEXT NOT NULL, repo_path TEXT NOT NULL,
  dirty INTEGER, ahead INTEGER, behind INTEGER, commits INTEGER,
  PRIMARY KEY (day, repo_path)
);
CREATE TABLE scan (
  at INTEGER PRIMARY KEY, duration_ms INTEGER, repos INTEGER, errors INTEGER
);
```

Daily upsert on each scan → months of trend data, "vs yesterday" deltas, and the
dashboard's 30/90-day lines. WAL mode, one connection guarded by a mutex; no
long-running transactions, so a crashed scan cannot corrupt the file.

---

## 13. Installability

| Concern | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Artifact | `.deb` (primary), `.AppImage` | `.dmg` containing `.app` (universal: arm64 + x86_64) | NSIS `.exe` + `.msi` |
| Build command | `cargo packager --release --formats deb,appimage` | `cargo packager --release --formats app,dmg` | `cargo packager --release --formats nsis,msi` |
| Runtime deps | none (glibc only; build against the oldest supported Ubuntu) | none | none (MSVC runtime bundled by NSIS) |
| Autostart | `~/.config/autostart/git-check.desktop` | `~/Library/LaunchAgents/*.plist` | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| Dock/taskbar | normal app entry; tray-only process hides window | `ActivationPolicy::Accessory` + `LSUIElement` (no Dock icon) | no taskbar entry; tray only |
| Tray caveat | GNOME needs the AppIndicator extension → `.deb` gets `Recommends: gnome-shell-extension-appindicator`; Ubuntu already enables it | none | none |
| Signing | none needed | ad-hoc `codesign -s -` is enough for your own machines; **notarization** (Developer ID) only for public distribution | Authenticode only for public distribution; otherwise SmartScreen will warn on first run |
| Updates | `apt install ./git-check_*.deb` / replace AppImage | replace `.app` | rerun installer |

- Config/state/logs live in platform dirs via `directories`; a `--portable` flag
  keeps everything next to the binary if you want a USB-stick install.
- `single-instance` + `--open-dashboard`: launching again from the app menu focuses
  the running dashboard instead of starting a second tray icon.
- Logging to `<state>/logs/git-check.log` with rotation; `--log-level` and
  `RUST_LOG` override.
- CI (`.github` or Forgejo Actions, whichever you prefer for `git.kwano.org`):
  matrix `ubuntu-latest` / `macos-14` (arm64) + `macos-13` (x86_64) / `windows-latest`;
  cache cargo; upload artifacts; tag → attach release files.

### Public-release path for the CLI (the M0 deliverable)

A CLI has a much cheaper distribution story than a GUI app, so ship M0 while the
tray is still being built:

| Channel | Artifact | Notes |
| --- | --- | --- |
| Forgejo/GitHub releases | `git-check-<ver>-{x86_64-unknown-linux-gnu,aarch64-unknown-linux-musl,x86_64-apple-darwin,aarch64-apple-darwin,x86_64-pc-windows-msvc}.tar.gz/.zip` | static-ish binaries, no runtime deps; `musl` build covers older glibc |
| `cargo install git-check-cli` | source | once the crate is published |
| `.deb` / `.rpm` | via `cargo-deb` / `cargo-generate-rpm` or cargo-packager | for Ubuntu users; `Depends: git` only |
| Homebrew tap | formula pointing at the release tarball | cheapest macOS path, no notarization needed for brew installs |
| `winget` / Scoop | manifest pointing at the `.zip` | winget manifest is a PR to microsoft/winget-pkgs |

Signing: **not required for the CLI.** Unsigned binaries fetched with `curl`,
`cargo`, `brew` or `apt` are not quarantined the way a browser-downloaded GUI app
is on macOS. If a user does download the macOS tarball via a browser, note
`xattr -d com.apple.quarantine` in the README — or add an ad-hoc `codesign -s -`
step, which costs nothing. Notarization is only worth paying for once the GUI app
ships as a `.dmg` (M4).

Versioning: single workspace version, `0.1.0` until the tray lands; tag `v0.1.0`;
`git-check --version` prints it via clap.

---

## 14. Milestones

**M0 — Headless core + CLI — DELIVERED**
Workspace, `git-check-core` (config, discovery, inspection, metrics), `git-check`
CLI with `report`, `--json`, `roots add/list/remove`, `--root`, `--depth`, `--scope`,
`--backend`, `--exit-code`.

Acceptance results on this machine (Ubuntu 26.04, 61 repos under
`~/Documents/Projects`):

| Check | Result |
| --- | --- |
| Report matches `git-check.sh` | ✅ same 8 dirty repos, byte-identical set |
| Full scan (12-month history) | ~230 ms warm (250–600 ms depending on page cache) |
| `--scope quick` | ~40 ms |
| `--json` | valid; `commits_since(n) == sum(trend_n)` asserted |
| Tests | 7/7 integration tests over real git fixtures |
| Lint | `clippy --all-targets -- -D warnings` clean |
| Extra repos found | 61 vs the script's 60 (nested checkout at depth 2/3) |

Deviations recorded:
- libgit2 backend deferred to M1: `cmake` is absent, so `vendored-libgit2` cannot
  build here. `--backend libgit2` fails with an explicit message rather than
  shipping an unverified code path.
- Discovery depth semantics changed to "levels below the root" (root = 0); an
  off-by-one that recorded repos one level too deep was caught by tests.
- The global `--depth` flag originally rewrote every root's depth before
  `roots add` saved, silently mutating unrelated roots; overrides are now applied
  per run only and never persisted.

Follow-ups carried into M1: stash/conflict fixtures (need a conflict-creating
setup), libgit2 backend once cmake is available, and a `--scope quick` path that
the tray can reuse for its 2-minute refresh.

**M1 — Tray icon + menu (the popup)**
`tray-icon` + `muda`, colour-tier icon set, KPI cells, unicode sparkline, top-6 rows,
actions, rebuild-on-scan, single instance, launch-at-login toggle.
Acceptance: icon appears on Ubuntu GNOME here; clicking a row opens the repo; badge
updates within one interval; no GTK dev libraries needed for the Linux build; smoke
run on macOS + Windows VMs.

**M2 — History + dashboard**
SQLite snapshots, `eframe/egui` window, KPI cards, 90-day chart, repo table with
sort/filter, per-root rollup, deltas vs yesterday.
Acceptance: dashboard opens from the menu and from a second launch; trend survives
restart and shows ≥2 days of data after two runs; table sorted by score by default.

**M3 — Folder picker, watch mode, polish**
`rfd` picker, root depth/label editing, `notify` watcher with 2 s debounce +
targeted rescan, settings window, error surfaces, logging, timeouts.
Acceptance: adding a folder from the tray changes the portfolio without editing TOML;
a commit in one repo updates the icon within ~3 s.

**M4 — Packaging + CI**
`Packager.toml`, icons for all three platforms (.png/.ico/.icns), desktop entry,
release workflow, local `.deb` install verified, dmg + NSIS artifacts produced in CI.
Acceptance: `apt install ./git-check_*.deb` on a clean Ubuntu VM yields a working
tray icon after login; same for macOS/Windows artifacts.

Each milestone leaves `main` runnable, per the trunk-based convention.

---

## 15. Risks and mitigations

| # | Risk | Likelihood | Mitigation / validation |
| --- | --- | --- | --- |
| 1 | `muda` + `ksni` menu interaction on Linux (dbusmenu) has rough edges | medium | Spike in M1 first: one icon, three items, verify on GNOME + KDE. Fallback: `libappindicator` feature + `libgtk-3-dev` (documented default path) |
| 2 | `tray-icon` ksni backend accidentally pulling GTK in at build time | low | Acceptance test: `cargo tree` shows no `gtk-sys`; build in a container without GTK headers |
| 3 | macOS: tray must be created on the main thread alongside eframe's loop | medium | Create the tray inside `eframe`'s setup closure; verify template icon + no Dock icon on a Mac before M2 |
| 4 | Wayland: popup window cannot be anchored to the icon | certain | Design already puts analytics in the anchored *menu*; dashboard is a normal window. Do not add window-positioning hacks |
| 5 | GNOME without the AppIndicator extension shows no icon | certain on stock GNOME | Document it; `Recommends:` in the deb; CLI still fully functional |
| 6 | libgit2 differs from git (sparse checkout, ignored files, weird configs) | medium | `Cli` backend + `--backend` flag; golden tests compare both backends on the same fixtures |
| 7 | Scan too slow on huge repos / network drives | medium | rayon, per-repo timeout, `--untracked-files=no` fallback, exclude list, optional watch-mode incremental scan |
| 8 | Windows icon/DPI and badge rendering | medium | Ship multi-size `.ico`; draw the badge into the pixmap; test at 100%/150%/200% scaling |
| 9 | Public distribution needs signing/notarization | low (personal use) | Defer; document the cost (Apple Developer Program, Authenticode cert) if it becomes team-wide |

---

## 16. Open questions for you

1. ~~**Dashboard scope**~~ — **answered: build M0 first** (CLI only). The dashboard
   (M2) is no longer assumed; it comes after the tray proves useful.
2. ~~**Distribution**~~ — **answered: public release.** M0's release path is laid
   out in §13; Apple notarization and Authenticode stay deferred until the GUI
   ships (M4).
3. **Depth and roots** — one root (`~/Documents/Projects`) with depth 3, or several
   labelled roots out of the box (work vs OSS)?
4. **The old script** — keep `git-check.sh` as a thin wrapper around the CLI now
   that M0 matches its output, or delete it?
5. **Repo hosting** — new repo on `git.kwano.org` under your owner account (with the
   `pythia` remote for agent pushes), or a plain local repo for now?

---

## Appendix: how a user installs it (target end state)

```sh
# Ubuntu
sudo apt install ./git-check_0.1.0_amd64.deb
git-check roots add ~/Documents/Projects
git-check-tray &        # or pick "git-check" from the app grid; autostart is a menu toggle
```

```powershell
# Windows
.\git-check_0.1.0_x64-setup.exe
git-check roots add C:\Users\me\source\repos
```

```sh
# macOS
open git-check_0.1.0_universal.dmg   # drag to /Applications
git-check roots add ~/Documents/Projects
```

From source, for development:

```sh
cargo build --release          # Linux: no apt packages needed with the ksni backend
cargo test -p git-check-core
cargo packager --release
```
