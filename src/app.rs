//! The application: the taskbar widget, the tray icon and the services
//! behind them.
//!
//! The controller owns timers, the global hotkey and the tray icon. The widget
//! is a non-activating child of Explorer's taskbar. All application state and window calls stay on the UI
//! thread; workers publish network results through messages or snapshots.

use crate::clock::now_epoch;
use netspeed_widget::commands::*;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, RegisterHotKey, TrackMouseEvent, UnregisterHotKey, HOT_KEY_MODIFIERS, TME_LEAVE,
    TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, LoadCursorW, LoadIconW, PostMessageW,
    PostQuitMessage, RegisterClassExW, SetTimer, SetWindowPos, ShowWindow, CS_DBLCLKS, HWND_TOP,
    IDC_ARROW, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE, MSG, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CAPTURECHANGED,
    WM_CONTEXTMENU, WM_DESTROY, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::canvas::Canvas;
use crate::config::{self, DisplayLayout, Placement, SpeedUnits};
use crate::format::{
    age_label, hotkey_label, speedtest_label, transfer_label, RateFormatter, RateText,
};
use crate::graph::TrafficGraph;
use crate::hotkey;
use crate::latency::{LatencyProbe, LatencyResult};
use crate::logger;
use crate::paint::{self, Fonts, WidgetView};
use crate::sampler::{NetSample, NetSampler};
use crate::schedule::next_speedtest_due;
use crate::speedtest::{self, SpeedtestResult};
use crate::theme::{self, Health, Palette};
use crate::win::dialog;
use crate::win::gdi::{blit_layered, to_wide};
use crate::win::menu::{self, MenuEntry};
use crate::win::placement::{
    choose_x, DensitySettler, OccupancyProbe, PlacementSettler, LAYOUT_REFRESH_MS,
    WM_TASKBAR_LAYOUT_CHANGED, WM_TASKBAR_LAYOUT_READY,
};
use crate::win::startup;
use crate::win::taskbar::{self, TaskbarWindow};
use crate::win::tray::{TrayIcon, TRAY_CALLBACK};
use crate::win::{appearance::Appearance, details, tooltip::Tooltip};

/// Custom message carrying a background result to the UI thread.
const WM_APP_SPEEDTEST: u32 = WM_APP + 1;

/// Timer that refreshes the widget.
const TIMER_TICK: usize = 1;
/// Slow timer for taskbar recovery and scheduled speedtests.
const TIMER_BACKGROUND: usize = 2;
/// One-shot retry for a rate-limited event-driven layout scan.
const TIMER_LAYOUT_RETRY: usize = 3;

/// How often the widget refreshes, in milliseconds.
const UI_TICK_MS: u32 = 250;
/// Interval for taskbar recovery and speedtest scheduling.
const BACKGROUND_TICK_MS: u32 = 2000;
/// How many samples the traffic graph keeps.
const GRAPH_SAMPLES: usize = 60;
/// Seconds between automatic speedtests.
const SPEEDTEST_INTERVAL_SEC: f64 = 4.0 * 60.0 * 60.0;
/// Delay before the first automatic speedtest after a fresh start.
const SPEEDTEST_STARTUP_GRACE_SEC: f64 = 20.0;
/// Delay before a failed speedtest is retried.
const SPEEDTEST_RETRY_SEC: f64 = 15.0 * 60.0;
/// Latency below this counts as healthy.
const LATENCY_GOOD_MS: f64 = 80.0;
/// Identifier of the global hotkey registration.
const HOTKEY_ID: i32 = 1;

/// The whole application state.
struct AppState {
    /// Hidden top-level controller, also used as the popup menu owner.
    controller: HWND,
    /// The widget window embedded in the taskbar.
    widget: HWND,
    /// The taskbar the widget lives in.
    taskbar: Option<TaskbarWindow>,
    /// Whether the widget is attached to a taskbar.
    embedded: bool,
    /// The theme preset in use.
    theme_name: String,
    palette: Palette,
    appearance: Appearance,
    dpi: u32,
    layout: DisplayLayout,
    effective_layout: DisplayLayout,
    units: SpeedUnits,
    auto_fit: bool,
    subtle_background: bool,
    graph_fill: bool,
    density_settler: DensitySettler,
    down_formatter: RateFormatter,
    up_formatter: RateFormatter,
    down_text: RateText,
    up_text: RateText,
    tooltip: Option<Tooltip>,
    sampler: NetSampler,
    probe: LatencyProbe,
    health: Health,
    smooth_down: f64,
    smooth_up: f64,
    max_down_seen: f64,
    max_up_seen: f64,
    speedtest_running: bool,
    speedtest_next_due: f64,
    speedtest_summary: String,
    graph: TrafficGraph,
    fonts: Option<Fonts>,
    width: i32,
    height: i32,
    /// Whether the cursor is over the widget.
    hovering: bool,
    /// Whether our popup menu is already open.
    menu_open: bool,
    menu_devices: Vec<Option<String>>,
    click_pending: bool,
    placement: Placement,
    widget_visible: bool,
    placement_available: bool,
    widget_shown: bool,
    occupancy: OccupancyProbe,
    placement_settler: PlacementSettler,
    show_graph: bool,
    hotkey_combo: String,
    hotkey_ok: bool,
    hotkey_id: i32,
    last_sample_ts: f64,
    tick_count: u64,
    tray: Option<TrayIcon>,
    icon: AppIcon,
    /// The widget's last successfully placed screen rectangle.
    placed_for: Option<RECT>,
    /// Native effects are prepared under STATE and executed after releasing it.
    pending_position: Option<POINT>,
    pending_visibility: Option<bool>,
    pending_canvas: Option<Canvas>,
    pending_tray_tip: Option<String>,
    blit_failed: bool,
    attach_failure: Option<String>,
}

// Only the UI thread accesses this slot. Workers use their own snapshots or
// post a completion message; they never use these handles or GDI resources.
unsafe impl Send for AppState {}

/// The global application state.
static STATE: OnceLock<Mutex<Option<AppState>>> = OnceLock::new();

/// The state slot.
fn state_slot() -> &'static Mutex<Option<AppState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

/// Runs the application.
pub fn run() -> Result<(), String> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(|err| err.to_string())?;
    let hinstance: HINSTANCE = hinstance.into();
    let controller_class = to_wide("NetSpeedWidgetController");
    let widget_class = to_wide("NetSpeedWidgetTaskbar");
    unsafe {
        register_class(hinstance, &controller_class, controller_proc);
        register_class(hinstance, &widget_class, widget_proc);
    }

    let controller = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR(controller_class.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            HWND::default(),
            windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
            hinstance,
            None,
        )
    }
    .map_err(|err| format!("could not create the controller window: {err}"))?;

    let icon = load_icon();
    let theme_name = config::theme();
    let config::DisplayPreferences {
        layout,
        units,
        auto_fit,
        subtle_background,
        graph_fill,
    } = config::display_preferences();
    let appearance = Appearance::current();
    let palette = theme::resolved_palette(&theme_name, appearance, subtle_background);
    let show_graph = config::show_graph();
    let widget_visible = config::widget_visible();
    let placement = config::placement();
    let hotkey_combo = config::hotkey();
    let sampler = NetSampler::new(Duration::from_secs(1), GRAPH_SAMPLES);
    let probe = LatencyProbe::new(Duration::from_secs(2), Duration::from_millis(1500));

    let height = 48;
    let width = paint::widget_width(layout, show_graph, 96);
    let next_due = next_speedtest_due(
        config::speedtest().as_ref(),
        now_epoch(),
        SPEEDTEST_INTERVAL_SEC,
        SPEEDTEST_STARTUP_GRACE_SEC,
    );

    let saved_test = config::speedtest();
    let state = AppState {
        controller,
        widget: HWND::default(),
        taskbar: None,
        embedded: false,
        theme_name,
        palette,
        appearance,
        dpi: 96,
        layout,
        effective_layout: layout,
        units,
        auto_fit,
        subtle_background,
        graph_fill,
        density_settler: DensitySettler::default(),
        down_formatter: RateFormatter::default(),
        up_formatter: RateFormatter::default(),
        down_text: RateFormatter::default().format(None, units),
        up_text: RateFormatter::default().format(None, units),
        tooltip: None,
        sampler,
        probe,
        health: Health::Good,
        smooth_down: 0.0,
        smooth_up: 0.0,
        max_down_seen: 0.0,
        max_up_seen: 0.0,
        speedtest_running: false,
        speedtest_next_due: next_due,
        speedtest_summary: saved_test
            .map(|test| {
                format!(
                    "Last speedtest: {:.1} D | {:.1} U Mb/s",
                    test.down_mbps, test.up_mbps
                )
            })
            .unwrap_or_default(),
        graph: TrafficGraph::new(),
        fonts: Fonts::for_dpi(96, layout == DisplayLayout::SingleLine, height),
        width,
        height,
        hovering: false,
        menu_open: false,
        menu_devices: Vec::new(),
        click_pending: false,
        placement,
        widget_visible,
        placement_available: false,
        widget_shown: false,
        occupancy: OccupancyProbe::new(controller),
        placement_settler: PlacementSettler::default(),
        show_graph,
        hotkey_combo,
        hotkey_ok: false,
        hotkey_id: HOTKEY_ID,
        last_sample_ts: 0.0,
        tick_count: 0,
        tray: None,
        icon,
        placed_for: None,
        pending_position: None,
        pending_visibility: None,
        pending_canvas: None,
        pending_tray_tip: None,
        blit_failed: false,
        attach_failure: None,
    };
    *state_slot().lock().unwrap_or_else(|err| err.into_inner()) = Some(state);

    create_widget(hinstance, &widget_class);
    attach_to_taskbar();
    add_tray_icon();
    flush_native_effects();

    {
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let state = guard.as_mut().expect("the state was just installed");
        state.sampler.start();
        state.probe.start();
        apply_hotkey(state);
    }

    unsafe {
        SetTimer(controller, TIMER_TICK, UI_TICK_MS, None);
        SetTimer(controller, TIMER_BACKGROUND, BACKGROUND_TICK_MS, None);
    }

    logger::startup(&format!("NetSpeed Widget v{}", env!("CARGO_PKG_VERSION")));

    unsafe {
        let mut message = MSG::default();
        loop {
            let result = windows::Win32::UI::WindowsAndMessaging::GetMessageW(
                &mut message,
                HWND::default(),
                0,
                0,
            );
            if result.0 <= 0 {
                break;
            }
            if details::translate(&message) {
                continue;
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&message);
            windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&message);
        }
    }

    shutdown();
    Ok(())
}

/// Registers a window class.
unsafe fn register_class(
    hinstance: HINSTANCE,
    name: &[u16],
    proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_DBLCLKS,
        lpfnWndProc: Some(proc),
        hInstance: hinstance,
        // SAFETY: IDC_ARROW is a predefined system cursor and needs no instance.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: PCWSTR(name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: The caller provides a live instance, a terminated class name and
    // a system-ABI window procedure; RegisterClassExW copies the class data.
    let _ = unsafe { RegisterClassExW(&wc) };
}

/// Only file-loaded icons are owned; LoadIconW returns shared resources.
struct AppIcon {
    handle: windows::Win32::UI::WindowsAndMessaging::HICON,
    owned: bool,
}

impl Drop for AppIcon {
    fn drop(&mut self) {
        if self.owned {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(self.handle);
            }
        }
    }
}

/// Loads the application icon.
fn load_icon() -> AppIcon {
    // winres embeds the icon as resource 1, including in standalone builds.
    if let Ok(module) = unsafe { GetModuleHandleW(PCWSTR::null()) } {
        if let Ok(icon) =
            unsafe { LoadIconW(HINSTANCE(module.0), PCWSTR(std::ptr::without_provenance(1))) }
        {
            return AppIcon {
                handle: icon,
                owned: false,
            };
        }
    }
    let path = crate::paths::icon_path();
    let wide = to_wide(&path.to_string_lossy());
    unsafe {
        if path.is_file() {
            if let Ok(handle) = windows::Win32::UI::WindowsAndMessaging::LoadImageW(
                None,
                PCWSTR(wide.as_ptr()),
                IMAGE_ICON,
                0,
                0,
                LR_LOADFROMFILE | LR_DEFAULTSIZE,
            ) {
                return AppIcon {
                    handle: windows::Win32::UI::WindowsAndMessaging::HICON(handle.0),
                    owned: true,
                };
            }
        }
        AppIcon {
            handle: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            owned: false,
        }
    }
}

/// Creates the widget window, if it does not exist yet.
fn create_widget(hinstance: HINSTANCE, class: &[u16]) {
    let (width, height) = {
        let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_ref() else {
            return;
        };
        if !state.widget.is_invalid() {
            return;
        }
        (state.width, state.height)
    };
    let title = to_wide("NetSpeed Widget");
    let widget = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_POPUP.0),
            0,
            0,
            width,
            height,
            HWND::default(),
            windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
            hinstance,
            None,
        )
    };
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };
    match widget {
        Ok(widget) => state.widget = widget,
        Err(err) => {
            logger::error("app", &format!("could not create the widget window: {err}"));
        }
    }
    let widget = state.widget;
    let (width, height) = (state.width, state.height);
    drop(guard);
    if !widget.is_invalid() {
        let tooltip = Tooltip::new(widget, width, height);
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        if let Some(state) = guard.as_mut() {
            state.tooltip = tooltip;
        }
    }
}

/// Re-evaluate the saved monitor preference even while a fallback remains valid.
fn preferred_taskbar() -> Option<TaskbarWindow> {
    let taskbars = taskbar::find_taskbars();
    let wanted = config::taskbar_monitor();
    let mut chosen: Option<TaskbarWindow> = None;
    if let Some(device) = wanted.as_deref() {
        chosen = taskbars
            .iter()
            .find(|taskbar| taskbar::monitor_device(taskbar).as_deref() == Some(device))
            .copied();
    }
    if chosen.is_none() {
        chosen = taskbars
            .iter()
            .find(|taskbar| taskbar.is_primary)
            .copied()
            .or_else(|| taskbars.first().copied());
    }
    chosen
}

/// Attaches the widget to the best matching taskbar.
fn attach_to_taskbar() {
    let Some(chosen) = preferred_taskbar() else {
        log_attach_failure("no taskbar found yet".to_string());
        return;
    };

    let widget = {
        let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_ref() else {
            return;
        };
        state.widget
    };
    if widget.is_invalid() {
        return;
    }
    // Parenting can synchronously dispatch window messages. Do not hold STATE.
    if let Err(err) = taskbar::embed_widget(widget, chosen.hwnd) {
        log_attach_failure(format!("could not embed widget: {err}"));
        return;
    }
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };
    state.taskbar = Some(chosen);
    if state.attach_failure.take().is_some() {
        logger::info("app", "taskbar attachment recovered");
    }
    state.embedded = true;
    state.placement_available = false;
    state.placed_for = None;
    logger::info(
        "app",
        &format!("embedded in the taskbar at {:?}", chosen.rect),
    );
    position_widget(state);
}

fn log_attach_failure(message: String) {
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(state) = guard.as_mut() {
        if state.attach_failure.is_none() {
            logger::warning("app", &message);
        }
        state.attach_failure = Some(message);
    }
}

/// Places the child widget using taskbar client coordinates.
fn position_widget(state: &mut AppState) {
    if state.menu_open {
        return;
    }
    if !state.widget_visible {
        sync_widget_visibility(state);
        return;
    }
    let Some(taskbar) = state.taskbar else {
        state.placement_available = false;
        sync_widget_visibility(state);
        return;
    };
    let Some(rect) = taskbar::taskbar_rect(taskbar.hwnd) else {
        state.placement_available = false;
        sync_widget_visibility(state);
        return;
    };
    let taskbar_height = rect.bottom - rect.top;
    let height = taskbar_height.max(1);
    let dpi = unsafe { GetDpiForWindow(taskbar.hwnd) }.max(96);
    let desired = paint::effective_layout(state.layout, height, dpi);
    let occupied = state
        .occupancy
        .occupied(taskbar.hwnd, state.widget, rect)
        .map(|areas| areas.to_vec());
    let gap = (8.0 * dpi as f32 / 96.0).round() as i32;
    let tray_left = taskbar::tray_left(taskbar.hwnd, rect);
    let can_fit = |layout, margin| {
        let width = paint::widget_width(layout, state.show_graph, dpi) + margin;
        occupied
            .as_ref()
            .and_then(|areas| {
                choose_x(
                    rect,
                    areas,
                    width,
                    tray_left - width - gap,
                    None,
                    gap,
                    state.placement,
                )
            })
            .is_some()
    };
    let mut candidate = desired;
    if state.auto_fit && desired == DisplayLayout::Balanced && occupied.is_some() {
        let expansion_margin = if state.effective_layout == DisplayLayout::Compact {
            gap * 2
        } else {
            0
        };
        if !can_fit(desired, expansion_margin) {
            candidate = DisplayLayout::Compact;
        }
    } else if state.auto_fit
        && desired == DisplayLayout::Balanced
        && state.effective_layout == DisplayLayout::Compact
        && occupied.is_none()
    {
        // Unknown occupancy is not evidence that there is room to expand.
        candidate = DisplayLayout::Compact;
    }
    let geometry_changed = dpi != state.dpi || height != state.height;
    state.effective_layout = state.density_settler.choose(
        state.effective_layout,
        candidate,
        geometry_changed || state.placed_for.is_none(),
        Instant::now(),
    );
    let width = paint::widget_width(state.effective_layout, state.show_graph, dpi);
    if state.width != width || geometry_changed {
        state.width = width;
        state.height = height;
        state.dpi = dpi;
        state.fonts = Fonts::for_dpi(
            dpi,
            state.effective_layout == DisplayLayout::SingleLine,
            height,
        );
        state.placed_for = None;
    }
    let preferred = match state.placement {
        Placement::Left => rect.left,
        Placement::Tray => (tray_left - width - gap).max(rect.left),
    };
    let x = occupied.as_ref().and_then(|occupied| {
        choose_x(
            rect,
            occupied,
            width,
            preferred,
            state.placed_for.map(|r| r.left),
            gap,
            state.placement,
        )
    });
    let target = x.map(|x| RECT {
        left: x,
        top: rect.top,
        right: x + width,
        bottom: rect.bottom,
    });
    // Explicit placement changes clear placed_for and take effect immediately.
    let same_geometry = state.placed_for.is_some_and(|r| {
        r.top == rect.top && r.bottom == rect.bottom && r.left >= rect.left && r.right <= rect.right
    });
    if same_geometry {
        if !state
            .placement_settler
            .ready(state.placed_for, target, Instant::now())
        {
            return;
        }
    } else {
        state.placement_settler.reset();
    }
    let Some(target) = target else {
        state.placement_available = false;
        sync_widget_visibility(state);
        return;
    };
    let x = target.left;
    if state.placed_for == Some(target) {
        state.placement_available = true;
        sync_widget_visibility(state);
        return;
    }
    let y = rect.top;
    let Some(origin) = taskbar::client_point(taskbar.hwnd, POINT { x, y }) else {
        state.placed_for = None;
        state.placement_available = false;
        sync_widget_visibility(state);
        return;
    };

    state.pending_position = Some(origin);
    state.placed_for = Some(target);
    state.placement_available = true;
    sync_widget_visibility(state);
}

/// Automatic tray-only fallback must not overwrite the user's visibility setting.
fn sync_widget_visibility(state: &mut AppState) {
    let show = state.widget_visible && state.embedded && state.placement_available;
    if state.widget_shown != show {
        state.widget_shown = show;
        state.pending_visibility = Some(show);
    }
}

/// Adds the tray icon.
fn add_tray_icon() {
    let (controller, icon) = {
        let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_ref() else {
            return;
        };
        if state.tray.is_some() {
            return;
        }
        (state.controller, state.icon.handle)
    };
    let tray = TrayIcon::add(controller, 1, icon, "NetSpeed Widget");
    if !tray.is_visible() {
        logger::warning("tray", "the notification area refused the icon");
    }
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(state) = guard.as_mut() {
        state.tray = Some(tray);
        update_tray_tooltip(state);
    } else {
        drop(guard);
        drop(tray);
    }
}

/// Refreshes the tray tooltip with the live readout.
fn update_tray_tooltip(state: &mut AppState) {
    let ping = ping_text(state);
    let tooltip = format!(
        "NetSpeed Widget v{}\n{}\nD {} {} | U {} {} | {}",
        env!("CARGO_PKG_VERSION"),
        speedtest_label(&state.speedtest_summary),
        state.down_text.value,
        state.down_text.unit,
        state.up_text.value,
        state.up_text.unit,
        ping
    );
    state.pending_tray_tip = Some(tooltip);
}

/// The latency readout for the current probe result.
fn ping_text(state: &AppState) -> String {
    crate::latency::ping_label(state.probe.latest(), now_epoch(), state.speedtest_running)
}

/// Registers the global hotkey.
fn apply_hotkey(state: &mut AppState) {
    if state.hotkey_ok {
        unsafe {
            let _ = UnregisterHotKey(state.controller, state.hotkey_id);
        }
        state.hotkey_ok = false;
    }
    match hotkey::parse_hotkey(&state.hotkey_combo) {
        Ok((mods, vk)) => {
            let flags = HOT_KEY_MODIFIERS(mods | hotkey::MOD_NOREPEAT);
            let result = unsafe { RegisterHotKey(state.controller, state.hotkey_id, flags, vk) };
            if result.is_ok() {
                state.hotkey_ok = true;
                logger::info(
                    "hotkey",
                    &format!("global hotkey set to {}", state.hotkey_combo),
                );
            } else if let Err(err) = result {
                logger::warning(
                    "hotkey",
                    &format!(
                        "could not register '{}': {err} ({:?})",
                        state.hotkey_combo,
                        err.code()
                    ),
                );
            }
        }
        Err(err) => {
            logger::warning("hotkey", &format!("invalid hotkey: {err}"));
        }
    }
}

/// Shows or hides the widget.
fn toggle_visibility() {
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };
    let visible = state.widget_visible;
    state.widget_visible = !visible;
    config::set_widget_visible(!visible);
    position_widget(state);
    sync_widget_visibility(state);
    logger::info("app", &format!("widget visible: {}", !visible));
}

/// Starts a speedtest on a worker thread.
fn start_speedtest(manual: bool) {
    let controller = {
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_mut() else {
            return;
        };
        if state.speedtest_running {
            return;
        }
        state.speedtest_running = true;
        state.speedtest_summary = "Speedtest is running...".to_string();
        state.controller
    };
    logger::info(
        "speedtest",
        if manual {
            "speedtest run (manual)"
        } else {
            "speedtest run (scheduled)"
        },
    );

    let handle = controller.0 as isize;
    let spawned = std::thread::Builder::new()
        .name("speedtest-worker".to_string())
        .spawn(move || {
            let outcome = speedtest::measure_speed();
            let result = match outcome {
                Ok(result) => Some(result),
                Err(err) => {
                    logger::error("speedtest", &format!("speedtest run failed: {err}"));
                    None
                }
            };
            *pending_result()
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = result;
            unsafe {
                let _ = PostMessageW(
                    HWND(handle as *mut c_void),
                    WM_APP_SPEEDTEST,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        });
    if let Err(err) = spawned {
        logger::error("speedtest", &format!("could not start worker: {err}"));
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        if let Some(state) = guard.as_mut() {
            state.speedtest_running = false;
            state.speedtest_summary = "Speedtest failed".to_string();
            state.speedtest_next_due = now_epoch() + SPEEDTEST_RETRY_SEC;
        }
    }
}

/// The result of the last finished speedtest.
fn pending_result() -> &'static Mutex<Option<SpeedtestResult>> {
    static RESULT: OnceLock<Mutex<Option<SpeedtestResult>>> = OnceLock::new();
    RESULT.get_or_init(|| Mutex::new(None))
}

/// Applies the result of a finished speedtest.
fn finish_speedtest(result: Option<SpeedtestResult>) {
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };
    state.speedtest_running = false;
    match result {
        Some(result) => {
            let measured = result.measured();
            if measured {
                let saved = config::set_speedtest(result.down_mbps, result.up_mbps);
                config::set_speedtest_source(result.source);
                state.speedtest_summary = format!(
                    "Last speedtest: {:.1} D | {:.1} U Mb/s",
                    saved.down_mbps, saved.up_mbps
                );
            } else {
                state.speedtest_summary = format!(
                    "Speedtest estimate: {:.1} D | {:.1} U Mb/s",
                    result.down_mbps, result.up_mbps
                );
            }
            logger::info(
                "speedtest",
                &format!(
                    "result: {:.1} D | {:.1} U Mb/s, source={}",
                    result.down_mbps, result.up_mbps, result.source
                ),
            );
            state.speedtest_next_due = now_epoch()
                + if measured {
                    SPEEDTEST_INTERVAL_SEC
                } else {
                    SPEEDTEST_RETRY_SEC
                };
        }
        None => {
            state.speedtest_summary = "Speedtest failed".to_string();
            state.speedtest_next_due = now_epoch() + SPEEDTEST_RETRY_SEC;
        }
    }
    update_tray_tooltip(state);
}

/// The controller window procedure.
unsafe extern "system" fn controller_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows supplies the controller's live window and message payload.
    let result = unsafe { controller_message(hwnd, msg, wparam, lparam) };
    flush_native_effects();
    result
}

unsafe fn controller_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TASKBAR_LAYOUT_CHANGED | WM_TASKBAR_LAYOUT_READY => {
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                if msg == WM_TASKBAR_LAYOUT_CHANGED {
                    state.occupancy.invalidate();
                }
                position_widget(state);
                render(state);
                // SAFETY: The timer belongs to this controller and only posts
                // WM_TIMER. The regular UI timer remains a fallback on failure.
                if unsafe { SetTimer(hwnd, TIMER_LAYOUT_RETRY, LAYOUT_REFRESH_MS, None) } == 0 {
                    logger::warning(
                        "placement",
                        "could not schedule layout retry; using UI timer",
                    );
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            tick(wparam.0);
            LRESULT(0)
        }
        WM_HOTKEY => {
            toggle_visibility();
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_COMMAND if lparam.0 == 0 => {
            handle_command((wparam.0 & 0xffff) as u32);
            LRESULT(0)
        }
        WM_APP_SPEEDTEST => {
            let result = pending_result()
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .take();
            finish_speedtest(result);
            LRESULT(0)
        }
        TRAY_CALLBACK => {
            handle_tray_click(lparam.0 as u32);
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: This runs on the UI thread that owns the message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// The widget window procedure.
unsafe extern "system" fn widget_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows supplies the widget's live window and message payload.
    let result = unsafe { widget_message(hwnd, msg, wparam, lparam) };
    flush_native_effects();
    result
}

unsafe fn widget_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
            // SAFETY: This handles WM_PAINT for hwnd and pairs the paint calls
            // with the same initialized PAINTSTRUCT before returning.
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
                let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            {
                let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
                if let Some(state) = guard.as_mut() {
                    let capture = unsafe { GetCapture() };
                    state.click_pending =
                        !state.menu_open && (capture.is_invalid() || capture == hwnd);
                }
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEACTIVATE => {
            // Do not delegate activation to Explorer or eat the first click.
            LRESULT(windows::Win32::UI::WindowsAndMessaging::MA_NOACTIVATE as isize)
        }
        WM_MOUSEMOVE => {
            if wparam.0 & 0x0001 != 0 {
                // Moving with the button held is not a click or a placement action.
                let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
                if let Some(state) = guard.as_mut() {
                    state.click_pending = false;
                }
            } else {
                set_hovering(true);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let click = {
                let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
                guard.as_mut().is_some_and(|state| {
                    let click = state.click_pending && !state.menu_open;
                    state.click_pending = false;
                    click
                })
            };
            if click {
                toggle_details();
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                state.click_pending = false;
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE => {
            {
                let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
                if let Some(state) = guard.as_mut() {
                    state.click_pending = false;
                }
            }
            let capture = unsafe { GetCapture() };
            if !capture.is_invalid() && capture != hwnd {
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        WM_LBUTTONDBLCLK => {
            // Active tests are explicit actions in the panel/menu, never clicks.
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                state.click_pending = false;
            }
            LRESULT(0)
        }
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            show_context_menu();
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            {
                let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
                if let Some(state) = guard.as_mut() {
                    state.click_pending = false;
                }
            }
            set_hovering(false);
            LRESULT(0)
        }
        WM_DESTROY => {
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            let mut tooltip = None;
            if let Some(state) = guard.as_mut() {
                // The controller survives widget destruction and the retry
                // timer can recreate the taskbar child window.
                if state.widget == hwnd {
                    tooltip = state.tooltip.take();
                    state.widget = HWND::default();
                    state.embedded = false;
                    state.widget_shown = false;
                    state.placement_available = false;
                    state.taskbar = None;
                    state.placed_for = None;
                    state.pending_canvas = None;
                    state.pending_position = None;
                    state.pending_visibility = None;
                }
            }
            drop(guard);
            drop(tooltip);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Rebuilds the widget after the taskbar came back.
fn recover_taskbar() {
    let needs_widget = {
        let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        match guard.as_ref() {
            Some(state) => state.widget.is_invalid(),
            None => false,
        }
    };
    if needs_widget {
        if let Ok(hinstance) = unsafe { GetModuleHandleW(PCWSTR::null()) } {
            let class = to_wide("NetSpeedWidgetTaskbar");
            create_widget(hinstance.into(), &class);
        }
    }
    attach_to_taskbar();
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(state) = guard.as_mut() {
        position_widget(state);
        render(state);
    }
}

/// Records that the cursor is over the widget, or has left it.
fn set_hovering(hovering: bool) {
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };
    if state.hovering == hovering {
        return;
    }
    state.hovering = hovering;
    if hovering {
        // Ask to be told when the cursor leaves again.
        unsafe {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: state.widget,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut tracking);
        }
    }
    render(state);
}

/// The UI refresh.
fn tick(timer: usize) {
    match timer {
        TIMER_LAYOUT_RETRY => {
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                // SAFETY: Stop this controller's retry timer after one tick.
                if unsafe {
                    windows::Win32::UI::WindowsAndMessaging::KillTimer(
                        state.controller,
                        TIMER_LAYOUT_RETRY,
                    )
                }
                .is_err()
                {
                    logger::warning("placement", "could not stop layout retry timer");
                }
                position_widget(state);
                render(state);
            }
            return;
        }
        TIMER_TICK => {}
        TIMER_BACKGROUND => {
            let preferred = preferred_taskbar();
            let (attached, due) = {
                let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
                match guard.as_ref() {
                    Some(state) => (
                        state.embedded
                            && state.taskbar.is_some_and(|bar| {
                                taskbar::is_window(bar.hwnd)
                                    && preferred.is_some_and(|wanted| wanted.hwnd == bar.hwnd)
                            }),
                        !state.speedtest_running && now_epoch() >= state.speedtest_next_due,
                    ),
                    None => return,
                }
            };
            if !attached {
                recover_taskbar();
            }
            if due {
                start_speedtest(false);
            }
            return;
        }
        _ => return,
    }

    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };
    state.tick_count += 1;
    if state.tick_count % 16 == 0 {
        let appearance = Appearance::current();
        if appearance != state.appearance {
            state.appearance = appearance;
            state.palette =
                theme::resolved_palette(&state.theme_name, appearance, state.subtle_background);
        }
    }

    if let Some(sample) = state.sampler.latest() {
        if sample.ts != state.last_sample_ts {
            state.last_sample_ts = sample.ts;
            apply_sample(state, sample);
        }
    }
    apply_latency(state);
    let available = state.last_sample_ts > 0.0 && now_epoch() - state.last_sample_ts <= 3.0;
    state.down_text = state
        .down_formatter
        .format(available.then_some(state.smooth_down), state.units);
    state.up_text = state
        .up_formatter
        .format(available.then_some(state.smooth_up), state.units);
    position_widget(state);
    render(state);

    if state.tick_count % 8 == 0 {
        update_tray_tooltip(state);
    }
    let snapshot = details_snapshot(state);
    let tooltip_text = format!("NetSpeed\n{}\n\n{}", snapshot.rates, snapshot.body);
    let mut tooltip = state.tooltip.take();
    let (width, height, widget) = (state.width, state.height, state.widget);
    let accessible_name = (state.tick_count % 8 == 0).then(|| {
        to_wide(&format!(
            "NetSpeed: download {} {}, upload {} {}, probe {}",
            state.down_text.value,
            state.down_text.unit,
            state.up_text.value,
            state.up_text.unit,
            ping_text(state)
        ))
    });
    drop(guard);
    if let Some(tooltip) = tooltip.as_mut() {
        tooltip.update(&tooltip_text, width, height);
    }
    if let Some(name) = accessible_name {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                widget,
                PCWSTR(name.as_ptr()),
            );
        }
    }
    details::update(snapshot);
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(state) = guard.as_mut() {
        if state.widget == widget {
            state.tooltip = tooltip;
        }
    }
}

/// Folds a new sample into the smoothed speeds and the peak log.
fn apply_sample(state: &mut AppState, sample: NetSample) {
    // A true zero must not become an indefinitely decaying phantom transfer.
    state.smooth_down = if sample.down_mbps == 0.0 {
        0.0
    } else {
        state.smooth_down + (sample.down_mbps - state.smooth_down) * 0.45
    };
    state.smooth_up = if sample.up_mbps == 0.0 {
        0.0
    } else {
        state.smooth_up + (sample.up_mbps - state.smooth_up) * 0.45
    };

    if sample.up_mbps > state.max_up_seen && sample.up_mbps >= 1.0 {
        state.max_up_seen = sample.up_mbps;
        logger::info(
            "sampler",
            &format!("new upstream peak {:.2} Mb/s", sample.up_mbps),
        );
    }
    if sample.down_mbps > state.max_down_seen && sample.down_mbps >= 1.0 {
        state.max_down_seen = sample.down_mbps;
        logger::info(
            "sampler",
            &format!("new downstream peak {:.2} Mb/s", sample.down_mbps),
        );
    }
}

/// Records the health of the latest latency reading.
fn apply_latency(state: &mut AppState) {
    let result = state.probe.latest();
    if result.ts == 0.0 {
        return;
    }
    if state.speedtest_running {
        // A running speedtest saturates the connection, so the probe reports
        // artificially high values. Those must not be recorded as real health
        // transitions, or every run would stamp a false outage onto the graph.
        return;
    }
    state.health = health_for_latency(result);
}

fn health_for_latency(result: LatencyResult) -> Health {
    if !result.ok {
        Health::Bad
    } else {
        match result.ms {
            Some(ms) if ms < LATENCY_GOOD_MS => Health::Good,
            _ => Health::Warn,
        }
    }
}

/// Draws the widget.
fn render(state: &mut AppState) {
    if state.widget.is_invalid() || !state.widget_visible || !state.placement_available {
        return;
    }
    let Some(fonts) = state.fonts.as_ref() else {
        return;
    };
    let mut canvas = Canvas::new(state.width, state.height);
    let samples = state.sampler.history();
    let result = state.probe.latest();
    let ping_text = ping_text(state);
    let ping_pending =
        state.speedtest_running || result.ts == 0.0 || now_epoch() - result.ts > 10.0;
    let view = WidgetView {
        down: &state.down_text,
        up: &state.up_text,
        ping_text: &ping_text,
        health: state.health,
        ping_pending,
        samples: &samples,
        layout: state.effective_layout,
        show_graph: state.show_graph,
        graph_fill: state.graph_fill && !state.appearance.high_contrast,
    };
    paint::paint(&mut canvas, fonts, &view, &state.palette, &mut state.graph);
    state.pending_canvas = Some(canvas);
}

/// Win32 may dispatch sent messages while waiting for Explorer. Never run these
/// effects with STATE locked, including when a nested window procedure returns.
fn flush_native_effects() {
    thread_local! { static FLUSHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if FLUSHING.with(|flag| flag.replace(true)) {
        return;
    }
    loop {
        let effects = {
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            guard.as_mut().map(|state| {
                let tip = state.pending_tray_tip.take();
                let tray = if tip.is_some() {
                    state.tray.take()
                } else {
                    None
                };
                (
                    state.widget,
                    state.width,
                    state.height,
                    state.pending_position.take(),
                    state.pending_visibility.take(),
                    state.pending_canvas.take(),
                    tip,
                    tray,
                )
            })
        };
        let Some((widget, width, height, position, visibility, canvas, tip, mut tray)) = effects
        else {
            break;
        };
        if position.is_none() && visibility.is_none() && canvas.is_none() && tip.is_none() {
            break;
        }
        let positioned = position.map(|origin| unsafe {
            SetWindowPos(
                widget,
                HWND_TOP,
                origin.x,
                origin.y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .is_ok()
        });
        let current = {
            let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            guard.as_ref().is_some_and(|state| state.widget == widget)
        };
        let mut blitted = None;
        if current {
            if positioned == Some(false) {
                unsafe {
                    let _ = ShowWindow(widget, SW_HIDE);
                }
            } else {
                if let Some(canvas) = canvas {
                    blitted = Some(blit_layered(widget, &canvas));
                }
                if let Some(show) = visibility {
                    unsafe {
                        let _ = ShowWindow(widget, if show { SW_SHOW } else { SW_HIDE });
                    }
                }
            }
        }
        if let (Some(tray), Some(tip)) = (tray.as_mut(), tip) {
            tray.set_tooltip(&tip);
        }
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        if let Some(state) = guard.as_mut() {
            if state.widget == widget {
                if positioned == Some(false) {
                    state.placement_available = false;
                    state.widget_shown = false;
                    state.placed_for = None;
                }
                if let Some(ok) = blitted {
                    if ok == state.blit_failed {
                        if ok {
                            logger::info("paint", "layered rendering recovered");
                        } else {
                            logger::warning("paint", "layered rendering failed");
                        }
                    }
                    state.blit_failed = !ok;
                }
            }
            if state.tray.is_none() {
                state.tray = tray.take();
            }
        }
        drop(guard);
        drop(tray);
    }
    FLUSHING.with(|flag| flag.set(false));
}

/// Shared text for native hover details and the keyboard-accessible panel.
fn details_snapshot(state: &AppState) -> details::Snapshot {
    let result = state.probe.latest();
    let probe = if result.ts == 0.0 {
        "Waiting for first probe".to_string()
    } else if now_epoch() - result.ts > 10.0 {
        "Probe result unavailable (stale)".to_string()
    } else if !result.ok {
        "Probe failed (TCP and ICMP)".to_string()
    } else if let Some(ms) = result.ms {
        format!("TCP latency: {ms:.0} ms")
    } else {
        "ICMP reachable; TCP probe blocked".to_string()
    };
    let (down, up) = state.sampler.session_totals();
    let speedtest = config::speedtest()
        .map(|test| {
            let source =
                config::speedtest_source().unwrap_or_else(|| "provider not recorded".to_string());
            format!(
                "Last measured test · {}\r\n↓ {:.1} Mbps   ↑ {:.1} Mbps\r\nSource: {}",
                age_label(test.ts, now_epoch()),
                test.down_mbps,
                test.up_mbps,
                source
            )
        })
        .unwrap_or_else(|| "No measured speedtest yet".to_string());
    let samples = state.sampler.history();
    let now = now_epoch();
    let (peak_down, peak_up) = samples
        .iter()
        .filter(|sample| now - sample.ts <= 60.0)
        .fold((0.0_f64, 0.0_f64), |(down, up), sample| {
            (down.max(sample.down_mbps), up.max(sample.up_mbps))
        });
    let sampling = if state.last_sample_ts == 0.0 {
        "Waiting for first traffic sample\r\n"
    } else if now - state.last_sample_ts > 3.0 {
        "Traffic sample unavailable\r\n"
    } else {
        ""
    };
    let testing = if state.speedtest_running {
        "Speedtest running; health held steady\r\n".to_string()
    } else if state.speedtest_summary.starts_with("Speedtest estimate")
        || state.speedtest_summary == "Speedtest failed"
    {
        format!("{}\r\n", state.speedtest_summary)
    } else {
        String::new()
    };
    let down_total = transfer_label(down);
    let up_total = transfer_label(up);
    let legend = if state.appearance.high_contrast {
        "Download: solid · Upload: dashed"
    } else {
        "Cyan ↓ download · Lavender ↑ upload"
    };
    let target = format!(
        "{}:{}",
        crate::latency::PING_HOST,
        crate::latency::PING_PORT
    );
    details::Snapshot {
        rates: format!(
            "↓ {} {}    ↑ {} {}",
            state.down_text.value, state.down_text.unit, state.up_text.value, state.up_text.unit
        ),
        body: format!(
            "{sampling}{probe}\r\nTarget: {target} · TCP / ICMP fallback\r\n\r\n\
             Session: ↓ {down_total}   ↑ {up_total}\r\n\r\n{testing}{speedtest}\r\n\r\n\
             60 s peaks: ↓ {peak_down:.1} / ↑ {peak_up:.1} Mbps\r\n{legend}\r\n\
             Graph: shared automatic scale, not utilization.\r\nActive tests transfer data."
        ),
        samples,
        palette: state.palette,
        fill: state.graph_fill && !state.appearance.high_contrast,
        testing: state.speedtest_running,
    }
}

fn toggle_details() {
    let (owner, anchor, dpi, snapshot, tooltip, widget) = {
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_mut() else {
            return;
        };
        let anchor = state
            .placed_for
            .or_else(|| {
                state
                    .taskbar
                    .and_then(|bar| taskbar::taskbar_rect(bar.hwnd))
            })
            .unwrap_or(RECT {
                left: 100,
                top: 600,
                right: 440,
                bottom: 648,
            });
        (
            state.controller,
            anchor,
            state
                .taskbar
                .map(|bar| unsafe { GetDpiForWindow(bar.hwnd) })
                .filter(|dpi| *dpi != 0)
                .unwrap_or(state.dpi)
                .max(96),
            details_snapshot(state),
            state.tooltip.take(),
            state.widget,
        )
    };
    if let Some(tooltip) = tooltip.as_ref() {
        tooltip.hide();
    }
    {
        let mut tooltip = tooltip;
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        if let Some(state) = guard.as_mut() {
            if state.widget == widget {
                state.tooltip = tooltip.take();
            }
        }
        drop(guard);
        drop(tooltip);
    }
    details::toggle(
        owner,
        anchor,
        dpi,
        snapshot,
        CMD_RUN_SPEEDTEST,
        CMD_SETTINGS,
    );
}

/// Builds the context menu.
fn build_menu(state: &mut AppState) -> Vec<MenuEntry> {
    let mut entries: Vec<MenuEntry> = Vec::new();
    entries.push(MenuEntry::Command {
        id: CMD_DETAILS,
        label: "Network details".to_string(),
    });
    let themes: Vec<MenuEntry> = theme::theme_names()
        .iter()
        .enumerate()
        .map(|(index, name)| MenuEntry::Check {
            id: CMD_THEME_BASE + index as u32,
            label: if *name == theme::DEFAULT_THEME {
                "Follow Windows (transparent)".to_string()
            } else {
                (*name).to_string()
            },
            checked: *name == state.theme_name,
        })
        .collect();
    entries.push(MenuEntry::Submenu {
        label: "Theme".to_string(),
        items: themes,
    });

    entries.push(MenuEntry::Check {
        id: CMD_TOGGLE_GRAPH,
        label: "Show traffic graph".to_string(),
        checked: state.show_graph,
    });
    entries.push(MenuEntry::Submenu {
        label: "Layout".to_string(),
        items: [
            DisplayLayout::Compact,
            DisplayLayout::Balanced,
            DisplayLayout::SingleLine,
        ]
        .iter()
        .map(|layout| MenuEntry::Check {
            id: match layout {
                DisplayLayout::Compact => CMD_LAYOUT_COMPACT,
                DisplayLayout::Balanced => CMD_LAYOUT_BALANCED,
                DisplayLayout::SingleLine => CMD_LAYOUT_SINGLE_LINE,
            },
            label: match layout {
                DisplayLayout::Compact => "Compact",
                DisplayLayout::Balanced => "Balanced (recommended)",
                DisplayLayout::SingleLine => "Single line",
            }
            .to_string(),
            checked: state.layout == *layout,
        })
        .chain(std::iter::once(MenuEntry::Check {
            id: CMD_AUTO_FIT,
            label: "Auto-fit crowded taskbars".to_string(),
            checked: state.auto_fit,
        }))
        .collect(),
    });
    entries.push(MenuEntry::Submenu {
        label: "Units".to_string(),
        items: vec![
            MenuEntry::Check {
                id: CMD_UNITS_BITS,
                label: "Bits (Kbps / Mbps / Gbps)".to_string(),
                checked: state.units == SpeedUnits::Bits,
            },
            MenuEntry::Check {
                id: CMD_UNITS_BYTES,
                label: "Bytes (kB/s / MB/s / GB/s, decimal)".to_string(),
                checked: state.units == SpeedUnits::Bytes,
            },
        ],
    });
    entries.push(MenuEntry::Check {
        id: CMD_SUBTLE_BACKGROUND,
        label: "Subtle background (Follow Windows)".to_string(),
        checked: state.subtle_background,
    });
    entries.push(MenuEntry::Check {
        id: CMD_GRAPH_FILL,
        label: "Shade download area".to_string(),
        checked: state.graph_fill,
    });

    let placement_items = vec![
        MenuEntry::Check {
            id: CMD_PLACEMENT_TRAY,
            label: "Next to the notification area".to_string(),
            checked: state.placement == Placement::Tray,
        },
        MenuEntry::Check {
            id: CMD_PLACEMENT_LEFT,
            label: "Prefer left side".to_string(),
            checked: state.placement == Placement::Left,
        },
    ];
    entries.push(MenuEntry::Submenu {
        label: "Placement".to_string(),
        items: placement_items,
    });
    if state.widget_visible && !state.placement_available {
        entries.push(MenuEntry::Disabled {
            label: "Taskbar space unavailable (tray only)".to_string(),
        });
    }

    let taskbars = taskbar::find_taskbars();
    state.menu_devices = taskbars.iter().map(taskbar::monitor_device).collect();
    if taskbars.len() > 1 {
        let current = config::taskbar_monitor();
        let items: Vec<MenuEntry> = taskbars
            .iter()
            .enumerate()
            .map(|(index, taskbar)| {
                let device = taskbar::monitor_device(taskbar);
                let label = match &device {
                    Some(device) => format!("Display {device}"),
                    None => format!("Display {}", index + 1),
                };
                MenuEntry::Check {
                    id: CMD_TASKBAR_BASE + index as u32,
                    label,
                    checked: if current.is_some() {
                        current == device
                    } else {
                        state.taskbar.is_some_and(|bar| bar.hwnd == taskbar.hwnd)
                    },
                }
            })
            .collect();
        entries.push(MenuEntry::Submenu {
            label: "Display".to_string(),
            items,
        });
    }

    entries.push(MenuEntry::Command {
        id: CMD_CHANGE_HOTKEY,
        label: hotkey_label(state.hotkey_ok, &state.hotkey_combo),
    });
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Check {
        id: CMD_TOGGLE_VISIBILITY,
        label: "Show widget".to_string(),
        checked: state.widget_visible,
    });
    entries.push(match startup::enabled() {
        Ok(checked) => MenuEntry::Check {
            id: CMD_TOGGLE_AUTOSTART,
            label: "Start with Windows".to_string(),
            checked,
        },
        Err(err) => {
            logger::error("startup", &err);
            MenuEntry::Disabled {
                label: "Start with Windows (registry unavailable)".to_string(),
            }
        }
    });
    entries.push(MenuEntry::Command {
        id: CMD_QUIT,
        label: "Quit".to_string(),
    });
    entries
}

/// Shows the context menu at the cursor.
fn show_context_menu() {
    let (entries, owner) = {
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_mut() else {
            return;
        };
        if state.menu_open {
            return;
        }
        (build_menu(state), state.controller)
    };
    if owner.is_invalid() {
        return;
    }
    let mut cursor = POINT::default();
    unsafe {
        if GetCursorPos(&mut cursor).is_err() {
            return;
        }
    }
    show_popup_menu(owner, &entries, cursor);
}

/// Shows the tray menu at the cursor.
fn show_tray_menu() {
    let (entries, owner) = {
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_mut() else {
            return;
        };
        if state.menu_open {
            return;
        }
        (build_menu(state), state.controller)
    };
    let mut cursor = POINT::default();
    unsafe {
        if GetCursorPos(&mut cursor).is_err() {
            return;
        }
    }
    let owner = if owner.is_invalid() {
        state_controller()
    } else {
        owner
    };
    show_popup_menu(owner, &entries, cursor);
}

/// Guard against opening another menu inside the nested message loop.
fn show_popup_menu(owner: HWND, entries: &[MenuEntry], cursor: POINT) {
    {
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_mut() else {
            return;
        };
        if state.menu_open {
            return;
        }
        state.menu_open = true;
    }
    let selected = menu::show(owner, entries, cursor.x, cursor.y);
    {
        let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        if let Some(state) = guard.as_mut() {
            state.menu_open = false;
        }
    }
    if let Some(id) = selected {
        handle_command(id);
    }
}

/// The controller window, used as a menu owner when the widget is gone.
fn state_controller() -> HWND {
    let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    match guard.as_ref() {
        Some(state) => state.controller,
        None => HWND::default(),
    }
}

/// Reacts to a click on the tray icon.
fn handle_tray_click(event: u32) {
    match event {
        x if x == WM_LBUTTONUP => toggle_visibility(),
        x if x == WM_RBUTTONUP || x == WM_CONTEXTMENU => show_tray_menu(),
        _ => {}
    }
}

/// Runs the command behind a menu entry.
fn handle_command(id: u32) {
    if id == 0 {
        return;
    }
    if (CMD_THEME_BASE..CMD_THEME_BASE + 500).contains(&id) {
        let index = (id - CMD_THEME_BASE) as usize;
        if let Some((name, _)) = theme::THEMES.get(index) {
            set_theme(name);
        }
        return;
    }
    if (CMD_TASKBAR_BASE..CMD_TASKBAR_BASE + 100).contains(&id) {
        let index = (id - CMD_TASKBAR_BASE) as usize;
        let device = {
            let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            guard
                .as_ref()
                .and_then(|state| state.menu_devices.get(index).cloned().flatten())
        };
        if let Some(device) = device {
            config::set_taskbar_monitor(Some(device));
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                state.taskbar = None;
                state.embedded = false;
                state.placed_for = None;
            }
            drop(guard);
            attach_to_taskbar();
        }
        return;
    }

    match id {
        CMD_DETAILS => toggle_details(),
        CMD_SETTINGS => show_context_menu(),
        CMD_LAYOUT_COMPACT
        | CMD_LAYOUT_BALANCED
        | CMD_LAYOUT_SINGLE_LINE
        | CMD_UNITS_BITS
        | CMD_UNITS_BYTES
        | CMD_AUTO_FIT
        | CMD_SUBTLE_BACKGROUND
        | CMD_GRAPH_FILL => {
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                match id {
                    CMD_LAYOUT_COMPACT => state.layout = DisplayLayout::Compact,
                    CMD_LAYOUT_BALANCED => state.layout = DisplayLayout::Balanced,
                    CMD_LAYOUT_SINGLE_LINE => state.layout = DisplayLayout::SingleLine,
                    CMD_UNITS_BITS | CMD_UNITS_BYTES => {
                        state.units = if id == CMD_UNITS_BITS {
                            SpeedUnits::Bits
                        } else {
                            SpeedUnits::Bytes
                        };
                        state.down_formatter = RateFormatter::default();
                        state.up_formatter = RateFormatter::default();
                    }
                    CMD_AUTO_FIT => state.auto_fit = !state.auto_fit,
                    CMD_SUBTLE_BACKGROUND => state.subtle_background = !state.subtle_background,
                    CMD_GRAPH_FILL => state.graph_fill = !state.graph_fill,
                    _ => {}
                }
                use config::DisplayPreference;
                config::set_display_preference(match id {
                    CMD_LAYOUT_COMPACT | CMD_LAYOUT_BALANCED | CMD_LAYOUT_SINGLE_LINE => {
                        DisplayPreference::Layout(state.layout)
                    }
                    CMD_UNITS_BITS | CMD_UNITS_BYTES => DisplayPreference::Units(state.units),
                    CMD_AUTO_FIT => DisplayPreference::AutoFit(state.auto_fit),
                    CMD_SUBTLE_BACKGROUND => DisplayPreference::Background(state.subtle_background),
                    CMD_GRAPH_FILL => DisplayPreference::Fill(state.graph_fill),
                    _ => unreachable!("display command was matched above"),
                });
                state.palette = theme::resolved_palette(
                    &state.theme_name,
                    state.appearance,
                    state.subtle_background,
                );
                state.placed_for = None;
                state.density_settler.reset();
            }
        }
        CMD_RUN_SPEEDTEST => start_speedtest(true),
        CMD_TOGGLE_GRAPH => {
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                state.show_graph = !state.show_graph;
                config::set_show_graph(state.show_graph);
                state.placed_for = None;
            }
        }
        CMD_PLACEMENT_TRAY => {
            config::set_placement(Placement::Tray);
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                state.placement = Placement::Tray;
                state.placed_for = None;
            }
        }
        CMD_PLACEMENT_LEFT => {
            config::set_placement(Placement::Left);
            let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
            if let Some(state) = guard.as_mut() {
                state.placement = Placement::Left;
                state.placed_for = None;
            }
        }
        CMD_CHANGE_HOTKEY => change_hotkey(),
        CMD_TOGGLE_VISIBILITY => toggle_visibility(),
        CMD_TOGGLE_AUTOSTART => toggle_autostart(),
        CMD_QUIT => {
            shutdown();
            unsafe {
                PostQuitMessage(0);
            }
        }
        _ => {}
    }
}

fn toggle_autostart() {
    let result = startup::enabled().and_then(|enabled| {
        startup::set_enabled(!enabled)?;
        logger::info("startup", &format!("start with Windows: {}", !enabled));
        Ok(())
    });
    if let Err(err) = result {
        logger::error("startup", &err);
        let text = to_wide(&format!("Could not change Start with Windows.\n\n{err}"));
        let title = to_wide("NetSpeed Widget");
        // SAFETY: The owner is the UI-thread controller; both terminated strings
        // remain live throughout the synchronous call. STATE is not locked.
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            MessageBoxW(
                state_controller(),
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

/// Switches to another theme preset.
fn set_theme(name: &str) {
    config::set_theme(name);
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(state) = guard.as_mut() {
        state.theme_name = name.to_string();
        state.palette = theme::resolved_palette(name, state.appearance, state.subtle_background);
        render(state);
    }
    logger::info("app", &format!("theme set to {name}"));
}

/// Opens the hotkey capture dialog and applies the result.
fn change_hotkey() {
    let (owner, current) = {
        let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        match guard.as_ref() {
            Some(state) => (state.widget, state.hotkey_combo.clone()),
            None => return,
        }
    };
    dialog::capture(owner, &current, try_change_hotkey);
}

fn try_change_hotkey(combo: &str) -> Result<(), String> {
    let (mods, vk) = hotkey::parse_hotkey(combo)?;
    let controller = state_controller();
    let old_id = {
        let guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
        let Some(state) = guard.as_ref() else {
            return Err("application is shutting down".to_string());
        };
        if state.hotkey_ok && state.hotkey_combo == combo {
            return Ok(());
        }
        state.hotkey_id
    };
    let new_id = if old_id == HOTKEY_ID {
        HOTKEY_ID + 1
    } else {
        HOTKEY_ID
    };
    // Bind the new combo before releasing the old one, so a combo the system
    // refuses cannot leave the user with nothing bound.
    if let Err(err) = unsafe {
        RegisterHotKey(
            controller,
            new_id,
            HOT_KEY_MODIFIERS(mods | hotkey::MOD_NOREPEAT),
            vk,
        )
    } {
        let reason = format!("{err} ({:?})", err.code());
        logger::warning("hotkey", &format!("could not register '{combo}': {reason}"));
        return Err(reason);
    }
    unsafe {
        let _ = UnregisterHotKey(controller, old_id);
    }
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(state) = guard.as_mut() {
        state.hotkey_combo = combo.to_string();
        state.hotkey_ok = true;
        state.hotkey_id = new_id;
    }
    config::set_hotkey(combo);
    Ok(())
}

/// Stops the services and removes the tray icon.
fn shutdown() {
    let mut guard = state_slot().lock().unwrap_or_else(|err| err.into_inner());
    let Some(mut state) = guard.take() else {
        return;
    };
    drop(guard);
    logger::section("App exit");
    details::close();
    state.tooltip = None;
    state.sampler.stop();
    state.probe.stop();
    unsafe {
        let _ = UnregisterHotKey(state.controller, state.hotkey_id);
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(state.controller, TIMER_TICK);
        let _ =
            windows::Win32::UI::WindowsAndMessaging::KillTimer(state.controller, TIMER_BACKGROUND);
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(
            state.controller,
            TIMER_LAYOUT_RETRY,
        );
    }
    if let Some(tray) = state.tray.as_mut() {
        tray.remove();
    }
    state.tray = None;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(state.widget);
        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(state.controller);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_health_covers_tcp_icmp_and_failure() {
        let result = |ms, ok| LatencyResult { ms, ok, ts: 100.0 };
        assert_eq!(health_for_latency(result(Some(79.9), true)), Health::Good);
        assert_eq!(health_for_latency(result(Some(80.0), true)), Health::Warn);
        assert_eq!(health_for_latency(result(Some(1500.0), true)), Health::Warn);
        assert_eq!(health_for_latency(result(None, true)), Health::Warn);
        assert_eq!(health_for_latency(result(None, false)), Health::Bad);
    }
}
