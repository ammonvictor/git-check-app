//! `git-check-tray` — portfolio health as a tray icon.
//!
//! A background thread rescans the configured roots on the configured
//! interval (and whenever "Scan now" is chosen). The main thread owns the tray
//! icon so the platform's event loop stays on the thread that created it, and
//! only the icon colour and tooltip change as scans complete.

mod icon;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use git_check_core::config::Config;
use git_check_core::inspect::{Backend, Depth};
use git_check_core::metrics::Tier;
use git_check_core::Snapshot;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

/// What a finished scan tells the tray.
struct Update {
    tier: Tier,
    tooltip: String,
}

/// Work the UI sends to the scan thread.
enum Request {
    Rescan,
    Quit,
}

fn main() {
    let config = match Config::load() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("git-check-tray: {err}");
            std::process::exit(1);
        }
    };
    let interval = Duration::from_secs(config.interval_secs.max(5));

    let scan_item = MenuItem::new("Scan now", true, None);
    let dashboard_item = MenuItem::new("Open dashboard", false, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &scan_item,
        &dashboard_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])
    .expect("menu");

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_title("git-check")
        .with_tooltip("git-check: scanning")
        .with_icon(icon::for_tier(Tier::Ok))
        .build()
        .unwrap_or_else(|err| {
            eprintln!("git-check-tray: cannot create the tray icon: {err}");
            std::process::exit(1);
        });

    let (request_tx, request_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    let worker = thread::spawn(move || worker_loop(config, interval, request_rx, update_tx));

    let menu_events = MenuEvent::receiver();
    let mut quitting = false;
    while !quitting {
        while let Ok(event) = menu_events.try_recv() {
            if event.id == quit_item.id() {
                quitting = true;
                break;
            }
            if event.id == scan_item.id() {
                let _ = request_tx.send(Request::Rescan);
            }
        }
        if quitting {
            break;
        }
        match update_rx.recv_timeout(Duration::from_millis(120)) {
            Ok(update) => {
                let _ = tray.set_icon(Some(icon::for_tier(update.tier)));
                let _ = tray.set_tooltip(Some(update.tooltip));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = request_tx.send(Request::Quit);
    let _ = worker.join();
}

fn worker_loop(
    config: Config,
    interval: Duration,
    requests: Receiver<Request>,
    updates: Sender<Update>,
) {
    loop {
        let update = match git_check_core::scan(&config, Backend::Cli, Depth::Quick) {
            Ok(snapshot) => update_from(&snapshot),
            Err(err) => Update {
                tier: Tier::Alert,
                tooltip: format!("git-check: scan failed — {err}"),
            },
        };
        if updates.send(update).is_err() {
            return;
        }
        match requests.recv_timeout(interval) {
            Ok(Request::Rescan) => continue,
            Ok(Request::Quit) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => continue,
        }
    }
}

/// Turn a snapshot into the two things the tray displays.
fn update_from(snapshot: &Snapshot) -> Update {
    let counters = snapshot.portfolio.counters();
    let mut tooltip = format!(
        "{} repos · {} dirty · {} unpushed",
        counters.repos, counters.dirty_repos, counters.ahead_commits
    );
    if counters.conflicts > 0 {
        tooltip.push_str(&format!(" · {} conflicts", counters.conflicts));
    }
    if counters.errors > 0 {
        tooltip.push_str(&format!(" · {} errors", counters.errors));
    }
    Update {
        tier: snapshot.portfolio.tier(),
        tooltip,
    }
}
