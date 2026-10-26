//! The hotkey capture dialog.
//!
//! The dialog listens for the next key combination, so the user does not have
//! to know the names of the virtual keys. A combo without Ctrl or Alt is
//! refused to avoid intercepting normal typing in other applications.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, DrawTextW, EndPaint, FillRect, GetMonitorInfoW, GetSysColor, GetSysColorBrush,
    InvalidateRect, MonitorFromWindow, SelectObject, SetBkMode, SetTextColor, COLOR_WINDOW,
    COLOR_WINDOWTEXT, HDC, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowRect, IsWindow, LoadCursorW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, TranslateMessage, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, MSG,
    SWP_NOSIZE, SWP_NOZORDER, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CHAR, WM_CLOSE, WM_DESTROY,
    WM_KEYDOWN, WM_PAINT, WM_SYSKEYDOWN, WNDCLASSEXW, WS_CAPTION, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
};

use crate::hotkey;
use crate::win::gdi::{to_wide, Font, WEIGHT_BOLD, WEIGHT_NORMAL};

/// Virtual key code of the escape key.
const VK_ESCAPE: u32 = 0x1B;
/// Virtual key code of the enter key.
const VK_RETURN: u32 = 0x0D;
/// Virtual key code of the control modifier (either side).
const VK_CONTROL: u32 = 0x11;
/// Virtual key code of the shift modifier (either side).
const VK_SHIFT: u32 = 0x10;
/// Virtual key code of the alt modifier (either side).
const VK_MENU: u32 = 0x12;
/// Bit set in a key state when the key is down.
const KEY_DOWN: i16 = -32768;

const CLASS_NAME: &str = "NetSpeedHotkeyDialog";
const DIALOG_WIDTH: i32 = 440;
const DIALOG_HEIGHT: i32 = 200;

/// The state the dialog window carries.
struct DialogState {
    current: String,
    captured: Option<String>,
    message: String,
    result: Option<String>,
    accept: fn(&str) -> Result<(), String>,
}

/// Opens the capture dialog and returns the chosen combo.
///
/// Returns `None` when the user cancelled or the capture was refused.
pub fn capture(
    parent: HWND,
    current: &str,
    accept: fn(&str) -> Result<(), String>,
) -> Option<String> {
    let mut state = Box::new(DialogState {
        current: current.to_string(),
        captured: None,
        message: "Ctrl or Alt required. Enter applies, Esc cancels.".to_string(),
        result: None,
        accept,
    });

    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }
        .map_err(|err| {
            crate::logger::error("hotkey", &format!("could not load dialog module: {err}"));
        })
        .ok()?;
    register_class(hinstance);
    let title = to_wide("Set hotkey");
    let class = to_wide(CLASS_NAME);
    let state_pointer = &mut *state as *mut DialogState;
    let dpi = unsafe { GetDpiForWindow(parent) }.max(96) as i32;

    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_VISIBLE.0),
            0,
            0,
            DIALOG_WIDTH * dpi / 96,
            DIALOG_HEIGHT * dpi / 96,
            HWND::default(),
            windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
            hinstance,
            None,
        )
        .map_err(|err| crate::logger::error("hotkey", &format!("could not create dialog: {err}")))
        .ok()?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_pointer as isize);
        center_on(hwnd, parent);
        let _ = SetForegroundWindow(hwnd);

        // A nested loop keeps the dialog modal without a dialog template.
        let mut msg = MSG::default();
        while IsWindow(hwnd).as_bool() {
            let result = GetMessageW(&mut msg, HWND::default(), 0, 0);
            if result.0 <= 0 {
                if result.0 == 0 {
                    PostQuitMessage(msg.wParam.0 as i32);
                }
                let _ = DestroyWindow(hwnd);
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    state.result.take()
}

/// Registers the dialog class once per process.
fn register_class(hinstance: HMODULE) {
    unsafe {
        let class = to_wide(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(dialog_proc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
    }
}

/// Centres the dialog on its owner, keeping it inside that monitor's work area.
fn center_on(hwnd: HWND, parent: HWND) {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let (mut x, mut y) = (100, 100);
        if !parent.is_invalid() {
            let mut owner = RECT::default();
            if GetWindowRect(parent, &mut owner).is_ok() {
                x = owner.left + (owner.right - owner.left - width) / 2;
                y = owner.top + (owner.bottom - owner.top - height) / 2;
            }
        }
        let monitor = MonitorFromWindow(
            if parent.is_invalid() { hwnd } else { parent },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            (x, y) = clamp_to_work_area(x, y, width, height, info.rcWork);
        }
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }
}

fn clamp_to_work_area(x: i32, y: i32, width: i32, height: i32, area: RECT) -> (i32, i32) {
    (
        x.clamp(area.left, (area.right - width).max(area.left)),
        y.clamp(area.top, (area.bottom - height).max(area.top)),
    )
}

/// The dialog window procedure.
unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED => {
            // SAFETY: WM_DPICHANGED supplies a RECT valid for this callback.
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            // SAFETY: hwnd is the dialog on its UI thread and suggested is the
            // Windows-provided rectangle. No Rust state borrow spans the call.
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER,
                )
            };
            // SAFETY: Invalidate the live dialog's entire client area.
            let _ = unsafe { InvalidateRect(hwnd, None, false) };
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: WM_PAINT supplies hwnd; ps is writable and paired below.
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rect = RECT::default();
            // SAFETY: hdc belongs to the active paint; rect is writable, and
            // system color constants identify borrowed, non-owned resources.
            unsafe {
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = FillRect(hdc, &rect, GetSysColorBrush(COLOR_WINDOW));
                let _ = SetTextColor(
                    hdc,
                    windows::Win32::Foundation::COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                );
            }
            // SAFETY: hwnd is the live dialog supplied to this procedure.
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
            let title_font = Font::native("Segoe UI", 15 * dpi / 96, WEIGHT_BOLD);
            let body_font = Font::native("Segoe UI", 13 * dpi / 96, WEIGHT_NORMAL);

            let mut y = 16 * dpi / 96;
            if let Some(font) = title_font.as_ref() {
                // SAFETY: The paint DC and font remain live through drawing.
                unsafe { draw_line(hdc, &rect, &mut y, "Press the new shortcut", font, dpi) };
            }
            let captured = with_state(hwnd, |state| match state.captured.clone() {
                Some(combo) => combo,
                None => hotkey::format_hotkey(&state.current),
            })
            .unwrap_or_default();
            if let Some(font) = body_font.as_ref() {
                // SAFETY: The paint DC and font remain live through drawing.
                unsafe { draw_line(hdc, &rect, &mut y, &captured, font, dpi) };
                if let Some(message) = with_state(hwnd, |state| state.message.clone()) {
                    // SAFETY: The same paint DC and font are still live.
                    unsafe { draw_line(hdc, &rect, &mut y, &message, font, dpi) };
                }
            }
            // SAFETY: ps is the PAINTSTRUCT initialized by BeginPaint above.
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            handle_key(hwnd, wparam.0 as u32);
            LRESULT(0)
        }
        WM_CHAR => {
            // Swallow the characters so the combo is not typed anywhere.
            LRESULT(0)
        }
        WM_CLOSE => {
            with_state(hwnd, |state| {
                state.result = None;
            });
            // SAFETY: This procedure runs on the dialog's owning UI thread;
            // with_state has released its borrow before destruction re-enters.
            if let Err(err) = unsafe { DestroyWindow(hwnd) } {
                crate::logger::warning("hotkey", &format!("could not close capture dialog: {err}"));
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // Closing a dialog must not terminate the application's message loop.
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Draws one line of text inside the dialog.
unsafe fn draw_line(hdc: HDC, rect: &RECT, y: &mut i32, text: &str, font: &Font, dpi: i32) {
    use windows::Win32::Graphics::Gdi::{DRAW_TEXT_FORMAT, DT_LEFT, DT_TOP, DT_WORDBREAK};
    // SAFETY: The caller provides a live paint DC and font; the previous font
    // is restored below before the borrowed font can be dropped.
    let old = unsafe { SelectObject(hdc, HGDIOBJ(font.handle().0)) };
    // SAFETY: hdc is the same live paint DC.
    let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let mut wide = to_wide(text);
    let text_len = wide.len() - 1;
    let mut line = RECT {
        left: rect.left + 16 * dpi / 96,
        top: *y,
        right: rect.right - 16 * dpi / 96,
        bottom: rect.bottom,
    };
    // SAFETY: The text slice and output rectangle remain valid for drawing.
    let _ = unsafe {
        DrawTextW(
            hdc,
            &mut wide[..text_len],
            &mut line,
            DRAW_TEXT_FORMAT(DT_LEFT.0 | DT_TOP.0 | DT_WORDBREAK.0),
        )
    };
    // SAFETY: old is the previous object returned for this DC above.
    let _ = unsafe { SelectObject(hdc, old) };
    *y += 30 * dpi / 96;
}

/// Reacts to a key press in the dialog.
fn handle_key(hwnd: HWND, vk: u32) {
    if vk == VK_ESCAPE {
        with_state(hwnd, |state| {
            state.result = None;
        });
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    }
    if vk == VK_RETURN {
        let accepted = with_state(hwnd, |state| state.captured.clone()).flatten();
        if let Some(combo) = accepted {
            let accept = with_state(hwnd, |state| state.accept);
            if let Some(Err(reason)) = accept.map(|accept| accept(&combo)) {
                with_state(hwnd, |state| {
                    state.message = format!("Unavailable: {reason}");
                });
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
                return;
            }
            with_state(hwnd, |state| {
                state.result = Some(combo);
            });
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return;
        }
    }
    if hotkey::ignored_capture_key(vk) {
        return;
    }
    let (ctrl, shift, alt) = modifier_state();
    with_state(hwnd, |state| {
        match hotkey::combo_from_capture(vk, ctrl, shift, alt) {
            Ok(combo) => {
                state.message = format!("Press Enter to use {}.", hotkey::format_hotkey(&combo));
                state.captured = Some(combo);
            }
            Err(reason) => {
                state.captured = None;
                state.message = reason;
            }
        }
    });
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// The current state of the modifier keys.
fn modifier_state() -> (bool, bool, bool) {
    unsafe {
        let down = |key: u32| GetKeyState(key as i32) & KEY_DOWN != 0;
        (down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU))
    }
}

/// Borrows state only for a data-only operation. Callbacks must not call Win32
/// or re-enter this helper; `capture` owns the box until the window is destroyed.
fn with_state<R>(hwnd: HWND, operation: impl FnOnce(&mut DialogState) -> R) -> Option<R> {
    unsafe {
        let pointer =
            windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if pointer == 0 {
            None
        } else {
            Some(operation(&mut *(pointer as *mut DialogState)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, PeekMessageW, PostMessageW, PostThreadMessageW, PM_REMOVE, WM_QUIT,
    };

    #[test]
    fn dialog_stays_inside_top_bottom_and_negative_monitor_work_areas() {
        let area = RECT {
            left: 0,
            top: 48,
            right: 1920,
            bottom: 1080,
        };
        assert_eq!(clamp_to_work_area(-156, -76, 440, 200, area), (0, 48));
        assert_eq!(clamp_to_work_area(1800, 1000, 440, 200, area), (1480, 880));
        let area = RECT {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1032,
        };
        assert_eq!(
            clamp_to_work_area(-2050, 1000, 440, 200, area),
            (-1920, 832)
        );
    }

    #[test]
    fn modifier_and_lock_keys_preserve_a_pending_capture() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let mut state = Box::new(DialogState {
            current: "ctrl+alt+n".to_string(),
            captured: Some("ctrl+shift+n".to_string()),
            message: "Press Enter to use Ctrl+Shift+N.".to_string(),
            result: None,
            accept: |_| Ok(()),
        });
        let class = to_wide("STATIC");
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE::default(),
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut *state as *mut DialogState) as isize,
            );
        }
        for vk in [
            0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0x90, 0x91, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5,
        ] {
            handle_key(hwnd, vk);
        }
        // Release the native window before inspecting (and dropping) its state.
        unsafe { DestroyWindow(hwnd) }.unwrap();
        assert_eq!(state.captured.as_deref(), Some("ctrl+shift+n"));
        assert_eq!(state.message, "Press Enter to use Ctrl+Shift+N.");
    }

    #[test]
    fn cancelling_capture_does_not_post_application_quit() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
        let closer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let class = to_wide(CLASS_NAME);
            while Instant::now() < deadline {
                if let Ok(hwnd) = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) } {
                    unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // Bound failure time if creation failed or the dialog was never found.
            let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
            false
        });
        assert!(capture(HWND::default(), "ctrl+alt+n", |_| Ok(())).is_none());
        assert!(closer.join().unwrap(), "capture window did not appear");
        let mut msg = MSG::default();
        assert!(
            !unsafe { PeekMessageW(&mut msg, HWND::default(), WM_QUIT, WM_QUIT, PM_REMOVE) }
                .as_bool()
        );
    }
}
