//! `git-check` — portfolio-wide git status for a folder of projects.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use git_check_core::config::{Config, Root};
use git_check_core::inspect::{Backend, Depth};
use git_check_core::metrics::Portfolio;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum BackendArg {
    Cli,
    #[value(name = "libgit2")]
    Libgit2,
}

impl From<BackendArg> for Backend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Cli => Backend::Cli,
            BackendArg::Libgit2 => Backend::Libgit2,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Scope {
    /// Working tree and ahead/behind only (fast).
    Quick,
    /// Also last commit, 12-month history and stashes.
    Full,
}

#[derive(Parser, Debug)]
#[command(
    name = "git-check",
    version,
    about = "Portfolio-wide git status for a folder of projects",
    after_help = "Examples:\n  \
        git-check\n  \
        git-check --root ~/code --depth 2\n  \
        git-check --json\n  \
        git-check roots add ~/code --depth 2 --label oss"
)]
struct Cli {
    /// Directory to search (repeatable). Overrides the configured roots.
    #[arg(long, global = true, value_name = "DIR")]
    root: Vec<PathBuf>,

    /// How many directory levels below each root to search.
    #[arg(long, global = true, value_name = "N")]
    depth: Option<usize>,

    /// Repository reader.
    #[arg(long, global = true, value_enum, default_value = "cli")]
    backend: BackendArg,

    /// Repos older than this many days count as cold.
    #[arg(long, global = true, value_name = "DAYS")]
    stale_days: Option<i64>,

    /// Use this config file instead of the platform default.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Machine-readable output (counters, trend, every repo).
    #[arg(long, global = true)]
    json: bool,

    /// Inspection depth.
    #[arg(long, global = true, value_enum, default_value = "full")]
    scope: Scope,

    /// Disable ANSI colour.
    #[arg(long, global = true)]
    no_color: bool,

    /// Exit with status 1 when anything needs attention (dirty, unpushed, conflict).
    #[arg(long, global = true)]
    exit_code: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print the portfolio report (default when no subcommand is given).
    Report,
    /// Manage the folders that are searched for repositories.
    Roots {
        #[command(subcommand)]
        action: RootsAction,
    },
    /// Show where configuration lives.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
enum RootsAction {
    /// Add a folder to the configured roots.
    Add {
        path: PathBuf,
        /// Display name for this root.
        #[arg(long)]
        label: Option<String>,
    },
    /// List configured roots.
    List,
    /// Remove a root by exact label or directory path.
    Remove { selector: String },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Print the config file path.
    Path,
    /// Print the effective configuration.
    Show,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("git-check: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let color =
        !cli.no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();

    let config = match &cli.config {
        Some(path) => Config::load_from(path)?,
        None => Config::load()?,
    };

    match cli.command {
        None | Some(Command::Report) => {
            let effective = effective_config(&cli, &config);
            if effective.roots.is_empty() {
                eprintln!(
                    "no roots configured; add one with `git-check roots add DIR` or use --root DIR"
                );
            }
            report(&cli, &effective, color)
        }
        Some(Command::Roots { ref action }) => roots(&cli, &config, action),
        Some(Command::Config { action }) => match action {
            ConfigAction::Path => {
                println!("{}", Config::path().display());
                Ok(ExitCode::SUCCESS)
            }
            ConfigAction::Show => {
                println!("{}", toml::to_string_pretty(&config)?);
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

/// Apply per-run overrides without touching the stored configuration.
fn effective_config(cli: &Cli, base: &Config) -> Config {
    let mut config = base.clone();
    if !cli.root.is_empty() {
        let depth = cli.depth.unwrap_or(3);
        config.roots = cli
            .root
            .iter()
            .map(|p| Root::new(Config::expand(p), depth, None))
            .collect();
    } else if let Some(depth) = cli.depth {
        for root in &mut config.roots {
            root.depth = depth;
        }
    }
    if let Some(days) = cli.stale_days {
        config.stale_after_days = days;
    }
    config
}

fn scan(config: &Config, backend: Backend, depth: Depth) -> Result<(Portfolio, u128, Vec<String>)> {
    let snapshot = git_check_core::scan(config, backend, depth)?;
    Ok((
        snapshot.portfolio,
        snapshot.duration_ms,
        snapshot.discovery_errors,
    ))
}

fn report(cli: &Cli, config: &Config, color: bool) -> Result<ExitCode> {
    let depth = match cli.scope {
        Scope::Quick => Depth::Quick,
        Scope::Full => Depth::Full,
    };
    let (portfolio, duration_ms, discovery_errors) = scan(config, cli.backend.into(), depth)?;
    let counters = portfolio.counters();

    for warning in discovery_errors.iter().take(3) {
        eprintln!("warning: {warning}");
    }
    if discovery_errors.len() > 3 {
        eprintln!(
            "warning: ... and {} more unreadable paths",
            discovery_errors.len() - 3
        );
    }

    if cli.json {
        let value = portfolio.to_json(duration_ms, &discovery_errors);
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print_report(&portfolio, duration_ms, color);
    }

    let attention = counters.dirty_repos > 0 || counters.ahead_repos > 0 || counters.conflicts > 0;
    if cli.exit_code && attention {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn paint(text: &str, colour: &str, color: bool) -> String {
    if color {
        format!("{colour}{text}{RESET}")
    } else {
        text.to_string()
    }
}

fn print_report(portfolio: &Portfolio, duration_ms: u128, color: bool) {
    let c = portfolio.counters();

    if c.repos == 0 {
        println!("No git repositories found.");
        println!(
            "{}",
            paint("Add a folder with: git-check roots add <DIR>", DIM, color)
        );
        return;
    }

    let mut segments = vec![format!("{} repos", c.repos)];
    let mut push = |count: usize, label: &str, colour: &str| {
        if count > 0 {
            segments.push(paint(&format!("{count} {label}"), colour, color));
        }
    };
    push(c.dirty_repos, "dirty", RED);
    push(c.ahead_commits, "unpushed", YELLOW);
    push(c.behind_commits, "behind", YELLOW);
    push(c.conflicts, "conflicts", RED);
    push(c.cold, "cold", DIM);

    println!(
        "{}  {}  {}",
        paint("git-check", BOLD, color),
        segments.join(&paint(" · ", DIM, color)),
        paint(&format!("{duration_ms} ms"), DIM, color),
    );

    let rows = portfolio.ranked();
    if rows.is_empty() {
        println!();
        println!(
            "{}",
            paint(
                &format!("All {} repos clean and pushed.", c.repos),
                GREEN,
                color
            )
        );
        return;
    }

    println!();
    let width = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .clamp(16, 34);
    for repo in &rows {
        let mut flags = repo.flags();
        if flags.is_empty() {
            if let Some(age) = repo.age_days(portfolio.generated_at) {
                if repo.is_cold(portfolio.generated_at, portfolio.stale_after_days) {
                    flags.push(format!("cold {age}d"));
                }
            }
        }
        if flags.is_empty() {
            continue;
        }
        let colour = if repo.conflicted > 0 || repo.dirty() > 0 {
            RED
        } else {
            YELLOW
        };
        let detail = if let Some(err) = &repo.error {
            err.clone()
        } else {
            flags.join(", ")
        };
        println!(
            "  {:<width$}  {}",
            repo.name,
            paint(&detail, colour, color),
            width = width
        );
    }

    if c.no_upstream > 0 || c.detached > 0 {
        println!();
        if c.no_upstream > 0 {
            println!(
                "  {}",
                paint(
                    &format!("{} without an upstream branch", c.no_upstream),
                    DIM,
                    color
                )
            );
        }
        if c.detached > 0 {
            println!(
                "  {}",
                paint(&format!("{} with detached HEAD", c.detached), DIM, color)
            );
        }
    }

    println!();
    let total = c.repos;
    println!(
        "{}",
        paint(
            &format!("{} of {total} repos need attention.", rows.len()),
            DIM,
            color
        )
    );
}

/// Indices of roots that `selector` names: an exact label, or the same
/// directory (after `~` expansion and, where the path exists, symlink
/// resolution). Never a substring or suffix, so `code` cannot hit `oss-code`.
fn matching_roots(roots: &[Root], selector: &str) -> Vec<usize> {
    let canonical = |p: &std::path::Path| {
        let expanded = Config::expand(p);
        expanded.canonicalize().unwrap_or(expanded)
    };
    let wanted = canonical(std::path::Path::new(selector));
    roots
        .iter()
        .enumerate()
        .filter(|(_, root)| {
            root.label.as_deref() == Some(selector) || canonical(&root.path) == wanted
        })
        .map(|(index, _)| index)
        .collect()
}

fn roots(cli: &Cli, config: &Config, action: &RootsAction) -> Result<ExitCode> {
    let mut config = config.clone();
    let path = cli.config.clone().unwrap_or_else(Config::path);

    match action {
        RootsAction::List => {
            if config.roots.is_empty() {
                println!("No roots configured. Add one with: git-check roots add <DIR>");
                return Ok(ExitCode::SUCCESS);
            }
            for root in &config.roots {
                let resolved = Config::expand(&root.path);
                let state = if resolved.is_dir() { "" } else { "  (missing)" };
                let label = root
                    .label
                    .as_deref()
                    .map(|l| format!("  [{l}]"))
                    .unwrap_or_default();
                println!(
                    "{:<50} depth={}{}{}",
                    resolved.display(),
                    root.depth,
                    label,
                    state
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        RootsAction::Add {
            path: new_path,
            label,
        } => {
            let resolved = Config::expand(new_path);
            if !resolved.is_dir() {
                bail!("{} is not a directory", resolved.display());
            }
            let key = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
            let exists = config.roots.iter().any(|r| {
                Config::expand(&r.path)
                    .canonicalize()
                    .map(|p| p == key)
                    .unwrap_or(false)
            });
            if exists {
                println!("{} is already a root", resolved.display());
                return Ok(ExitCode::SUCCESS);
            }
            let depth = cli.depth.unwrap_or(3);
            config
                .roots
                .push(Root::new(resolved.clone(), depth, label.clone()));
            config
                .save_to(&path)
                .with_context(|| format!("saving {}", path.display()))?;
            println!("Added {} (depth {depth})", resolved.display());
            println!(
                "{}",
                paint(&format!("Saved to {}", path.display()), DIM, false)
            );
            Ok(ExitCode::SUCCESS)
        }
        RootsAction::Remove { selector } => {
            let matches = matching_roots(&config.roots, selector);
            match matches.as_slice() {
                [] => bail!("no root matches `{selector}` (try: git-check roots list)"),
                [index] => {
                    let removed = config.roots.remove(*index);
                    config
                        .save_to(&path)
                        .with_context(|| format!("saving {}", path.display()))?;
                    println!("Removed {}", removed.display());
                    Ok(ExitCode::SUCCESS)
                }
                many => {
                    let names: Vec<String> =
                        many.iter().map(|i| config.roots[*i].display()).collect();
                    bail!(
                        "`{selector}` matches {} roots ({}); give the full path instead",
                        many.len(),
                        names.join(", ")
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_matches_labels_and_whole_paths_only() {
        let roots = vec![
            Root::new("/home/you/code", 3, None),
            Root::new("/home/you/oss-code", 2, Some("oss".to_string())),
        ];
        assert_eq!(matching_roots(&roots, "/home/you/code"), vec![0]);
        assert_eq!(matching_roots(&roots, "oss"), vec![1]);
        // A suffix of both paths used to remove both.
        assert!(matching_roots(&roots, "code").is_empty());
        assert!(matching_roots(&roots, "de").is_empty());
    }

    #[test]
    fn duplicate_labels_are_reported_not_guessed() {
        let roots = vec![
            Root::new("/a", 1, Some("work".to_string())),
            Root::new("/b", 1, Some("work".to_string())),
        ];
        assert_eq!(matching_roots(&roots, "work"), vec![0, 1]);
    }
}
