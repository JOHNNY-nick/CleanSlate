//! Local log files (ENF-10): `<data_dir>\logs\cleanslate.<date>.log`, one
//! file per day, the 7 most recent kept. Nothing is ever sent anywhere
//! (ENF-11).

use std::path::Path;
use std::sync::OnceLock;

use tracing_appender::rolling::{RollingFileAppender, Rotation};

use crate::error::{CoreError, CoreResult};

/// Number of daily log files kept.
pub const KEPT_FILES: usize = 7;

static INSTALLED: OnceLock<()> = OnceLock::new();

/// Builds the daily rotating appender in `dir`.
pub fn appender(dir: &Path) -> CoreResult<RollingFileAppender> {
    std::fs::create_dir_all(dir).map_err(|e| CoreError::from_io(dir, e))?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("cleanslate")
        .filename_suffix("log")
        .max_log_files(KEPT_FILES)
        .build(dir)
        .map_err(|e| CoreError::InvalidInput(format!("log appender: {e}")))
}

/// Installs the global log subscriber once per process (later calls do
/// nothing). Best effort: logging must never stop the engine.
pub fn init(dir: &Path) {
    INSTALLED.get_or_init(|| {
        if let Ok(writer) = appender(dir) {
            let _ = tracing_subscriber::fmt()
                .with_writer(writer)
                .with_ansi(false)
                .with_max_level(tracing::Level::INFO)
                .try_init();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn daily_files_in_the_given_folder() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        let mut writer = appender(&logs).unwrap();
        writer.write_all(b"scan done\n").unwrap();
        writer.flush().unwrap();
        let files: Vec<String> = std::fs::read_dir(&logs)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(
            files[0].starts_with("cleanslate.") && files[0].ends_with(".log"),
            "{files:?}"
        );
    }
}
