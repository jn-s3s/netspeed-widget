//! File logging for a packaged app that has no console.
//!
//! `log.txt` under the app data directory is the only diagnostic surface a
//! windowed build has, so it has to survive being the only one: writes never
//! panic into the caller, the file rolls over at a size cap instead of
//! growing forever, and a failed write is mirrored to stderr where one
//! exists.
//!
//! Every line is tagged with the subsystem that emitted it, which keeps each
//! entry attributable without repeating the tag in the message text.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths::config_path;

/// Name of the log file inside the app data directory.
pub const LOG_FILE: &str = "log.txt";
/// Size at which the log rolls over.
const MAX_LOG_BYTES: u64 = 2_000_000;
/// How many rolled-over copies are kept.
const ROTATED_FILES: u32 = 2;

/// Guards the file so two threads cannot interleave a block.
static LOCK: Mutex<()> = Mutex::new(());
static LOG_PATH: LazyLock<PathBuf> = LazyLock::new(|| config_path(LOG_FILE));

/// Records a routine event.
pub fn info(subsystem: &str, message: &str) {
    write("INFO", subsystem, message);
}

/// Records a recovered failure the app worked around.
pub fn warning(subsystem: &str, message: &str) {
    write("WARN", subsystem, message);
}

/// Records a failure the caller could not recover from.
pub fn error(subsystem: &str, message: &str) {
    write("ERROR", subsystem, message);
}

/// Writes a separator block that groups the lines that follow it.
pub fn section(title: &str) {
    let stamp = now_iso();
    let rule = "-".repeat(59);
    let block = format!("{rule}\n------  {title}\n------  {stamp}\n{rule}\n");
    append(&block);
}

/// Writes the startup banner carrying the runtime and packaging mode.
pub fn startup(app_name: &str) {
    section(&format!(
        "Startup - {app_name} | exe={} | debug={}",
        env!("CARGO_PKG_NAME"),
        cfg!(debug_assertions)
    ));
}

fn write(level: &str, subsystem: &str, message: &str) {
    append(&format!(
        "[{}] - [{level}] [{}] {message}\n",
        now_iso(),
        subsystem.to_uppercase()
    ));
}

/// Appends one block, rolling the file when it passes the size cap.
fn append(block: &str) {
    let _guard = LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let path = &*LOG_PATH;
    if should_rotate(path, block.len() as u64) {
        rotate(path);
    }
    if let Err(err) = append_block(path, block) {
        eprintln!("[ERROR] [LOG] write failed: {err} | dropped: {block}");
    }
}

/// Opens the log in append mode and writes one block.
fn append_block(path: &Path, block: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(block.as_bytes())
}

/// True when appending `incoming` bytes would pass the size cap.
fn should_rotate(path: &Path, incoming: u64) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.len() + incoming > MAX_LOG_BYTES,
        Err(_) => false, // no file yet, nothing to roll
    }
}

/// Shifts the log through the numbered backups, dropping the oldest.
fn rotate(path: &PathBuf) {
    let stem = path.to_string_lossy().to_string();
    for index in (1..ROTATED_FILES).rev() {
        let older = PathBuf::from(format!("{stem}.{index}"));
        if older.exists() {
            let _ = fs::rename(&older, PathBuf::from(format!("{stem}.{}", index + 1)));
        }
    }
    if path.exists() {
        let _ = fs::rename(path, PathBuf::from(format!("{stem}.1")));
    }
}

/// Current UTC time as an ISO 8601 timestamp.
///
/// Hand-rolled rather than pulled from a date-time crate: the log only needs
/// a sortable stamp, and the app has no other reason to carry a calendar.
fn now_iso() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Converts a day count since the Unix epoch into a civil date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollover_obeys_cap_and_shifts_backups() {
        let directory =
            std::env::temp_dir().join(format!("netspeed-log-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("log.txt");
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_LOG_BYTES - 1).unwrap();
        drop(file);
        assert!(!should_rotate(&path, 1));
        assert!(should_rotate(&path, 2));
        fs::write(&path, "first").unwrap();
        rotate(&path);
        fs::write(&path, "second").unwrap();
        rotate(&path);
        fs::write(&path, "third").unwrap();
        rotate(&path);
        assert_eq!(
            fs::read_to_string(directory.join("log.txt.1")).unwrap(),
            "third"
        );
        assert_eq!(
            fs::read_to_string(directory.join("log.txt.2")).unwrap(),
            "second"
        );
        fs::remove_file(directory.join("log.txt.1")).unwrap();
        fs::remove_file(directory.join("log.txt.2")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn civil_dates_match_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_746), (2026, 10, 20));
    }
}
