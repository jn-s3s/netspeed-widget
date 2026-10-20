//! Win32 helpers: taskbar embedding, the tray icon, menus and fonts.

pub mod appearance;
pub mod details;
pub mod dialog;
pub mod gdi;
pub mod menu;
pub mod placement;
pub mod taskbar;
pub mod tooltip;
pub mod tray;

/// Foreground-window tests must not deactivate one another in parallel runs.
#[cfg(test)]
pub static UI_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
