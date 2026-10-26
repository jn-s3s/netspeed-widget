//! The notification area icon.
//!
//! The icon provides the same menu as the widget, including when taskbar
//! placement is unavailable or the user has hidden the widget.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{HICON, WM_USER};

/// Message the shell sends when the tray icon is clicked.
pub const TRAY_CALLBACK: u32 = WM_USER + 20;

/// A live notification area icon.
pub struct TrayIcon {
    data: NOTIFYICONDATAW,
    visible: bool,
}

impl TrayIcon {
    /// Adds the icon to the notification area.
    pub fn add(hwnd: HWND, id: u32, icon: HICON, tip: &str) -> Self {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: id,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK,
            hIcon: icon,
            ..Default::default()
        };
        set_tip(&mut data, tip);
        let visible = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
        Self { data, visible }
    }

    /// Whether the icon was accepted by the shell.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Replaces the tooltip.
    pub fn set_tooltip(&mut self, tip: &str) {
        let was_visible = self.visible;
        set_tip(&mut self.data, tip);
        // Explorer can restart without notifying our message-only controller.
        // A failed modify means the icon needs to be registered again.
        unsafe {
            if !self.visible || !Shell_NotifyIconW(NIM_MODIFY, &self.data).as_bool() {
                self.visible = Shell_NotifyIconW(NIM_ADD, &self.data).as_bool();
            }
        }
        if self.visible != was_visible {
            if self.visible {
                crate::logger::info("tray", "notification icon recovered");
            } else {
                crate::logger::warning("tray", "notification icon unavailable");
            }
        }
    }

    /// Removes the icon from the notification area.
    pub fn remove(&mut self) {
        if self.visible {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            }
            self.visible = false;
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        self.remove();
    }
}

/// Copies a tooltip into the fixed-size shell buffer.
fn set_tip(data: &mut NOTIFYICONDATAW, tip: &str) {
    let wide: Vec<u16> = tip.encode_utf16().take(data.szTip.len() - 1).collect();
    data.szTip = [0u16; 128];
    data.szTip[..wide.len()].copy_from_slice(&wide);
}
