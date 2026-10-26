//! Paths for per-user state and optional loose resources.
//!
//! Settings and logs use [`appdata_dir`]. Optional files beside the executable
//! use [`resource_path`]. Both return a `PathBuf`. The application icon is
//! embedded; the loose icon path is a runtime fallback.

use std::path::PathBuf;

/// Folder created under `%APPDATA%` for this app's settings and log.
pub const APP_DIR_NAME: &str = "NetSpeedWidget";

/// Filename of the optional loose icon used if the embedded icon cannot load.
pub const ICON_FILE: &str = "icon.ico";

/// Directory holding the per-user settings and the log file.
///
/// Tries to create the directory and returns its path even if creation fails.
/// Config and log writers report failures when they try to write. If `%APPDATA%`
/// is absent, uses `AppData\Roaming` under `%USERPROFILE%` or the current directory.
pub fn appdata_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join("AppData").join("Roaming")
        });
    let target = base.join(APP_DIR_NAME);
    // Best effort: config/log writers report failures at the actual write
    // boundary. Calling the logger here would recurse through its own path.
    let _ = std::fs::create_dir_all(&target);
    target
}

/// Returns a user state file path under the app data directory.
pub fn config_path(filename: &str) -> PathBuf {
    appdata_dir().join(filename)
}

/// Returns a resource path relative to the executable's directory.
///
/// Falls back to the current directory if the executable path is unavailable.
pub fn resource_path(relative: &str) -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(relative)
}

/// Returns the loose icon path used if loading the embedded icon fails.
pub fn icon_path() -> PathBuf {
    resource_path(ICON_FILE)
}
