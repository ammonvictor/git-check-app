//! `git-check-tray` — portfolio health as a tray icon.
//!
//! A background thread rescans the configured roots on the configured
//! interval (and whenever "Scan now" is chosen). The main thread owns the tray
//! icon so the platform's event loop stays on the thread that created it; each
//! finished scan replaces the icon colour, the tooltip and the popup menu.

mod autostart;
mod icon;
mod popup;

use std::fs::File;
use std::io::{ErrorKind, Result as IoResult};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use git_check_core::config::Config;
use git_check_core::inspect::{Backend, Depth};
use git_check_core::metrics::Tier;
use git_check_core::Snapshot;
use tray_icon::menu::MenuEvent;
use tray_icon::{TrayIcon, TrayIconBuilder};

/// Result of one scan, or the reason it failed.
struct Update {
    snapshot: Option<Snapshot>,
    error: Option<String>,
}

/// Work the UI sends to the scan thread.
enum Request {
    Rescan,
    Quit,
}

/// macOS: become an accessory app so the tray lives in the menu bar without a
/// Dock icon or a Cmd-Tab entry.
#[cfg(target_os = "macos")]
fn configure_app() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    if let Some(marker) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(marker);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

#[cfg(not(target_os = "macos"))]
fn configure_app() {}

fn main() {
    configure_app();
    let mut config = match Config::load() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("git-check-tray: {err}");
            std::process::exit(1);
        }
    };
    let interval = Duration::from_secs(config.interval_secs.max(5));

    // Two trays would fight over the same tray host, so the second one exits.
    let _instance = match acquire_instance_lock() {
        Ok(lock) => lock,
        Err(err) if err.kind() == ErrorKind::WouldBlock => {
            eprintln!("git-check-tray: already running");
            return;
        }
        Err(err) => {
            eprintln!("git-check-tray: cannot lock the state directory: {err}");
            std::process::exit(1);
        }
    };

    let tray = TrayIconBuilder::new()
        .with_title("git-check")
        .with_tooltip("git-check: scanning")
        .with_icon(icon::for_tier(Tier::Ok))
        .build()
        .unwrap_or_else(|err| {
            eprintln!("git-check-tray: cannot create the tray icon: {err}");
            std::process::exit(1);
        });

    // Keep the OS login item in step with the config on every start: this
    // creates it when enabled, removes it when disabled, and refreshes the
    // executable path when the binary has moved.
    let mut launch_at_login = config.launch_at_login;
    if let Err(err) = autostart::apply(launch_at_login) {
        eprintln!("git-check-tray: cannot update launch at login: {err}");
    }

    let (menu, mut ids) = popup::build(None, launch_at_login);
    tray.set_menu(Some(Box::new(menu)));

    let (request_tx, request_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    let worker_config = config.clone();
    let worker = thread::spawn(move || worker_loop(worker_config, interval, request_rx, update_tx));

    let menu_events = MenuEvent::receiver();
    let mut quitting = false;
    while !quitting {
        while let Ok(event) = menu_events.try_recv() {
            if event.id == ids.quit {
                quitting = true;
                break;
            }
            if event.id == ids.scan {
                let _ = request_tx.send(Request::Rescan);
            }
            if event.id == ids.launch {
                let wanted = !launch_at_login;
                match autostart::apply(wanted) {
                    Ok(()) => {
                        launch_at_login = wanted;
                        config.launch_at_login = wanted;
                        if let Err(err) = config.save() {
                            eprintln!("git-check-tray: cannot save the config: {err}");
                        }
                    }
                    Err(err) => {
                        eprintln!("git-check-tray: cannot change launch at login: {err}");
                    }
                }
                ids.launch_item.set_checked(launch_at_login);
            }
        }
        if quitting {
            break;
        }
        match update_rx.recv_timeout(Duration::from_millis(120)) {
            Ok(update) => {
                apply(&tray, &update);
                if update.snapshot.is_some() {
                    let (menu, fresh_ids) = popup::build(update.snapshot.as_ref(), launch_at_login);
                    tray.set_menu(Some(Box::new(menu)));
                    ids = fresh_ids;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = request_tx.send(Request::Quit);
    let _ = worker.join();
}

/// Icon colour and tooltip for a finished scan, or the failure state.
fn apply(tray: &TrayIcon, update: &Update) {
    match &update.snapshot {
        Some(snapshot) => {
            let counters = snapshot.portfolio.counters();
            let mut tooltip = format!(
                "{} repos · {} dirty · {} unpushed",
                counters.repos, counters.dirty_repos, counters.ahead_commits
            );
            // The counts live in the tooltip rather than the icon, so they
            // stay legible at every DPI scaling.
            if let Some(scan) = chrono::DateTime::from_timestamp(snapshot.portfolio.generated_at, 0)
            {
                tooltip.push_str(&format!(
                    " · scanned {}",
                    scan.with_timezone(&chrono::Local).format("%H:%M")
                ));
            }
            if counters.conflicts > 0 {
                tooltip.push_str(&format!(" · {} conflicts", counters.conflicts));
            }
            if counters.errors > 0 {
                tooltip.push_str(&format!(" · {} errors", counters.errors));
            }
            let _ = tray.set_icon(Some(icon::for_tier(snapshot.portfolio.tier())));
            let _ = tray.set_tooltip(Some(tooltip));
        }
        None => {
            let tooltip = update
                .error
                .clone()
                .unwrap_or_else(|| "git-check: scan failed".to_string());
            let _ = tray.set_icon(Some(icon::for_tier(Tier::Alert)));
            let _ = tray.set_tooltip(Some(tooltip));
        }
    }
}

/// Hold an exclusive lock on the state directory for as long as the tray runs.
fn acquire_instance_lock() -> IoResult<File> {
    acquire_lock_in(&Config::state_dir())
}

fn acquire_lock_in(dir: &Path) -> IoResult<File> {
    std::fs::create_dir_all(dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("tray.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(std::io::Error::new(
            ErrorKind::WouldBlock,
            "another instance holds the lock",
        )),
        Err(std::fs::TryLockError::Error(err)) => Err(err),
    }
}

fn worker_loop(
    config: Config,
    interval: Duration,
    requests: Receiver<Request>,
    updates: Sender<Update>,
) {
    loop {
        let started = std::time::Instant::now();
        let update = match git_check_core::scan(&config, Backend::Cli, Depth::Full) {
            Ok(snapshot) => Update {
                snapshot: Some(snapshot),
                error: None,
            },
            Err(err) => Update {
                snapshot: None,
                error: Some(format!("git-check: scan failed — {err}")),
            },
        };
        if std::env::var_os("GIT_CHECK_TRAY_TRACE").is_some() {
            eprintln!("scan finished in {} ms", started.elapsed().as_millis());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_instance_holds_no_lock() {
        let dir = std::env::temp_dir().join(format!("git-check-tray-lock-{}", std::process::id()));
        let first = acquire_lock_in(&dir).expect("first instance");
        let second = acquire_lock_in(&dir);
        match second {
            Err(err) => assert_eq!(err.kind(), ErrorKind::WouldBlock),
            Ok(_) => panic!("second instance acquired the lock"),
        }
        drop(first);
        let third = acquire_lock_in(&dir).expect("lock is released with the first instance");
        drop(third);
        let _ = std::fs::remove_dir_all(dir);
    }
}
