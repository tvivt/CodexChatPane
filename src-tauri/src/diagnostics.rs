use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 20 * 1024 * 1024;
const MAX_LOG_FILES: usize = 5;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Default for Level {
    fn default() -> Self {
        Self::Info
    }
}

impl Level {
    fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warn => 1,
            Self::Info => 2,
            Self::Debug => 3,
        }
    }

    fn from_rank(rank: u8) -> Self {
        match rank {
            0 => Self::Error,
            1 => Self::Warn,
            3 => Self::Debug,
            _ => Self::Info,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    fn allows(self, record: Self) -> bool {
        record.rank() <= self.rank()
    }
}

struct LoggerInner {
    level: AtomicU8,
    path: PathBuf,
    write_lock: Mutex<()>,
}

#[derive(Clone)]
pub struct Logger(Arc<LoggerInner>);

impl Logger {
    pub fn new(path: PathBuf, level: Level) -> Self {
        Self(Arc::new(LoggerInner {
            level: AtomicU8::new(level.rank()),
            path,
            write_lock: Mutex::new(()),
        }))
    }

    pub fn level(&self) -> Level {
        Level::from_rank(self.0.level.load(Ordering::Relaxed))
    }

    pub fn set_level(&self, level: Level) {
        self.0.level.store(level.rank(), Ordering::Relaxed);
    }

    pub fn enabled(&self, level: Level) -> bool {
        self.level().allows(level)
    }

    pub fn log(&self, level: Level, message: impl AsRef<str>) {
        if !self.enabled(level) {
            return;
        }
        let Ok(_guard) = self.0.write_lock.lock() else {
            return;
        };
        let Some(parent) = self.0.path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let rotate = fs::metadata(&self.0.path)
            .map(|metadata| metadata.len() >= MAX_LOG_BYTES)
            .unwrap_or(false);
        if rotate {
            rotate_logs(&self.0.path);
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.0.path);
        let Ok(mut file) = file else {
            return;
        };
        if rotate {
            let _ = writeln!(
                file,
                "{} [info] log rotated after reaching {} bytes retainedFiles={}",
                timestamp(),
                MAX_LOG_BYTES,
                MAX_LOG_FILES
            );
        }
        let _ = writeln!(
            file,
            "{} [{}] [thread={:?}] {}",
            timestamp(),
            level.as_str(),
            std::thread::current().id(),
            message.as_ref()
        );
    }

    pub fn error(&self, message: impl AsRef<str>) {
        self.log(Level::Error, message);
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        self.log(Level::Warn, message);
    }

    pub fn info(&self, message: impl AsRef<str>) {
        self.log(Level::Info, message);
    }

    pub fn debug(&self, message: impl AsRef<str>) {
        self.log(Level::Debug, message);
    }
}

fn rotate_logs(path: &Path) {
    let _ = fs::remove_file(rotated_log_path(path, MAX_LOG_FILES));
    for index in (1..MAX_LOG_FILES).rev() {
        let target = rotated_log_path(path, index);
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            rotated_log_path(path, index - 1)
        };
        let _ = fs::remove_file(&target);
        let _ = fs::rename(source, target);
    }
}

fn rotated_log_path(path: &Path, index: usize) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(format!(".{index}"));
    value.into()
}

fn timestamp() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", elapsed.as_secs(), elapsed.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::{rotate_logs, Level, Logger, MAX_LOG_FILES};
    use std::fs;

    #[test]
    fn logger_filters_by_level_and_writes_debug_after_switch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("window-attach.log");
        let logger = Logger::new(path.clone(), Level::Info);

        logger.debug("hidden debug");
        logger.info("visible info");
        let contents = fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("hidden debug"));
        assert!(contents.contains("visible info"));

        logger.set_level(Level::Debug);
        logger.debug("visible debug");
        assert!(fs::read_to_string(path).unwrap().contains("visible debug"));
    }

    #[test]
    fn rotates_active_log_and_keeps_five_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("window-attach.log");
        fs::write(&path, "active").unwrap();
        for index in 1..=MAX_LOG_FILES {
            fs::write(
                super::rotated_log_path(&path, index),
                format!("archive-{index}"),
            )
            .unwrap();
        }

        rotate_logs(&path);

        assert_eq!(
            fs::read_to_string(super::rotated_log_path(&path, 1)).unwrap(),
            "active"
        );
        assert_eq!(
            fs::read_to_string(super::rotated_log_path(&path, 4)).unwrap(),
            "archive-3"
        );
        assert!(!super::rotated_log_path(&path, MAX_LOG_FILES).exists());
    }
}
