//! File logging for the tray.
//!
//! The tray normally runs from Explorer, LaunchServices or a login item, so
//! stderr goes nowhere. A size-capped log under the state directory is the only
//! way to see what happened after the fact.
//!
//! Configure with:
//! * `GIT_CHECK_LOG` — `error`, `warn`, `info` (default), `debug` or `trace`.
//! * `GIT_CHECK_LOG_DIR` — override the directory (default `<state>/logs`).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use log::{LevelFilter, Log, Metadata, Record};

/// Rotate once the active file passes this size.
const MAX_BYTES: u64 = 1024 * 1024;
/// Number of rotated files kept besides the active one.
const KEEP: usize = 3;

/// Append-only log file that renames itself out of the way when it grows.
pub struct RotatingFile {
    path: PathBuf,
    file: File,
    size: u64,
    max_bytes: u64,
}

impl RotatingFile {
    pub fn open(path: PathBuf, max_bytes: u64) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            path,
            file,
            size,
            max_bytes,
        })
    }

    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let needed = line.len() as u64 + 1;
        if self.size + needed > self.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.size += needed;
        Ok(())
    }

    /// `<name>.log` becomes `<name>.log.1`, older files shift up, the oldest
    /// beyond [`KEEP`] is discarded.
    fn rotate(&mut self) -> io::Result<()> {
        for index in (1..KEEP).rev() {
            let from = self.rotated(index);
            if from.exists() {
                std::fs::rename(&from, self.rotated(index + 1))?;
            }
        }
        let _ = std::fs::remove_file(self.rotated(KEEP + 1));
        if Path::new(&self.path).exists() {
            std::fs::rename(&self.path, self.rotated(1))?;
        }
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.size = 0;
        Ok(())
    }

    fn rotated(&self, index: usize) -> PathBuf {
        let name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "git-check-tray.log".to_string());
        self.path.with_file_name(format!("{name}.{index}"))
    }
}

struct FileLogger {
    file: Mutex<RotatingFile>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!(
            "{stamp} {:<5} {} {}",
            record.level(),
            record.target(),
            record.args()
        );
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_line(&line);
        }
    }

    fn flush(&self) {}
}

fn parse_level(value: &str) -> LevelFilter {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => LevelFilter::Error,
        "warn" | "warning" => LevelFilter::Warn,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    }
}

/// Start logging to `<state>/logs/git-check-tray.log`.
///
/// Returns the log path on success. Failure is not fatal: the tray keeps
/// running and only loses its log.
pub fn init() -> Option<PathBuf> {
    let level = std::env::var("GIT_CHECK_LOG")
        .map(|value| parse_level(&value))
        .unwrap_or(LevelFilter::Info);
    log::set_max_level(level);

    let directory = std::env::var_os("GIT_CHECK_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| git_check_core::config::Config::state_dir().join("logs"));
    let path = directory.join("git-check-tray.log");

    let file = RotatingFile::open(path.clone(), MAX_BYTES).ok()?;
    static LOGGER: OnceLock<FileLogger> = OnceLock::new();
    let logger = LOGGER.get_or_init(|| FileLogger {
        file: Mutex::new(file),
    });
    log::set_logger(logger).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "git-check-tray-log-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rotates_and_keeps_a_bounded_number_of_files() {
        let dir = scratch("rotate");
        let path = dir.join("app.log");
        let mut file = RotatingFile::open(path.clone(), 120).unwrap();

        for index in 0..40 {
            file.write_line(&format!("line {index:03} padding padding"))
                .unwrap();
        }

        assert!(path.exists(), "active file");
        assert!(dir.join("app.log.1").exists(), "first rotation");
        assert!(dir.join("app.log.2").exists(), "second rotation");
        assert!(
            !dir.join("app.log.4").exists(),
            "never more than KEEP archives"
        );

        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let size = entry.metadata().unwrap().len();
            assert!(size <= 200, "{} grew to {size}", entry.path().display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn appends_to_an_existing_file() {
        let dir = scratch("append");
        let path = dir.join("app.log");
        std::fs::write(&path, "before\n").unwrap();
        let mut file = RotatingFile::open(path.clone(), 1024).unwrap();
        file.write_line("after").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("before\n"));
        assert!(text.ends_with("after\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn levels_parse_from_the_environment_string() {
        assert_eq!(parse_level("debug"), LevelFilter::Debug);
        assert_eq!(parse_level(" WARN "), LevelFilter::Warn);
        assert_eq!(parse_level("nonsense"), LevelFilter::Info);
    }
}
