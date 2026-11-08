//! The metrics popup.
//!
//! On Wayland a tray client cannot position a window next to its icon, so the
//! popup is the icon's native menu itself: the KPI line, the 30-day sparkline
//! and the worst offenders live in disabled items above the actions.

use git_check_core::metrics::{Counters, Portfolio};
use git_check_core::Snapshot;
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem};

/// Eight fill levels, lowest first.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Longest repository name shown in an offender row.
const NAME_WIDTH: usize = 24;

/// Items the main loop has to recognise after the menu is replaced.
pub struct MenuIds {
    pub scan: MenuId,
    pub quit: MenuId,
    pub launch: MenuId,
    pub launch_item: CheckMenuItem,
}

/// Menu for a finished scan, or a placeholder while the first one runs.
pub fn build(snapshot: Option<&Snapshot>, launch_at_login: bool) -> (Menu, MenuIds) {
    let menu = Menu::new();

    if let Some(snapshot) = snapshot {
        let counters = snapshot.portfolio.counters();
        append(&menu, kpi_line(&counters));
        append(&menu, sparkline_line(&snapshot.portfolio, &counters));
        separator(&menu);

        let ranked = snapshot.portfolio.ranked();
        if ranked.is_empty() {
            append(&menu, "Nothing needs attention".to_string());
        } else {
            for repo in ranked.into_iter().take(3) {
                append(&menu, offender_line(&repo.name, &repo.flags()));
            }
        }
        separator(&menu);
    } else {
        append(&menu, "Scanning…".to_string());
        separator(&menu);
    }

    let scan = MenuItem::new("Scan now", true, None);
    let dashboard = MenuItem::new("Open dashboard", false, None);
    let launch = CheckMenuItem::new("Launch at login", true, launch_at_login, None);
    let quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[
        &scan,
        &dashboard,
        &launch,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .ok();

    let ids = MenuIds {
        scan: scan.id().clone(),
        quit: quit.id().clone(),
        launch: launch.id().clone(),
        launch_item: launch,
    };
    (menu, ids)
}

/// Append a disabled item: the popup's numbers are labels, not actions.
fn append(menu: &Menu, text: String) {
    menu.append(&MenuItem::new(text, false, None)).ok();
}

fn separator(menu: &Menu) {
    menu.append(&PredefinedMenuItem::separator()).ok();
}

/// `9 dirty · 0 unpushed · 46 cold`, plus conflicts when there are any.
pub fn kpi_line(counters: &Counters) -> String {
    let mut line = format!(
        "{} dirty · {} unpushed · {} cold",
        counters.dirty_repos, counters.ahead_commits, counters.cold
    );
    if counters.conflicts > 0 {
        line.push_str(&format!(" · {} conflicts", counters.conflicts));
    }
    line
}

/// `30d ▁▂▄▆█… · 351 commits`.
pub fn sparkline_line(portfolio: &Portfolio, counters: &Counters) -> String {
    let trend = portfolio.daily_commits(30);
    let spark = sparkline(&trend);
    if spark.is_empty() {
        "30d · no commits".to_string()
    } else {
        format!("30d {spark} · {} commits", counters.commits_30d)
    }
}

/// One block per day, scaled to the busiest day in the window.
pub fn sparkline(values: &[usize]) -> String {
    let max = values.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return String::new();
    }
    values
        .iter()
        .map(|&value| BLOCKS[value * (BLOCKS.len() - 1) / max])
        .collect()
}

/// `career-advisor · 66 dirty`, with the name trimmed to fit a menu.
pub fn offender_line(name: &str, flags: &[String]) -> String {
    let mut short: String = name.chars().take(NAME_WIDTH).collect();
    if name.chars().count() > NAME_WIDTH {
        short.push('…');
    }
    if flags.is_empty() {
        short
    } else {
        format!("{short} · {}", flags.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_scales_against_the_busiest_day() {
        assert_eq!(sparkline(&[0, 4]), "▁█");
        assert_eq!(sparkline(&[1, 2, 4]), "▂▄█");
        assert_eq!(sparkline(&[3]), "█");
    }

    #[test]
    fn sparkline_is_empty_without_activity() {
        assert_eq!(sparkline(&[0, 0, 0]), "");
        assert_eq!(sparkline(&[]), "");
    }

    #[test]
    fn kpi_line_mentions_conflicts_only_when_present() {
        let counters = Counters {
            dirty_repos: 9,
            ahead_commits: 2,
            cold: 46,
            ..Default::default()
        };
        assert_eq!(kpi_line(&counters), "9 dirty · 2 unpushed · 46 cold");

        let with_conflicts = Counters {
            conflicts: 1,
            ..counters
        };
        assert!(kpi_line(&with_conflicts).ends_with("· 1 conflicts"));
    }

    #[test]
    fn offender_names_are_trimmed() {
        let long = "a-very-long-repository-name-that-will-not-fit";
        let line = offender_line(long, &["2 dirty".to_string()]);
        let kept: String = long.chars().take(NAME_WIDTH).collect();
        assert_eq!(line, format!("{kept}… · 2 dirty"));
    }

    #[test]
    fn offender_line_without_flags_is_just_the_name() {
        assert_eq!(offender_line("short", &[]), "short");
    }
}
