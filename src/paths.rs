//! Where resources and per-user state live.
//!
//! Two roots matter. Read-only files that ship beside the executable resolve
//! through [`resource_path`]; user settings and the log resolve through
//! [`appdata_dir`]. Both return a `PathBuf` so callers never rebuild paths
//! by hand, which is what the config writer and the speedtest bundle need.

use std::path::PathBuf;

/// Folder created under `%APPDATA%` for this app's settings and log.
pub const APP_DIR_NAME: &str = "NetSpeedWidget";

/// The application icon, shared by the window chrome and the tray.
pub const ICON_FILE: &str = "icon.ico";

/// Optional manually installed fast-cli layout.
/// Standard releases do not contain Node or an npm bundle.
pub const THIRD_PARTY: &str = "third_party";
pub const NODE_DIR: &str = "node";
pub const NODE_BINARY: &str = "node.exe";
pub const FAST_BUNDLE_DIR: &str = "fast-bundle";
pub const FAST_CLI_ENTRY: [&str; 4] = ["node_modules", "fast-cli", "distribution", "cli.js"];

/// Directory holding the per-user settings and the log file.
///
/// The directory is created when it is missing. A directory that cannot be
/// created is still returned, because the config and log writers surface that
/// failure where the damage would actually land.
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
    let _ = std::fs::create_dir_all(&target);
    target
}

/// Absolute path to a user state file inside the app data directory.
pub fn config_path(filename: &str) -> PathBuf {
    appdata_dir().join(filename)
}

/// Absolute path to a resource that ships alongside the executable.
///
/// Optional loose resources resolve against the directory holding the executable.
/// Standard Rust releases embed the icon and use HTTPS without an extra runtime.
pub fn resource_path(relative: &str) -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(relative)
}

/// The application icon, shared by the window chrome and the tray.
pub fn icon_path() -> PathBuf {
    resource_path(ICON_FILE)
}

/// The `node.exe` in an optional manually installed provider bundle.
pub fn bundled_node_path() -> PathBuf {
    resource_path(THIRD_PARTY).join(NODE_DIR).join(NODE_BINARY)
}

/// Working directory for the optional fast-cli bundle.
pub fn fast_bundle_dir() -> PathBuf {
    resource_path(THIRD_PARTY).join(FAST_BUNDLE_DIR)
}

/// The fast-cli entry script inside the optional npm bundle.
pub fn fast_cli_entry_path() -> PathBuf {
    let mut path = fast_bundle_dir();
    for part in FAST_CLI_ENTRY {
        path.push(part);
    }
    path
}
