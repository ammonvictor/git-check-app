# Contributing to git-check

Thanks for taking a look. This project is small on purpose: a scanner, a metrics
layer and (soon) a tray front end. Keep changes in that shape.

## Development

Requirements: Rust **1.90+** and `git` in `PATH`. Nothing else — no C toolchain,
no GUI libraries.

```sh
git clone https://github.com/ammonvictor/git-check-app
cd git-check-app
make check      # fmt --check + clippy -D warnings + tests
make run ARGS="--root ~/code --depth 2"
```

`make help` lists the targets. The same commands run in CI on Linux, macOS and
Windows, so a change that only passes on your machine is not done.

## Where things live

| Path | Purpose |
| --- | --- |
| `crates/git-check-core` | config, discovery, inspection, metrics — **no UI, no terminal output** |
| `crates/git-check` | the `git-check` binary: argument parsing and the report |
| `crates/git-check-tray` | tray icon and menu: consumes the same `Portfolio` |
| `crates/git-check-core/tests/scan.rs` | integration tests over real repositories created with `git` |

Anything that computes a number belongs in `core` with a test, not in `main.rs`.
The tray app and the JSON output must be able to reuse it.

## Commits

- Trunk-based: commit to `main`. Branch only for a risky or long-running change.
- Subject: `area: imperative summary`, e.g. `discovery: prune nested worktrees`.
  Areas: `core`, `discovery`, `inspect`, `metrics`, `cli`, `docs`, `ci`.
- Body: why the change is needed and what you verified. "Fixes the bug" is not
  enough; say which case was broken and how you checked it.
- One logical change per commit, each leaving the tree runnable.

## Pull requests

1. `make check` passes locally.
2. New behaviour has a test. Parsing changes need a fixture that would fail
   before the change.
3. Public metrics (counts, trend, definitions) stay consistent between the CLI,
   `--json` and the docs. If you redefine a metric, update all three.
4. Docs that describe behaviour (`README.md`) move with the code.

## Definition changes

The metric definitions in `README.md` are the contract. "Dirty", "unpushed" and
"behind" mean exactly what that table says; changing them is a breaking change
and needs a changelog entry.

## Reporting problems

- Bugs: use the bug report template. Include `git-check --version`, your OS, and
  the command's output. Redact paths if you prefer — but keep the error text.
- Security issues: do **not** open a public issue; see
  [SECURITY.md](SECURITY.md).

## Code of conduct

Participation is covered by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

By contributing you agree your work is licensed under both the MIT and Apache-2.0
licenses, as described in `LICENSE-MIT` and `LICENSE-APACHE`.
