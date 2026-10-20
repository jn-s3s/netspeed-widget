//! Keyboard-accessible native details panel with a larger labeled history graph.
//! No application mutex may be held when calling into this module.

use crate::canvas::{Canvas, Rect};
use crate::graph::TrafficGraph;
use crate::sampler::NetSample;
use crate::theme::Palette;
use crate::win::appearance::system_color;
use crate::win::gdi::{blit_opaque, to_wide, Font};
use std::cell::Cell;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetSysColor, InvalidateRect, COLOR_WINDOW, COLOR_WINDOWTEXT, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_GETFIRSTVISIBLELINE, EM_GETSEL, EM_LINESCROLL, EM_SETSEL};
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: &str = "NetSpeedDetails";
thread_local! { static PANEL: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) }; }
thread_local! { static DEACTIVATED: Cell<Option<Instant>> = const { Cell::new(None) }; }

#[derive(Clone)]
pub struct Snapshot {
    pub body: String,
    pub rates: String,
    pub samples: Vec<NetSample>,
    pub palette: Palette,
    pub fill: bool,
    pub testing: bool,
}

struct PanelState {
    snapshot: Snapshot,
    graph: TrafficGraph,
    body: HWND,
    owner: HWND,
    font: Option<Font>,
    title_font: Option<Font>,
    scale: f32,
    run_command: u32,
    settings_command: u32,
}

pub fn translate(message: &MSG) -> bool {
    let hwnd = PANEL.get();
    if hwnd.is_invalid() || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return false;
    }
    if message.message == WM_KEYDOWN
        && message.wParam.0 == 0x1B
        && (message.hwnd == hwnd || unsafe { IsChild(hwnd, message.hwnd) }.as_bool())
    {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        return true;
    }
    unsafe { IsDialogMessageW(hwnd, message).as_bool() }
}

pub fn hide() {
    let hwnd = PANEL.get();
    if !hwnd.is_invalid() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

pub fn close() {
    let hwnd = PANEL.replace(HWND::default());
    if !hwnd.is_invalid() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    DEACTIVATED.set(None);
}

pub fn update(snapshot: Snapshot) {
    let hwnd = PANEL.get();
    if hwnd.is_invalid() || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return;
    }
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PanelState;
        if pointer.is_null() {
            return;
        }
        let (changed, body) = {
            let state = &mut *pointer;
            let changed =
                state.snapshot.body != snapshot.body || state.snapshot.rates != snapshot.rates;
            state.snapshot = snapshot;
            (changed, state.body)
        };
        // Setting native text sends WM_COMMAND synchronously; hold no state borrow.
        if changed {
            let text = to_wide(&format!(
                "Live: {}\r\n{}",
                (*pointer).snapshot.rates,
                (*pointer).snapshot.body
            ));
            let first = SendMessageW(body, EM_GETFIRSTVISIBLELINE, WPARAM(0), LPARAM(0));
            let (mut start, mut end) = (0_u32, 0_u32);
            SendMessageW(
                body,
                EM_GETSEL,
                WPARAM((&mut start as *mut u32) as usize),
                LPARAM((&mut end as *mut u32) as isize),
            );
            let _ = SetWindowTextW(body, PCWSTR(text.as_ptr()));
            SendMessageW(
                body,
                EM_SETSEL,
                WPARAM(start as usize),
                LPARAM(end as isize),
            );
            let current = SendMessageW(body, EM_GETFIRSTVISIBLELINE, WPARAM(0), LPARAM(0));
            SendMessageW(body, EM_LINESCROLL, WPARAM(0), LPARAM(first.0 - current.0));
        }
        let button = GetDlgItem(hwnd, 1).unwrap_or_default();
        let label = to_wide(if (*pointer).snapshot.testing {
            "Testing..."
        } else {
            "&Run speedtest"
        });
        let _ = SetWindowTextW(button, PCWSTR(label.as_ptr()));
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
            button,
            !(*pointer).snapshot.testing,
        );
        let _ = InvalidateRect(hwnd, None, false);
    }
}

pub fn toggle(
    owner: HWND,
    anchor: RECT,
    dpi: u32,
    snapshot: Snapshot,
    run_command: u32,
    settings_command: u32,
) {
    // Clicking the taskbar first deactivates the panel, then delivers mouse-up.
    // Do not interpret that close click as a request to reopen it immediately.
    if DEACTIVATED
        .take()
        .is_some_and(|time| time.elapsed() < Duration::from_millis(350))
    {
        return;
    }
    let existing = PANEL.get();
    if !existing.is_invalid() && unsafe { IsWindowVisible(existing) }.as_bool() {
        hide();
        return;
    }
    // Recreate on each open so monitor DPI, fonts and anchor are always current.
    close();
    let Ok(instance) = (unsafe { GetModuleHandleW(PCWSTR::null()) }) else {
        return;
    };
    let class = to_wide(CLASS);
    let title = to_wide("NetSpeed - Network details");
    let scale = dpi.max(96) as f32 / 96.0;
    let state = Box::new(PanelState {
        snapshot,
        graph: TrafficGraph::new(),
        body: HWND::default(),
        owner,
        font: Font::new("Segoe UI", (12.0 * scale).round() as i32, 400),
        title_font: Font::new("Segoe UI", (14.0 * scale).round() as i32, 600),
        scale,
        run_command,
        settings_command,
    });
    let pointer = Box::into_raw(state);
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(panel_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        let width = (340.0 * scale).round() as i32;
        let height = (490.0 * scale).round() as i32;
        let monitor = windows::Win32::Graphics::Gdi::MonitorFromRect(
            &anchor,
            windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST,
        );
        let mut info = windows::Win32::Graphics::Gdi::MONITORINFO {
            cbSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = windows::Win32::Graphics::Gdi::GetMonitorInfoW(monitor, &mut info);
        let area = info.rcWork;
        let x = (anchor.right - width).clamp(area.left, (area.right - width).max(area.left));
        let y = if anchor.top - height >= area.top {
            anchor.top - height - 4
        } else {
            anchor.bottom + 4
        }
        .clamp(area.top, (area.bottom - height).max(area.top));
        // Install state after creation; the procedure does not need it during creation.
        match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
            x,
            y,
            width,
            height,
            owner,
            None,
            instance,
            None,
        ) {
            Ok(hwnd) => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, pointer as isize);
                PANEL.set(hwnd);
                build_controls(hwnd, &mut *pointer);
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus((*pointer).body);
            }
            Err(err) => {
                crate::logger::error("details", &format!("could not create details panel: {err}"));
                drop(Box::from_raw(pointer));
            }
        }
    }
}

unsafe fn build_controls(hwnd: HWND, state: &mut PanelState) {
    let s = state.scale;
    let edit = to_wide("EDIT");
    let text = to_wide(&format!(
        "Live: {}\r\n{}",
        state.snapshot.rates, state.snapshot.body
    ));
    state.body = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        PCWSTR(edit.as_ptr()),
        PCWSTR(text.as_ptr()),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE(ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32),
        (16.0 * s) as i32,
        (176.0 * s) as i32,
        (300.0 * s) as i32,
        (210.0 * s) as i32,
        hwnd,
        HMENU(netspeed_widget::commands::DETAILS_BODY_ID as usize as *mut _),
        None,
        None,
    )
    .unwrap_or_default();
    if let Some(font) = &state.font {
        font.apply_to(state.body);
    }
    for (index, label) in ["&Run speedtest", "&Settings", "&Close"].iter().enumerate() {
        let class = to_wide("BUTTON");
        let label = to_wide(label);
        if let Ok(button) = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR(label.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ((16.0 + index as f32 * 104.0) * s) as i32,
            (402.0 * s) as i32,
            (96.0 * s) as i32,
            (28.0 * s) as i32,
            hwnd,
            HMENU((index + 1) as *mut _),
            None,
            None,
        ) {
            if let Some(font) = &state.font {
                font.apply_to(button);
            }
        }
    }
}

unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PanelState;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    match message {
        WM_PAINT => {
            let state = &mut *pointer;
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let mut canvas = Canvas::new(rect.right.max(1), rect.bottom.max(1));
            let background = system_color(GetSysColor(COLOR_WINDOW));
            let palette = crate::theme::native_panel_palette(
                state.snapshot.palette,
                background,
                system_color(GetSysColor(COLOR_WINDOWTEXT)),
                crate::win::appearance::Appearance::current().high_contrast,
            );
            canvas.fill_rounded_rect(
                Rect::new(0.0, 0.0, rect.right as f32, rect.bottom as f32),
                0.0,
                system_color(GetSysColor(COLOR_WINDOW)),
            );
            let s = state.scale;
            if let Some(font) = &state.title_font {
                canvas.draw_text(16.0 * s, 12.0 * s, &state.snapshot.rates, font, palette.fg);
            }
            state.graph.draw(
                &mut canvas,
                Rect::new(16.0 * s, 70.0 * s, 300.0 * s, 66.0 * s),
                &state.snapshot.samples,
                &palette,
                state.snapshot.fill,
                s,
            );
            if let Some(font) = &state.font {
                canvas.draw_text(
                    16.0 * s,
                    42.0 * s,
                    "Traffic · last 60 seconds",
                    font,
                    palette.fg,
                );
                canvas.draw_text(
                    16.0 * s,
                    146.0 * s,
                    &format!("Shared auto scale: {:.1} Mbps", state.graph.scale()),
                    font,
                    palette.fg,
                );
            }
            blit_opaque(dc, &canvas);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            if !(1..=3).contains(&id) || wparam.0 >> 16 != BN_CLICKED as usize {
                return LRESULT(0);
            }
            let state = &*pointer;
            let command = match id {
                1 => Some(state.run_command),
                2 => Some(state.settings_command),
                _ => None,
            };
            let owner = state.owner;
            if let Some(command) = command {
                let _ = PostMessageW(owner, WM_COMMAND, WPARAM(command as usize), LPARAM(0));
            }
            if id != 1 {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        // Reopening rebuilds controls at the new monitor's DPI.
        WM_DPICHANGED => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_ACTIVATE if wparam.0 & 0xFFFF == WA_INACTIVE as usize => {
            DEACTIVATED.set(Some(Instant::now()));
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            PANEL.set(HWND::default());
            drop(Box::from_raw(pointer));
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            body: "TCP latency: 8 ms\r\nSession: 1 MB".to_string(),
            rates: "↓ 24.8 Mbps    ↑ 1.6 Mbps".to_string(),
            samples: Vec::new(),
            palette: crate::theme::palette_for("Default"),
            fill: false,
            testing: false,
        }
    }

    #[test]
    fn details_updates_remain_open_and_escape_dismisses() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let anchor = RECT {
            left: 100,
            top: 700,
            right: 324,
            bottom: 748,
        };
        toggle(HWND::default(), anchor, 96, snapshot(), 100, 101);
        let hwnd = PANEL.get();
        assert!(!hwnd.is_invalid());
        unsafe {
            assert!(IsWindowVisible(hwnd).as_bool());
            let body = GetDlgItem(hwnd, 10).unwrap();
            assert_ne!(GetWindowTextLengthW(body), 0);
            let mut next = snapshot();
            next.body.push_str("\r\nUpdated session");
            next.testing = true;
            update(next);
            assert!(
                IsWindowVisible(hwnd).as_bool(),
                "edit notifications must not dismiss the panel"
            );
            let button = GetDlgItem(hwnd, 1).unwrap();
            assert!(
                !windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(button).as_bool()
            );
            // Force actual rendering rather than testing only window creation.
            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
            assert!(translate(&MSG {
                hwnd: body,
                message: WM_KEYDOWN,
                wParam: WPARAM(0x1B),
                ..Default::default()
            }));
            assert!(!IsWindowVisible(hwnd).as_bool());
        }
        close();
        assert!(PANEL.get().is_invalid());
    }
}
