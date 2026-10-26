//! Keyboard-accessible native details panel with a larger labeled history graph.
//! No application mutex may be held when calling into this module.

use crate::canvas::{Canvas, Rect};
use crate::graph::TrafficGraph;
use crate::sampler::NetSample;
use crate::theme::Palette;
use crate::win::appearance::system_color;
use crate::win::gdi::{blit_opaque, to_wide, Font};
use crate::win::text::LabelFont;
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
    font: Option<LabelFont>,
    title_font: Option<LabelFont>,
    control_font: Option<Font>,
    scale: f32,
    run_command: u32,
    settings_command: u32,
}

/// Physical-pixel regions, derived from font metrics and the actual client area.
struct PanelLayout {
    heading_y: f32,
    traffic_y: f32,
    graph: Rect,
    scale_y: f32,
    body: Rect,
    buttons_y: f32,
    width: f32,
}

impl PanelState {
    fn layout(&self, width: i32, height: i32) -> PanelLayout {
        let s = self.scale;
        let margin = 16.0 * s;
        let gap = 8.0 * s;
        let heading_height = self
            .title_font
            .as_ref()
            .map_or(20.0 * s, LabelFont::line_height);
        let label_height = self.font.as_ref().map_or(17.0 * s, LabelFont::line_height);
        // Align the heading's baseline rather than inferring ascent from height.
        let heading_baseline = 28.0 * s;
        let heading_y = (heading_baseline
            - self
                .title_font
                .as_ref()
                .map_or(16.0 * s, LabelFont::baseline))
        .round();
        let traffic_y = (heading_y + heading_height + gap).ceil();
        let graph = Rect::new(
            margin,
            (traffic_y + label_height + gap).ceil(),
            (width as f32 - 2.0 * margin).max(1.0),
            66.0 * s,
        );
        let scale_y = (graph.bottom() + gap).ceil();
        let body_y = (scale_y + label_height + 12.0 * s).ceil();
        let buttons_y = height as f32 - margin - 28.0 * s;
        PanelLayout {
            heading_y,
            traffic_y,
            graph,
            scale_y,
            body: Rect::new(
                margin,
                body_y,
                graph.width,
                (buttons_y - margin - body_y).max(1.0),
            ),
            buttons_y,
            width: graph.width,
        }
    }
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
        font: LabelFont::new("Segoe UI", 13.0 * scale, 400),
        title_font: LabelFont::new("Segoe UI", 16.0 * scale, 600),
        control_font: Font::native("Segoe UI", (13.0 * scale).round() as i32, 400),
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
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
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
    let mut client = RECT::default();
    // SAFETY: The live parent owns this writable client rectangle.
    let _ = unsafe { GetClientRect(hwnd, &mut client) };
    let layout = state.layout(client.right, client.bottom);
    let edit = to_wide("EDIT");
    let text = to_wide(&format!(
        "Live: {}\r\n{}",
        state.snapshot.rates, state.snapshot.body
    ));
    // SAFETY: hwnd is the live panel parent; class and text are terminated
    // buffers kept alive for this synchronous call. No creation data is passed.
    state.body = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(edit.as_ptr()),
            PCWSTR(text.as_ptr()),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_VSCROLL
                | WINDOW_STYLE(ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32),
            layout.body.x.round() as i32,
            layout.body.y.round() as i32,
            layout.body.width.round() as i32,
            layout.body.height.round() as i32,
            hwnd,
            HMENU(netspeed_widget::commands::DETAILS_BODY_ID as usize as *mut _),
            None,
            None,
        )
    }
    .unwrap_or_default();
    if let Some(font) = &state.control_font {
        font.apply_to(state.body);
    }
    for (index, label) in ["&Run speedtest", "&Settings", "&Close"].iter().enumerate() {
        let class = to_wide("BUTTON");
        let label = to_wide(label);
        // SAFETY: hwnd is the live panel parent; both terminated strings live
        // through creation, and the integer child ID is not an owned menu.
        if let Ok(button) = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR(label.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                (16.0 * s + index as f32 * (layout.width + 8.0 * s) / 3.0).round() as i32,
                layout.buttons_y.round() as i32,
                ((layout.width - 16.0 * s) / 3.0).round() as i32,
                (28.0 * s) as i32,
                hwnd,
                HMENU((index + 1) as *mut _),
                None,
                None,
            )
        } {
            if let Some(font) = &state.control_font {
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
    // SAFETY: Windows invokes this procedure for its panel window. User data is
    // either null during creation or the Box<PanelState> installed by show().
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PanelState;
    if pointer.is_null() {
        // SAFETY: Forward the unchanged Windows-supplied message tuple.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    match message {
        WM_PAINT => {
            // SAFETY: The UI thread owns the installed Box until WM_NCDESTROY;
            // this painting path does not re-enter the panel procedure.
            let state = unsafe { &mut *pointer };
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: WM_PAINT supplies hwnd; ps is writable and paired below.
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rect = RECT::default();
            // SAFETY: rect is a writable output buffer for the live panel.
            let _ = unsafe { GetClientRect(hwnd, &mut rect) };
            let mut canvas = Canvas::new(rect.right.max(1), rect.bottom.max(1));
            // SAFETY: Both constants are valid system color indices.
            let (background, foreground) = unsafe {
                (
                    system_color(GetSysColor(COLOR_WINDOW)),
                    system_color(GetSysColor(COLOR_WINDOWTEXT)),
                )
            };
            let palette = crate::theme::native_panel_palette(
                state.snapshot.palette,
                background,
                foreground,
                crate::win::appearance::Appearance::current().high_contrast,
            );
            canvas.fill_rounded_rect(
                Rect::new(0.0, 0.0, rect.right as f32, rect.bottom as f32),
                0.0,
                background,
            );
            let s = state.scale;
            let layout = state.layout(rect.right, rect.bottom);
            if let Some(font) = &state.title_font {
                let rates = font.fit(&state.snapshot.rates, layout.width);
                canvas.draw_text(16.0 * s, layout.heading_y, &rates, font, palette.fg);
            }
            state.graph.draw(
                &mut canvas,
                layout.graph,
                &state.snapshot.samples,
                &palette,
                state.snapshot.fill,
                s,
            );
            if let Some(font) = &state.font {
                canvas.draw_text(
                    16.0 * s,
                    layout.traffic_y,
                    "Traffic · last 60 seconds",
                    font,
                    palette.fg,
                );
                canvas.draw_text(
                    16.0 * s,
                    layout.scale_y,
                    &format!("Shared auto scale: {:.1} Mbps", state.graph.scale()),
                    font,
                    palette.fg,
                );
            }
            blit_opaque(dc, &canvas);
            // SAFETY: ps is the PAINTSTRUCT initialized by BeginPaint above.
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            if !(1..=3).contains(&id) || wparam.0 >> 16 != BN_CLICKED as usize {
                return LRESULT(0);
            }
            // SAFETY: The installed Box is live on the UI thread; the borrow
            // ends before posting a command or changing window visibility.
            let state = unsafe { &*pointer };
            let command = match id {
                1 => Some(state.run_command),
                2 => Some(state.settings_command),
                _ => None,
            };
            let owner = state.owner;
            if let Some(command) = command {
                // SAFETY: The command contains only integer values, not borrowed
                // pointers that could expire before the owner processes it.
                if let Err(err) =
                    unsafe { PostMessageW(owner, WM_COMMAND, WPARAM(command as usize), LPARAM(0)) }
                {
                    crate::logger::warning(
                        "details",
                        &format!("could not post panel command: {err}"),
                    );
                }
            }
            if id != 1 {
                // SAFETY: hwnd is this panel's window on its owning UI thread.
                // The return value is previous visibility, not a failure status.
                let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: hwnd is this panel's window on its owning UI thread.
            // The return value is previous visibility, not a failure status.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            LRESULT(0)
        }
        // Reopening rebuilds controls at the new monitor's DPI.
        WM_DPICHANGED => {
            // SAFETY: hwnd is this panel's window on its owning UI thread.
            // The return value is previous visibility, not a failure status.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            LRESULT(0)
        }
        WM_ACTIVATE if wparam.0 & 0xFFFF == WA_INACTIVE as usize => {
            DEACTIVATED.set(Some(Instant::now()));
            // SAFETY: hwnd is this panel's window on its owning UI thread.
            // The return value is previous visibility, not a failure status.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // SAFETY: Clear the pointer before releasing its Box so any
            // subsequent messages take the null-user-data path.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            PANEL.set(HWND::default());
            // SAFETY: show() transferred this Box once; WM_NCDESTROY reclaims
            // it exactly once after clearing all window access to the pointer.
            unsafe { drop(Box::from_raw(pointer)) };
            // SAFETY: Forward the unchanged Windows-supplied message tuple.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        // SAFETY: Forward the unchanged Windows-supplied message tuple.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::TextRenderer;

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
    fn details_layout_and_dpi_lifecycle_keep_controls_inside_client() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            DEACTIVATED.set(None);
            toggle(
                HWND::default(),
                RECT {
                    left: 100,
                    top: 1050,
                    right: 900,
                    bottom: 1080,
                },
                dpi,
                snapshot(),
                100,
                101,
            );
            let hwnd = PANEL.get();
            assert!(!hwnd.is_invalid());
            unsafe {
                let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const PanelState);
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client).unwrap();
                let layout = state.layout(client.right, client.bottom);
                let title = state.title_font.as_ref().unwrap();
                let label = state.font.as_ref().unwrap();
                assert!(layout.heading_y >= 0.0);
                assert!(layout.heading_y + title.line_height() < layout.traffic_y);
                assert!(layout.traffic_y + label.line_height() < layout.graph.y);
                assert!(layout.graph.bottom() < layout.scale_y);
                assert!(layout.scale_y + label.line_height() < layout.body.y);
                assert!(layout.body.bottom() < layout.buttons_y);
                assert!(layout.body.height >= 180.0 * state.scale);
                assert!(layout.buttons_y + 28.0 * state.scale < client.bottom as f32);
                let long = title.fit("↓ 123456789.99 GB/s    ↑ 123456789.99 GB/s", layout.width);
                assert!(long.ends_with('…'));
                assert!(title.measure(&long) <= layout.width);
                let mut origin = windows::Win32::Foundation::POINT::default();
                assert!(windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin).as_bool());
                for id in [1, 2, 3, 10] {
                    let control = GetDlgItem(hwnd, id).unwrap();
                    assert!(IsWindowVisible(control).as_bool());
                    let mut bounds = RECT::default();
                    GetWindowRect(control, &mut bounds).unwrap();
                    assert!(bounds.left >= origin.x && bounds.top >= origin.y);
                    assert!(bounds.right <= origin.x + client.right);
                    assert!(bounds.bottom <= origin.y + client.bottom);
                    if id == 1 {
                        assert!(
                            state
                                .control_font
                                .as_ref()
                                .unwrap()
                                .measure("Run speedtest")
                                + 4.0 * state.scale
                                <= (bounds.right - bounds.left) as f32
                        );
                    }
                }
                // The existing transition policy hides stale-DPI content. Reopen
                // recreates all font/cache resources instead of stretching masks.
                // Exercise handler policy directly, without relying on OS
                // dispatch of synthetic monitor changes on an unaware test thread.
                panel_proc(hwnd, WM_DPICHANGED, WPARAM(192), LPARAM(0));
                assert!(!IsWindowVisible(hwnd).as_bool());
            }
            close();
        }
        DEACTIVATED.set(None);
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetGuiResources(process: windows::Win32::Foundation::HANDLE, flags: u32) -> u32;
    }

    #[test]
    fn repeated_popups_release_native_and_text_resources() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let process = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
        let mut baseline = (0, 0);
        for i in 0..41 {
            DEACTIVATED.set(None);
            toggle(
                HWND::default(),
                RECT {
                    left: 100,
                    top: 1050,
                    right: 900,
                    bottom: 1080,
                },
                [96, 120, 144, 192][i % 4],
                snapshot(),
                100,
                101,
            );
            let hwnd = PANEL.get();
            assert!(!hwnd.is_invalid());
            let weak = unsafe {
                let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const PanelState);
                let mask = state
                    .title_font
                    .as_ref()
                    .unwrap()
                    .mask(&state.snapshot.rates);
                std::rc::Rc::downgrade(&mask.pixels)
            };
            close();
            assert!(weak.upgrade().is_none());
            let counts = unsafe { (GetGuiResources(process, 0), GetGuiResources(process, 1)) };
            if i == 0 {
                baseline = counts;
            }
            if i == 40 {
                eprintln!("Popup lifecycle: GDI/USER {:?} -> {:?}", baseline, counts);
                assert!(counts.0 <= baseline.0 + 4 && counts.1 <= baseline.1 + 4);
            }
        }
        DEACTIVATED.set(None);
    }

    #[test]
    #[ignore = "captures real desktop popups and measures presentation"]
    fn details_rendering_benchmark_and_screenshots() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let stage = std::env::var("NETSPEED_CAPTURE_STAGE").unwrap_or_else(|_| "after".into());
        let output = std::env::current_dir()
            .unwrap()
            .join("target/rendering-comparison");
        std::fs::create_dir_all(&output).unwrap();
        unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(
                windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            );
        }
        for dpi in [96, 120, 144, 192] {
            for (case, rates) in [
                ("short", "↓ 1.28 MB/s    ↑ 112 kB/s"),
                ("long", "↓ 999.99 Gbps    ↑ 999.99 Gbps"),
            ] {
                let mut data = snapshot();
                data.rates = rates.into();
                data.body = "Adapter: Intel(R) Ethernet Connection with a very long interface name\r\nTCP latency: 8 ms\r\nSession: 1.28 GB\r\nDownload: 999.99 Gbps\r\nUpload: 999.99 Gbps".into();
                data.samples = (0..60)
                    .map(|i| NetSample {
                        down_mbps: ((i * 17) % 75) as f64,
                        up_mbps: ((i * 7) % 25) as f64,
                        ts: crate::now_epoch() - 59.0 + i as f64,
                    })
                    .collect();
                let start = Instant::now();
                toggle(
                    HWND::default(),
                    RECT {
                        left: 100,
                        top: 1050,
                        right: 900,
                        bottom: 1080,
                    },
                    dpi,
                    data,
                    100,
                    101,
                );
                let hwnd = PANEL.get();
                assert!(!hwnd.is_invalid());
                eprintln!("requested layout DPI {dpi}; actual window DPI {}", unsafe {
                    windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd)
                });
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
                }
                let cold = start.elapsed();
                let start = Instant::now();
                for _ in 0..100 {
                    unsafe {
                        let _ = InvalidateRect(hwnd, None, false);
                        let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
                    }
                }
                eprintln!(
                    "{stage} {dpi} DPI {case}: open+first paint {:?}; warm paint+presentation {:?}",
                    cold,
                    start.elapsed() / 100
                );
                let mut rect = RECT::default();
                unsafe {
                    GetWindowRect(hwnd, &mut rect).unwrap();
                }
                let path = output.join(format!("{stage}-{dpi}-{case}.png"));
                let script = format!("Add-Type -AssemblyName System.Drawing; $b = New-Object System.Drawing.Bitmap({}, {}); $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen({}, {}, 0, 0, $b.Size); $b.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $b.Dispose()", rect.right-rect.left, rect.bottom-rect.top, rect.left, rect.top, path.display());
                assert!(std::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &script])
                    .status()
                    .unwrap()
                    .success());
                close();
                DEACTIVATED.set(None);
            }
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
            assert_ne!(
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0,
                0,
                "network details must stack above the taskbar"
            );
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
