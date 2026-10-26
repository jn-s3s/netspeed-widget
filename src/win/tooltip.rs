//! Native, non-activating hover details. Text storage outlives the native tool.

use crate::win::gdi::to_wide;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::Controls::{
    TOOLTIPS_CLASSW, TTDT_INITIAL, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_DELTOOLW, TTM_SETDELAYTIME,
    TTM_SETMAXTIPWIDTH, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

pub struct Tooltip {
    hwnd: HWND,
    tool: TTTOOLINFOW,
    text: Vec<u16>,
}

impl Tooltip {
    pub fn new(owner: HWND, width: i32, height: i32) -> Option<Self> {
        use windows::Win32::UI::Controls::{
            InitCommonControlsEx, ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX, TTDT_AUTOPOP,
        };
        unsafe {
            let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_WIN95_CLASSES,
            });
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                PCWSTR::null(),
                WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX),
                0,
                0,
                0,
                0,
                owner,
                None,
                None,
                None,
            )
        }
        .ok()?;
        let mut value = Self {
            hwnd,
            tool: TTTOOLINFOW {
                // V2 size works with both classic and themed common controls.
                // V5 rejects the newer lpReserved field even on current Windows.
                cbSize: std::mem::offset_of!(TTTOOLINFOW, lpReserved) as u32,
                uFlags: TTF_SUBCLASS,
                hwnd: owner,
                uId: 1,
                rect: RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                ..Default::default()
            },
            text: to_wide("NetSpeed"),
        };
        value.tool.lpszText = PWSTR(value.text.as_mut_ptr());
        unsafe {
            SendMessageW(hwnd, TTM_SETMAXTIPWIDTH, WPARAM(0), LPARAM(380));
            SendMessageW(
                hwnd,
                TTM_SETDELAYTIME,
                WPARAM(TTDT_INITIAL as usize),
                LPARAM(600),
            );
            SendMessageW(
                hwnd,
                TTM_SETDELAYTIME,
                WPARAM(TTDT_AUTOPOP as usize),
                LPARAM(30000),
            );
            SendMessageW(
                hwnd,
                TTM_ADDTOOLW,
                WPARAM(0),
                LPARAM((&value.tool as *const TTTOOLINFOW) as isize),
            );
        }
        Some(value)
    }

    pub fn hide(&self) {
        unsafe {
            SendMessageW(
                self.hwnd,
                windows::Win32::UI::Controls::TTM_POP,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    /// Disables hover tracking during menus, dismissing any visible or pending tip.
    pub fn set_active(&self, active: bool) {
        // SAFETY: The tooltip window is owned by this object on the UI thread.
        unsafe {
            SendMessageW(
                self.hwnd,
                windows::Win32::UI::Controls::TTM_ACTIVATE,
                WPARAM(usize::from(active)),
                LPARAM(0),
            );
        }
    }

    pub fn update(&mut self, text: &str, width: i32, height: i32) {
        use windows::Win32::UI::Controls::TTM_NEWTOOLRECTW;
        if self.text != to_wide(text) {
            self.text = to_wide(text);
            self.tool.lpszText = PWSTR(self.text.as_mut_ptr());
            unsafe {
                SendMessageW(
                    self.hwnd,
                    TTM_UPDATETIPTEXTW,
                    WPARAM(0),
                    LPARAM((&self.tool as *const TTTOOLINFOW) as isize),
                );
            }
        }
        self.tool.rect.right = width;
        self.tool.rect.bottom = height;
        unsafe {
            let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(self.tool.hwnd).max(96);
            SendMessageW(
                self.hwnd,
                TTM_SETMAXTIPWIDTH,
                WPARAM(0),
                LPARAM((380 * dpi / 96) as isize),
            );
            SendMessageW(
                self.hwnd,
                TTM_NEWTOOLRECTW,
                WPARAM(0),
                LPARAM((&self.tool as *const TTTOOLINFOW) as isize),
            );
        }
    }
}

impl Drop for Tooltip {
    fn drop(&mut self) {
        unsafe {
            SendMessageW(
                self.hwnd,
                TTM_DELTOOLW,
                WPARAM(0),
                LPARAM((&self.tool as *const TTTOOLINFOW) as isize),
            );
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tool_tracks_its_owner_and_releases_resources() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let class = to_wide("STATIC");
        unsafe {
            let owner = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                224,
                48,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let mut tooltip = Tooltip::new(owner, 224, 48)
                .expect("native tooltip must initialize common controls");
            let hwnd = tooltip.hwnd;
            assert_ne!(
                windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE,
                ) as u32
                    & WS_EX_TOPMOST.0,
                0,
                "hover details must stack above the taskbar without activation"
            );
            assert_eq!(
                SendMessageW(
                    hwnd,
                    windows::Win32::UI::Controls::TTM_GETTOOLCOUNT,
                    WPARAM(0),
                    LPARAM(0)
                )
                .0,
                1
            );
            tooltip.set_active(false);
            tooltip.update("Download: 24.8 Mbps\nUpload: 1.6 Mbps", 128, 48);
            assert_eq!(tooltip.tool.rect.right, 128);
            tooltip.set_active(true);
            assert_eq!(
                SendMessageW(
                    hwnd,
                    windows::Win32::UI::Controls::TTM_GETTOOLCOUNT,
                    WPARAM(0),
                    LPARAM(0)
                )
                .0,
                1,
                "menu suppression must preserve the registered hover tool"
            );
            tooltip.hide();
            drop(tooltip);
            assert!(!windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd).as_bool());
            let _ = DestroyWindow(owner);
        }
    }
}
