//! Native popup menus.
//!
//! The widget and the tray icon show the same menu, so it is built from a
//! small tree of entries that the app fills in from its current state.

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, PostMessageW, SetForegroundWindow, TrackPopupMenuEx,
    HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

use crate::win::gdi::to_wide;

/// One entry of a popup menu.
pub enum MenuEntry {
    /// A plain command.
    Command { id: u32, label: String },
    /// Informational text, not a selectable command.
    Disabled { label: String },
    /// A command carrying a check mark.
    Check {
        id: u32,
        label: String,
        checked: bool,
    },
    /// A separator line.
    Separator,
    /// A submenu.
    Submenu {
        label: String,
        items: Vec<MenuEntry>,
    },
}

/// Shows the menu at the given screen position and returns the chosen id.
///
/// The menu is modal, so this call blocks until the user picks an entry or
/// dismisses it.
pub fn show(hwnd: HWND, entries: &[MenuEntry], x: i32, y: i32) -> Option<u32> {
    let menu = build(entries)?;
    // The menu needs the foreground window to be ours, otherwise it closes
    // as soon as it loses the mouse.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        // Do not use TPM_WORKAREA: the cursor can be on the taskbar, and the
        // menu should overlap it rather than being pushed into the desktop.
        let result = TrackPopupMenuEx(
            menu,
            (TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY).0,
            x,
            y,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        // Releases the foreground lock the menu took.
        let _ = PostMessageW(hwnd, WM_NULL, None, None);
        if result.as_bool() {
            return Some(result.0 as u32);
        }
    }
    None
}

/// Builds the native menu tree from the entry list.
fn build(entries: &[MenuEntry]) -> Option<HMENU> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for entry in entries {
            match entry {
                MenuEntry::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                MenuEntry::Command { id, label } => {
                    let wide = to_wide(label);
                    let _ = AppendMenuW(menu, MF_STRING, *id as usize, PCWSTR(wide.as_ptr()));
                }
                MenuEntry::Disabled { label } => {
                    let wide = to_wide(label);
                    let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(wide.as_ptr()));
                }
                MenuEntry::Check { id, label, checked } => {
                    let wide = to_wide(label);
                    let flags = if *checked { MF_CHECKED } else { MF_UNCHECKED };
                    let _ =
                        AppendMenuW(menu, MF_STRING | flags, *id as usize, PCWSTR(wide.as_ptr()));
                }
                MenuEntry::Submenu { label, items } => {
                    let submenu = match build(items) {
                        Some(submenu) => submenu,
                        None => continue,
                    };
                    let wide = to_wide(label);
                    let _ = AppendMenuW(menu, MF_POPUP, submenu.0 as usize, PCWSTR(wide.as_ptr()));
                }
            }
        }
        Some(menu)
    }
}
