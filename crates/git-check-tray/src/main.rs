//! `git-check-tray` — portfolio health as a tray icon.
//!
//! A background thread rescans the configured roots on the configured
//! interval (and whenever "Scan now" is chosen). The main thread owns the tray
//! icon so the platform's event loop stays on the thread that created it; each
//! finished scan replaces the icon colour, the tooltip and the popup menu.

mod autostart;
mod icon;
mod logging;
mod popup;

use std::fs::File;
use std::io::{ErrorKind, Result as IoResult};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use git_check_core::config::{Config, Root};
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
    /// Roots changed: use this config from now on and rescan.
    Config(Box<Config>),
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
        // tray-icon requires the event loop to be running before a status item
        // is created, not merely to exist.
        app.finishLaunching();
        for _ in 0..5 {
            pump_run_loop();
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn configure_app() {}

/// macOS: hand AppKit a slice of run-loop time.
///
/// The scan/UI loop below owns the main thread, so without this the status
/// item never lays out (its frame stays empty) and clicks are never dispatched
/// to it — the tray appears to do nothing. Other platforms have no such loop to
/// service; their tray backends run on their own threads.
#[cfg(target_os = "macos")]
fn pump_run_loop() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEventMask};
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode};
    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(marker);
    let until = NSDate::dateWithTimeIntervalSinceNow(0.05);
    // SAFETY: NSDefaultRunLoopMode is an immutable framework constant, and all
    // of this runs on the main thread.
    unsafe {
        if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(&until),
            NSDefaultRunLoopMode,
            true,
        ) {
            if std::env::var_os("GIT_CHECK_TRAY_TRACE").is_some() {
                eprintln!("macos event type={}", event.r#type().0);
            }
            // Running the run loop alone never dispatches mouse events to
            // views; the application has to send each event itself.
            app.sendEvent(&event);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn pump_run_loop() {}

/// Windows: drain the thread's message queue.
///
/// `tray-icon` hosts the icon in a hidden window, so without a message pump the
/// icon never appears and clicks are never dispatched — the same failure the
/// macOS run loop caused.
#[cfg(target_os = "windows")]
fn pump_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    // SAFETY: the queue belongs to this thread, which is the one that created
    // the tray; no window handles are dereferenced here.
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn pump_messages() {}

/// macOS: give the status item the NSMenu itself.
///
/// tray-icon normally receives clicks through a subview it installs over the
/// status item button. That subview does not take a first click while another
/// app is active (and an accessory app cannot activate itself), so clicks were
/// being swallowed. A menu attached to the NSStatusItem is opened by AppKit,
/// which gets the click regardless of the active app.
#[cfg(target_os = "macos")]
fn attach_menu(tray: &TrayIcon, menu: &tray_icon::menu::Menu) {
    use objc2_app_kit::NSMenu;
    use tray_icon::menu::ContextMenu;
    let Some(status_item) = tray.ns_status_item() else {
        return;
    };
    let pointer = menu.ns_menu();
    if pointer.is_null() {
        return;
    }
    // SAFETY: muda hands back the NSMenu it retains for this Menu, and the tray
    // keeps the Menu alive for as long as the status item shows it.
    let ns_menu = unsafe { &*(pointer as *const NSMenu) };
    status_item.setMenu(Some(ns_menu));
}

fn main() {
    configure_app();
    let log_path = logging::init();

    let config = match Config::load() {
        Ok(config) => config,
        Err(err) => {
            log::error!("cannot load the config: {err}");
            eprintln!("git-check-tray: {err}");
            std::process::exit(1);
        }
    };
    let interval = Duration::from_secs(config.interval_secs.max(5));
    log::info!(
        "starting git-check-tray; config={}, {} root(s), interval={}s, log={}",
        Config::path().display(),
        config.roots.len(),
        interval.as_secs(),
        log_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "disabled".to_string())
    );

    // Two trays would fight over the same tray host, so the second one exits.
    let _instance = match acquire_instance_lock() {
        Ok(lock) => lock,
        Err(err) if err.kind() == ErrorKind::WouldBlock => {
            log::info!("another instance holds the state lock; exiting");
            eprintln!("git-check-tray: already running");
            return;
        }
        Err(err) => {
            log::error!("cannot lock the state directory: {err}");
            eprintln!("git-check-tray: cannot lock the state directory: {err}");
            std::process::exit(1);
        }
    };

    #[allow(unused_mut)]
    let mut builder = TrayIconBuilder::new().with_tooltip("git-check: scanning");
    // macOS tints template images for the current menu bar appearance.
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_icon_templated(icon::for_tier(Tier::Ok));
    }
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder
            .with_icon(icon::for_tier(Tier::Ok))
            .with_title("git-check");
    }
    let tray = builder.build().unwrap_or_else(|err| {
        log::error!("cannot create the tray icon: {err}");
        eprintln!("git-check-tray: cannot create the tray icon: {err}");
        std::process::exit(1);
    });

    // Keep the OS login item in step with the config on every start: this
    // creates it when enabled, removes it when disabled, and refreshes the
    // executable path when the binary has moved.
    let mut launch_at_login = config.launch_at_login;
    if let Err(err) = autostart::apply(launch_at_login) {
        log::warn!("cannot reconcile the login item: {err}");
        eprintln!("git-check-tray: cannot update launch at login: {err}");
    } else {
        log::info!("login item reconciled (launch_at_login={launch_at_login})");
    }

    // Creating the queue before the tray exists mirrors the macOS launch setup.
    pump_messages();

    let (menu, mut ids) = popup::build(None, launch_at_login);
    #[cfg(target_os = "macos")]
    attach_menu(&tray, &menu);
    tray.set_menu(Some(Box::new(menu)));

    let (request_tx, request_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    let worker_config = config.clone();
    let worker = thread::spawn(move || worker_loop(worker_config, interval, request_rx, update_tx));

    let trace = std::env::var_os("GIT_CHECK_TRAY_TRACE").is_some();
    // Test hook: open the popup once after the first scan so the menu can be
    // inspected without synthesising a mouse click.
    let trace_menu = std::env::var_os("GIT_CHECK_TRAY_TRACE_MENU").is_some();
    let mut menu_shown = false;
    let menu_events = MenuEvent::receiver();
    let mut quitting = false;
    while !quitting {
        pump_run_loop();
        pump_messages();
        while let Ok(event) = menu_events.try_recv() {
            if event.id == ids.quit {
                log::info!("menu: quit");
                quitting = true;
                break;
            }
            if event.id == ids.scan {
                log::info!("menu: scan now");
                let _ = request_tx.send(Request::Rescan);
            }
            if event.id == ids.choose {
                log::info!("menu: choose folder");
                if let Some(folder) = choose_folder() {
                    let result = update_config(&Config::path(), |fresh| {
                        if has_root(fresh, &folder) {
                            false
                        } else {
                            fresh.roots.push(Root::new(folder.clone(), 3, None));
                            true
                        }
                    });
                    match result {
                        Ok(Some(fresh)) => {
                            log::info!("added root {}", folder.display());
                            let _ = request_tx.send(Request::Config(Box::new(fresh)));
                        }
                        Ok(None) => log::info!("{} is already a root", folder.display()),
                        Err(err) => {
                            log::error!("cannot update the config: {err}");
                            eprintln!("git-check-tray: cannot update the config: {err}");
                        }
                    }
                }
            }
            if event.id == ids.launch {
                let wanted = !launch_at_login;
                match autostart::apply(wanted) {
                    Ok(()) => {
                        launch_at_login = wanted;
                        log::info!("launch at login set to {wanted}");
                        // Reload before saving: the file may have changed since
                        // startup, and an unparseable file must not be replaced.
                        let result = update_config(&Config::path(), |fresh| {
                            let changed = fresh.launch_at_login != wanted;
                            fresh.launch_at_login = wanted;
                            changed
                        });
                        if let Err(err) = result {
                            log::warn!("cannot update the config: {err}");
                            eprintln!("git-check-tray: cannot update the config: {err}");
                        }
                    }
                    Err(err) => {
                        log::error!("cannot change launch at login: {err}");
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
                if trace {
                    match tray.rect() {
                        Some(rect) => eprintln!(
                            "tray rect: {}x{} at {},{}",
                            rect.size.width, rect.size.height, rect.position.x, rect.position.y
                        ),
                        None => eprintln!("tray rect: unavailable"),
                    }
                }
                if update.snapshot.is_some() {
                    let (menu, fresh_ids) = popup::build(update.snapshot.as_ref(), launch_at_login);
                    #[cfg(target_os = "macos")]
                    attach_menu(&tray, &menu);
                    tray.set_menu(Some(Box::new(menu)));
                    ids = fresh_ids;
                    if trace_menu && !menu_shown {
                        menu_shown = true;
                        tray.show_menu();
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = request_tx.send(Request::Quit);
    let _ = worker.join();
}

/// Apply one change to the config **as it is on disk now**, then save it.
///
/// The tray runs for days; the CLI (`git-check roots add`) or a text editor may
/// change the file meanwhile. Saving the copy loaded at startup would silently
/// undo those edits, so every write re-reads first. `change` returns whether it
/// modified anything; `Ok(None)` means there was nothing to save. A file that
/// no longer parses is reported, never overwritten.
fn update_config(
    path: &Path,
    change: impl FnOnce(&mut Config) -> bool,
) -> git_check_core::Result<Option<Config>> {
    let mut fresh = Config::load_from(path)?;
    if !change(&mut fresh) {
        return Ok(None);
    }
    fresh.save_to(path)?;
    Ok(Some(fresh))
}

/// Whether `folder` is already a root, comparing resolved paths so `~/code`
/// and `/home/you/code` (or a symlink to it) count as the same root.
fn has_root(config: &Config, folder: &Path) -> bool {
    let resolve = |p: &Path| {
        let expanded = Config::expand(p);
        expanded.canonicalize().unwrap_or(expanded)
    };
    let wanted = resolve(folder);
    config
        .roots
        .iter()
        .any(|root| resolve(&root.path) == wanted)
}

/// Native folder chooser; returns None when the user cancels.
fn choose_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Choose a folder to scan")
        .pick_folder()
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
            log::info!(
                "scan: {} repos, {} dirty, {} unpushed, tier {} ({} ms)",
                counters.repos,
                counters.dirty_repos,
                counters.ahead_commits,
                snapshot.portfolio.tier().as_str(),
                snapshot.duration_ms
            );
            let _ = tray.set_icon(Some(icon::for_tier(snapshot.portfolio.tier())));
            let _ = tray.set_tooltip(Some(tooltip));
        }
        None => {
            let tooltip = update
                .error
                .clone()
                .unwrap_or_else(|| "git-check: scan failed".to_string());
            log::error!("scan failed: {tooltip}");
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
    mut config: Config,
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
            Ok(Request::Config(next)) => {
                config = *next;
                continue;
            }
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

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("git-check-tray-cfg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saving_keeps_edits_made_while_the_tray_ran() {
        let dir = scratch("keep");
        let path = dir.join("config.toml");
        let startup = Config {
            roots: vec![Root::new("/a", 1, None)],
            ..Config::default()
        };
        startup.save_to(&path).unwrap();

        // Meanwhile, `git-check roots add /b` writes the file.
        let mut external = Config::load_from(&path).unwrap();
        external.roots.push(Root::new("/b", 1, None));
        external.save_to(&path).unwrap();

        // The tray toggles launch at login from its stale in-memory copy.
        let saved = update_config(&path, |fresh| {
            fresh.launch_at_login = true;
            true
        })
        .unwrap()
        .expect("changed");

        let on_disk = Config::load_from(&path).unwrap();
        assert!(on_disk.launch_at_login);
        assert_eq!(on_disk.roots.len(), 2, "the externally added root survives");
        assert_eq!(saved.roots, on_disk.roots);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unparseable_file_is_not_overwritten() {
        let dir = scratch("broken");
        let path = dir.join("config.toml");
        std::fs::write(&path, "roots = [ half-typed").unwrap();
        assert!(update_config(&path, |_| true).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "roots = [ half-typed"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_same_folder_is_recognised_however_it_is_spelled() {
        let dir = scratch("same");
        let config = Config {
            roots: vec![Root::new(&dir, 3, None)],
            ..Config::default()
        };
        assert!(has_root(&config, &dir.join(".")));
        assert!(!has_root(&config, &dir.join("other")));
        let _ = std::fs::remove_dir_all(dir);
    }
}
