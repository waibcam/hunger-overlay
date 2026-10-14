//! Minimal local log file at %APPDATA%\HungerOverlay\overlay.log (deleted and restarted at startup once it exceeds 1 MB).
//! The log stays on this computer; it is never uploaded anywhere. Callers never log the API key.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Open log file handle (None until `init` succeeds; writes are then silently dropped).
static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Opens the log file in append mode, first deleting it if it has grown beyond 1 MB.
pub fn init() {
    let path = crate::config::data_dir().join("overlay.log");
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        *LOG.lock().unwrap() = Some(f);
    }
}

/// Appends one line prefixed with the UTC time of day (also echoed to stderr in debug builds).
pub fn write(msg: &str) {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (h, m, s) = ((t / 3600) % 24, (t / 60) % 60, t % 60);
    let line = format!("[{h:02}:{m:02}:{s:02} UTC] {msg}\n");
    if let Some(f) = LOG.lock().unwrap().as_mut() {
        let _ = f.write_all(line.as_bytes());
    }
    #[cfg(debug_assertions)]
    eprint!("{line}");
}

/// `format!`-style logging macro: `logf!("x = {}", x)` writes one line to the local log file.
#[macro_export]
macro_rules! logf {
    ($($t:tt)*) => { $crate::log::write(&format!($($t)*)) };
}
