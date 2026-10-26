//! Native popup menus.
//!
//! The widget and the tray icon show the same menu, so it is built from a
//! small tree of entries that the app fills in from its current state.

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, PostMessageW, SetForegroundWindow, SetMenuItemInfoW,
    TrackPopupMenuEx, HMENU, MENUITEMINFOW, MFT_RADIOCHECK, MF_CHECKED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MIIM_FTYPE, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_NULL,
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
    /// A toggle that retains its check mark while unavailable.
    DependentCheck {
        id: u32,
        label: String,
        checked: bool,
        enabled: bool,
    },
    /// One choice in a mutually exclusive group.
    Radio {
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
                MenuEntry::Check { id, label, checked }
                | MenuEntry::DependentCheck {
                    id, label, checked, ..
                }
                | MenuEntry::Radio { id, label, checked } => {
                    let wide = to_wide(label);
                    let mut flags = if *checked { MF_CHECKED } else { MF_UNCHECKED };
                    if matches!(entry, MenuEntry::DependentCheck { enabled: false, .. }) {
                        flags |= MF_GRAYED;
                    }
                    let _ =
                        AppendMenuW(menu, MF_STRING | flags, *id as usize, PCWSTR(wide.as_ptr()));
                    if matches!(entry, MenuEntry::Radio { .. }) {
                        let info = MENUITEMINFOW {
                            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                            fMask: MIIM_FTYPE,
                            fType: MFT_RADIOCHECK,
                            ..Default::default()
                        };
                        let _ = SetMenuItemInfoW(menu, *id, false, &info);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMenuItemInfoW, GetSubMenu, MFS_CHECKED, MFS_GRAYED, MIIM_STATE,
    };

    #[test]
    fn native_menu_preserves_radio_toggle_and_disabled_check_states() {
        let entries = vec![MenuEntry::Submenu {
            label: "Options".to_string(),
            items: vec![
                MenuEntry::Radio {
                    id: 1,
                    label: "Selected".to_string(),
                    checked: true,
                },
                MenuEntry::Radio {
                    id: 2,
                    label: "Other".to_string(),
                    checked: false,
                },
                MenuEntry::Check {
                    id: 3,
                    label: "Toggle".to_string(),
                    checked: true,
                },
                MenuEntry::DependentCheck {
                    id: 4,
                    label: "Unavailable".to_string(),
                    checked: true,
                    enabled: false,
                },
                MenuEntry::DependentCheck {
                    id: 5,
                    label: "Available".to_string(),
                    checked: false,
                    enabled: true,
                },
            ],
        }];
        let menu = build(&entries).unwrap();
        // SAFETY: Both handles refer to live menus built above; info has the required size.
        unsafe {
            let submenu = GetSubMenu(menu, 0);
            for id in 1..=5 {
                let mut info = MENUITEMINFOW {
                    cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_FTYPE | MIIM_STATE,
                    ..Default::default()
                };
                GetMenuItemInfoW(submenu, id, false, &mut info).unwrap();
                assert_eq!(info.fType.contains(MFT_RADIOCHECK), id <= 2);
                assert_eq!(info.fState.contains(MFS_CHECKED), matches!(id, 1 | 3 | 4));
                assert_eq!(info.fState.0 & MFS_GRAYED.0 != 0, id == 4);
            }
            DestroyMenu(menu).unwrap();
        }
    }
}
