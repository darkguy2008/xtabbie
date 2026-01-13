//! Logging utilities for xtabbie debugging.

use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static LOG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Get the log file path following XDG Base Directory spec.
fn get_log_path() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state_home).join("xtabbie").join("xtabbie.log")
}

/// Enable logging and create the log directory if needed.
pub fn enable() {
    let path = get_log_path();
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }
    LOG_ENABLED.store(true, Ordering::Relaxed);
    log(&format!("Logging enabled, writing to: {}", path.display()));
}

/// Check if logging is enabled.
pub fn is_enabled() -> bool {
    LOG_ENABLED.load(Ordering::Relaxed)
}

/// Clear the log file (truncate to zero length).
pub fn clear() {
    if !is_enabled() {
        return;
    }
    let path = get_log_path();
    let _ = File::create(&path);
}

/// Log a message with timestamp.
pub fn log(msg: &str) {
    if !is_enabled() {
        return;
    }
    let path = get_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{}", msg);
    }
}

/// Log a formatted message (convenience macro-like function).
#[macro_export]
macro_rules! log_fmt {
    ($($arg:tt)*) => {
        $crate::log::log(&format!($($arg)*))
    };
}
