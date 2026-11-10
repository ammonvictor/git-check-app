# Security policy

## Supported versions

`git-check` is pre-1.0. Security fixes land on the latest `0.x` line only.

| Version | Supported |
| --- | --- |
| 0.2.x | yes |
| older | no |

## Reporting a vulnerability

Please use GitHub's private reporting:

**https://github.com/ammonvictor/git-check-app/security/advisories/new**

Do not open a public issue for a vulnerability. Please include the version, the
operating system, and the smallest reproduction you can manage.

Expect an initial response within a few days. There is no bug bounty.

## Security model

Worth knowing when you assess a report:

- `git-check` is a **local** tool. It reads repositories on disk and never
  performs network requests, never runs `git fetch`, and never prompts for
  credentials.
- It executes the `git` binary found in `PATH`. Anything that controls `PATH`
  controls what runs — that is a property of every git front end, not a flaw in
  this one.
- It runs no build scripts from the repositories it inspects. It runs no git
  hooks and does not verify commit signatures during a scan: every git call
  passes `-c core.fsmonitor=false -c log.showSignature=false`, because
  `git status` and `git log` otherwise run programs named in the repository's
  `.git/config` (`core.fsmonitor`, `gpg.program`).
- A repository whose local config defines a content filter — `filter.*.clean`,
  `.smudge` or `.process`, including one pulled in through `include.path` — is
  reported and **not inspected**. `git status` would otherwise run that program
  while comparing a file. If you trust such a repository, remove the local filter
  config or inspect it with plain `git`.
- If you find another config key that makes `git status`, `git log` or
  `git stash list` run a program, report it; that is exactly the class of issue
  this section is about.
- Git inspection uses `--no-optional-locks`, so it cannot leave a stale
  `index.lock` behind.
- It writes only to its own config and state directories
  (`~/.config/git-check`, `~/.local/state/git-check` on Linux), or the paths in
  `GIT_CHECK_CONFIG` / `GIT_CHECK_STATE`.

## Data in output

`--json` and the report contain repository paths, branch names and commit
subjects. That is your data, not telemetry — nothing is uploaded. Still, be
careful pasting JSON into a bug report if your project names or commit messages
are sensitive.
