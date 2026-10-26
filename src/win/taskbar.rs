//! Taskbar discovery and child-window attachment.

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, ScreenToClient, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, FindWindowW, GetClassNameW, GetParent, GetWindowLongW,
    GetWindowRect, IsWindow, SetParent, SetWindowLongW, SetWindowPos, GWL_STYLE, HWND_TOP,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_CHILD, WS_CLIPSIBLINGS, WS_POPUP,
};

/// Attach only our own window; Explorer's styles and layout remain untouched.
pub fn embed_widget(widget: HWND, taskbar: HWND) -> windows::core::Result<()> {
    unsafe {
        let style = GetWindowLongW(widget, GWL_STYLE);
        SetWindowLongW(
            widget,
            GWL_STYLE,
            ((style as u32 & !WS_POPUP.0) | WS_CHILD.0 | WS_CLIPSIBLINGS.0) as i32,
        );
        // A successful first attachment returns a null previous parent, which
        // the bindings expose as an error. Verify the resulting parent instead.
        let result = SetParent(widget, taskbar);
        if GetParent(widget).ok() != Some(taskbar) {
            SetWindowLongW(widget, GWL_STYLE, style);
            return Err(result
                .err()
                .unwrap_or_else(windows::core::Error::from_win32));
        }
        SetWindowPos(
            widget,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED,
        )
    }
}

/// Convert a screen position to the parent taskbar's client coordinates.
pub fn client_point(taskbar: HWND, mut point: POINT) -> Option<POINT> {
    unsafe {
        ScreenToClient(taskbar, &mut point)
            .as_bool()
            .then_some(point)
    }
}

/// A taskbar window found on the desktop.
#[derive(Clone, Copy, Debug)]
pub struct TaskbarWindow {
    pub hwnd: HWND,
    /// Screen coordinates of the taskbar.
    pub rect: RECT,
    /// True for the main taskbar, false for a secondary one.
    pub is_primary: bool,
}

/// Class names of the notification area across Windows versions.
const TRAY_CLASSES: [&str; 5] = [
    "TrayNotifyWnd",
    "NotifyIconOverflowWindow",
    "ExplorerNotifyAreaWnd",
    "SysPager",
    "UserPromotedNotificationArea",
];

/// Collects every taskbar window on the desktop.
pub fn find_taskbars() -> Vec<TaskbarWindow> {
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: EnumWindows synchronously passes the exclusive vector pointer
        // supplied below; the vector lives until enumeration returns.
        let taskbars = unsafe { &mut *(lparam.0 as *mut Vec<TaskbarWindow>) };
        let class_name = class_name(hwnd);
        if class_name == "Shell_TrayWnd" || class_name == "Shell_SecondaryTrayWnd" {
            if let Some(rect) = taskbar_rect(hwnd) {
                taskbars.push(TaskbarWindow {
                    hwnd,
                    rect,
                    is_primary: class_name == "Shell_TrayWnd",
                });
            }
        }
        BOOL(1)
    }

    let mut taskbars: Vec<TaskbarWindow> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut taskbars as *mut Vec<TaskbarWindow> as isize),
        );
    }
    // Customized Explorer shells can omit the primary taskbar from EnumWindows
    // while it remains available by class lookup, just like embedded widgets.
    if !taskbars.iter().any(|bar| bar.is_primary) {
        // SAFETY: Static class string and a null title; no pointers are retained.
        if let Ok(hwnd) = unsafe {
            FindWindowW(
                windows::core::w!("Shell_TrayWnd"),
                windows::core::PCWSTR::null(),
            )
        } {
            if let Some(rect) = taskbar_rect(hwnd) {
                taskbars.push(TaskbarWindow {
                    hwnd,
                    rect,
                    is_primary: true,
                });
            }
        }
    }
    taskbars.sort_by_key(|taskbar| {
        (
            taskbar.rect.top,
            taskbar.rect.left,
            taskbar.rect.bottom,
            taskbar.rect.right,
        )
    });
    taskbars
}

/// The class name of a window.
pub fn class_name(hwnd: HWND) -> String {
    unsafe {
        let mut buffer = [0u16; 64];
        let length = GetClassNameW(hwnd, &mut buffer);
        if length <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..length as usize])
    }
}

/// The bounding rectangle of a window, in screen coordinates.
pub fn window_rect(hwnd: HWND) -> Option<RECT> {
    unsafe {
        let mut rect = RECT::default();
        match GetWindowRect(hwnd, &mut rect) {
            Ok(()) => Some(rect),
            Err(_) => None,
        }
    }
}

/// The taskbar rectangle, in screen coordinates.
///
/// Query geometry directly rather than sending synchronous appbar messages.
pub fn taskbar_rect(hwnd: HWND) -> Option<RECT> {
    // GetWindowRect does not synchronously message Explorer, unlike the appbar API.
    window_rect(hwnd)
}

/// Leave Explorer's top separator uncovered by one physical pixel at every DPI.
pub(crate) fn widget_rect(bar: RECT) -> Option<RECT> {
    let top = bar.top.checked_add(1)?;
    (top < bar.bottom).then_some(RECT { top, ..bar })
}

/// The state a descendant search carries.
struct DescendantSearch<'a> {
    classes: &'a [&'a str],
    found: Option<HWND>,
}

/// Finds a descendant window by class name.
///
/// The notification area moved between Windows releases and its wrapper
/// class differs per build, so the search walks the whole tree and matches
/// against a list of known names.
pub fn find_descendant(parent: HWND, classes: &[&str]) -> Option<HWND> {
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: EnumChildWindows synchronously passes the exclusive search
        // pointer supplied below; search and its borrowed classes remain live.
        let search = unsafe { &mut *(lparam.0 as *mut DescendantSearch) };
        if search.found.is_some() {
            return BOOL(0);
        }
        let name = class_name(hwnd);
        if search.classes.iter().any(|class| *class == name) {
            search.found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut search = DescendantSearch {
        classes,
        found: None,
    };
    unsafe {
        let _ = EnumChildWindows(
            parent,
            Some(enum_proc),
            LPARAM(&mut search as *mut DescendantSearch as isize),
        );
    }
    search.found
}

/// The left edge of the notification area, in screen coordinates.
///
/// Falls back to the right edge of the taskbar when the notification area
/// cannot be located, which keeps the widget from being pushed off screen.
pub fn tray_left(taskbar: HWND, taskbar_rect: RECT) -> i32 {
    find_descendant(taskbar, &TRAY_CLASSES)
        .and_then(window_rect)
        .map(|rect| rect.left)
        .unwrap_or(taskbar_rect.right)
}

/// The Windows display device name of the monitor a taskbar sits on.
///
/// The device name survives a taskbar being recreated and the display order
/// changing, which an index into the taskbar list would not.
pub fn monitor_device(taskbar: &TaskbarWindow) -> Option<String> {
    unsafe {
        let point = windows::Win32::Foundation::POINT {
            x: taskbar.rect.left + (taskbar.rect.right - taskbar.rect.left) / 2,
            y: taskbar.rect.top + (taskbar.rect.bottom - taskbar.rect.top) / 2,
        };
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO).as_bool() {
            let length = info
                .szDevice
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(info.szDevice.len());
            return Some(String::from_utf16_lossy(&info.szDevice[..length]));
        }
        None
    }
}

/// Whether a window still exists.
pub fn is_window(hwnd: HWND) -> bool {
    unsafe { IsWindow(hwnd).as_bool() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_bounds_preserve_one_physical_pixel_and_the_bottom_edge() {
        // Physical heights representative of several DPI settings and monitors.
        for (top, height) in [(1000, 48), (1000, 60), (-1440, 72), (-2160, 96), (0, 2)] {
            let bar = RECT {
                left: -1920,
                top,
                right: 0,
                bottom: top + height,
            };
            let widget = widget_rect(bar).expect("room below the separator");
            assert_eq!(widget.top, bar.top + 1);
            assert_eq!(widget.bottom, bar.bottom);
            assert_eq!(widget.left, bar.left);
            assert_eq!(widget.right, bar.right);
            assert_eq!(widget.bottom - widget.top, height - 1);
        }
    }

    #[test]
    fn widget_bounds_reject_taskbars_without_room_below_the_separator() {
        for (top, bottom) in [(0, 1), (0, 0), (1, 0), (i32::MAX, i32::MAX)] {
            assert!(widget_rect(RECT {
                left: 0,
                top,
                right: 1920,
                bottom,
            })
            .is_none());
        }
    }
}
