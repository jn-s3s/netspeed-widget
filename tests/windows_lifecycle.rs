//! Real-window regressions, with isolated settings and no synthetic desktop clicks.
#![cfg(windows)]

use netspeed_widget::commands::*;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, FindWindowExW, FindWindowW, GetParent, GetWindow,
    GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    PostMessageW, SendMessageTimeoutW, WindowFromPoint, GWL_EXSTYLE, GWL_STYLE, GW_HWNDNEXT,
    SMTO_ABORTIFHUNG, WM_CANCELMODE, WM_CLOSE, WM_COMMAND, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NULL, WM_RBUTTONUP, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_POPUP,
};

struct RunningApp {
    child: Child,
    data: std::path::PathBuf,
}

impl Drop for RunningApp {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "Application log: {}",
                std::fs::read_to_string(self.data.join("NetSpeedWidget/log.txt"))
                    .unwrap_or_default()
            );
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn responsive(hwnd: HWND, message: u32) {
    let mut result = 0;
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                hwnd,
                message,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                2000,
                Some(&mut result),
            )
        }
        .0,
        0,
        "window did not answer message {message:#x}"
    );
}

fn command(hwnd: HWND, id: u32) {
    let mut result = 0;
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_COMMAND,
                WPARAM(id as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                2000,
                Some(&mut result),
            )
        }
        .0,
        0,
        "command {id} did not finish"
    );
}

fn is_above(mut hwnd: HWND, other: HWND) -> bool {
    while let Ok(next) = unsafe { GetWindow(hwnd, GW_HWNDNEXT) } {
        if next == other {
            return true;
        }
        hwnd = next;
    }
    false
}

fn find_widget() -> Option<HWND> {
    let class = wide("NetSpeedWidgetTaskbar");
    for name in ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
        let taskbar_class = wide(name);
        let mut after = HWND::default();
        while let Ok(taskbar) = unsafe {
            FindWindowExW(
                HWND::default(),
                after,
                PCWSTR(taskbar_class.as_ptr()),
                PCWSTR::null(),
            )
        } {
            if let Ok(widget) = unsafe {
                FindWindowExW(
                    taskbar,
                    HWND::default(),
                    PCWSTR(class.as_ptr()),
                    PCWSTR::null(),
                )
            } {
                return Some(widget);
            }
            after = taskbar;
        }
    }
    None
}

struct OpenMenu(HWND);

fn assert_menu_organization(popup: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMenuItemCount, GetMenuItemID, GetMenuItemInfoW, GetMenuStringW, GetSubMenu, HMENU,
        MENUITEMINFOW, MFT_RADIOCHECK, MF_BYPOSITION, MIIM_FTYPE,
    };
    // MN_GETHMENU retrieves the native handle from a popup window without input injection.
    let mut result = 0;
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                popup,
                0x01E1,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                2000,
                Some(&mut result),
            )
        }
        .0,
        0
    );
    let menu = HMENU(result as *mut _);
    fn labels(menu: HMENU) -> Vec<String> {
        (0..unsafe { GetMenuItemCount(menu) })
            .map(|index| {
                let mut text = [0_u16; 256];
                let count =
                    unsafe { GetMenuStringW(menu, index as u32, Some(&mut text), MF_BYPOSITION) };
                String::from_utf16_lossy(&text[..count as usize])
            })
            .collect()
    }
    let root = labels(menu);
    assert_eq!(&root[..2], &["Network details", "Show widget"]);
    let appearance_index = root.iter().position(|label| label == "Appearance").unwrap();
    assert_eq!(
        &root[appearance_index..],
        &["Appearance", "Taskbar", "Preferences", "", "Quit"]
    );
    let appearance = unsafe { GetSubMenu(menu, appearance_index as i32) };
    let appearance_labels = labels(appearance);
    assert_eq!(
        &appearance_labels[..5],
        &["Layout", "Units", "Theme", "Subtle background", ""]
    );
    assert!(appearance_labels[5].starts_with("Show traffic graph"));
    assert_eq!(
        &appearance_labels[6..],
        &["Shade download area", "Animate error indicator"]
    );
    for index in 0..3 {
        let submenu = unsafe { GetSubMenu(appearance, index) };
        let mut info = MENUITEMINFOW {
            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE,
            ..Default::default()
        };
        unsafe { GetMenuItemInfoW(submenu, 0, true, &mut info) }.unwrap();
        assert!(info.fType.contains(MFT_RADIOCHECK));
    }
    let layout = unsafe { GetSubMenu(appearance, 0) };
    assert_eq!(
        labels(layout),
        [
            "Compact",
            "Balanced",
            "Single line",
            "",
            "Auto-fit crowded taskbars"
        ]
    );
    let taskbar = unsafe { GetSubMenu(menu, appearance_index as i32 + 1) };
    let taskbar_labels = labels(taskbar);
    assert_eq!(
        &taskbar_labels[..2],
        &["Next to notification area", "Prefer left side"]
    );
    assert!(taskbar_labels.len() == 2 || taskbar_labels[2..] == ["", "Display"]);
    let preferences = unsafe { GetSubMenu(menu, appearance_index as i32 + 2) };
    assert!(labels(preferences)[0].starts_with("Start with Windows"));
    assert!(labels(preferences)[1].starts_with("Change visibility shortcut…"));
    assert_eq!(unsafe { GetMenuItemID(preferences, 1) }, CMD_CHANGE_HOTKEY);
    fn assert_max_depth(menu: HMENU, depth: usize) {
        assert!(depth <= 2, "more than two submenu levels");
        for index in 0..unsafe { GetMenuItemCount(menu) } {
            let submenu = unsafe { GetSubMenu(menu, index) };
            if !submenu.is_invalid() {
                assert_max_depth(submenu, depth + 1);
            }
        }
    }
    assert_max_depth(menu, 0);
}

struct SiblingWindow(HWND);

impl Drop for SiblingWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

fn mouse_movement_preserves_position_and_sibling_capture(widget: HWND, taskbar: HWND) {
    // A hidden sibling models another embedded app without sending desktop
    // clicks or changing the real widget's state.
    let class = wide("STATIC");
    let sibling = SiblingWindow(
        unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_CHILD,
                0,
                0,
                1,
                1,
                taskbar,
                None,
                None,
                None,
            )
        }
        .unwrap(),
    );
    let capture = unsafe { GetCapture() };
    let mut before = RECT::default();
    unsafe { GetWindowRect(widget, &mut before) }.unwrap();
    responsive(widget, WM_LBUTTONDOWN);
    assert_eq!(
        unsafe { GetCapture() },
        capture,
        "widget must not capture the mouse"
    );
    let mut result = 0;
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                widget,
                WM_MOUSEMOVE,
                WPARAM(1),
                LPARAM(50 | (10 << 16)),
                SMTO_ABORTIFHUNG,
                2000,
                Some(&mut result),
            )
        }
        .0,
        0
    );
    responsive(widget, WM_LBUTTONUP);
    let mut after = RECT::default();
    unsafe { GetWindowRect(widget, &mut after) }.unwrap();
    assert_eq!(
        before, after,
        "mouse movement must not reposition the widget"
    );
    unsafe {
        SetCapture(sibling.0);
    }
    responsive(widget, WM_NULL);
    assert_eq!(
        unsafe { GetCapture() },
        sibling.0,
        "capture loss must not release another taskbar app's capture"
    );
    responsive(widget, WM_LBUTTONUP);
    assert_eq!(unsafe { GetCapture() }, sibling.0);
    responsive(widget, WM_LBUTTONDOWN);
    assert_eq!(
        unsafe { GetCapture() },
        sibling.0,
        "NetSpeed must not steal an existing taskbar drag"
    );
    responsive(widget, WM_CANCELMODE);
    assert_eq!(unsafe { GetCapture() }, sibling.0);
}

impl Drop for OpenMenu {
    fn drop(&mut self) {
        let _ = unsafe { PostMessageW(self.0, WM_CANCELMODE, WPARAM(0), LPARAM(0)) };
    }
}

fn popup_stays_in_front_of_widget(
    widget: HWND,
    controller: HWND,
    source: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) {
    let _cleanup = OpenMenu(controller);
    unsafe { PostMessageW(source, message, wparam, lparam) }.unwrap();
    let class = wide("#32768");
    let thread = unsafe { GetWindowThreadProcessId(widget, None) };
    let deadline = Instant::now() + Duration::from_secs(3);
    let popup = loop {
        let mut after = HWND::default();
        let mut found = None;
        while let Ok(hwnd) = unsafe {
            FindWindowExW(
                HWND::default(),
                after,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
            )
        } {
            if unsafe { GetWindowThreadProcessId(hwnd, None) } == thread {
                found = Some(hwnd);
                break;
            }
            after = hwnd;
        }
        if let Some(hwnd) = found {
            break hwnd;
        }
        assert!(Instant::now() < deadline, "right-click popup did not open");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_menu_organization(popup);
    // Let multiple UI timers run inside TrackPopupMenuEx's nested message loop.
    // The menu must stay above the taskbar containing the widget.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        // Explicitly deliver refresh ticks so scheduler delays cannot turn this
        // into a false pass without exercising the refresh path.
        let mut result = 0;
        assert_ne!(
            unsafe {
                SendMessageTimeoutW(
                    controller,
                    windows::Win32::UI::WindowsAndMessaging::WM_TIMER,
                    WPARAM(1),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    2000,
                    Some(&mut result),
                )
            }
            .0,
            0
        );
        assert!(
            is_above(popup, unsafe { GetParent(widget) }.unwrap()),
            "widget covered its own right-click popup"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_keyboard_submenus(popup, thread);
    unsafe { PostMessageW(controller, WM_CANCELMODE, WPARAM(0), LPARAM(0)) }.unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while unsafe { IsWindow(popup) }.as_bool() {
        assert!(Instant::now() < deadline, "right-click popup did not close");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_keyboard_submenus(mut popup: HWND, thread: u32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMenuItemCount, GetMenuState, GetMenuStringW, GetSubMenu, HMENU, MF_BYPOSITION,
        MF_HILITE, WM_KEYDOWN,
    };

    fn popup_menu(popup: HWND) -> HMENU {
        let mut result = 0;
        assert_ne!(
            unsafe {
                SendMessageTimeoutW(
                    popup,
                    0x01E1,
                    WPARAM(0),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    2000,
                    Some(&mut result),
                )
            }
            .0,
            0
        );
        HMENU(result as *mut _)
    }

    fn selected(menu: HMENU) -> Option<u32> {
        (0..unsafe { GetMenuItemCount(menu) } as u32)
            .find(|index| unsafe { GetMenuState(menu, *index, MF_BYPOSITION) & MF_HILITE.0 != 0 })
    }

    fn press_key(popup: HWND, key: usize) {
        unsafe {
            PostMessageW(popup, WM_KEYDOWN, WPARAM(key), LPARAM(1)).unwrap();
            PostMessageW(
                popup,
                windows::Win32::UI::WindowsAndMessaging::WM_KEYUP,
                WPARAM(key),
                LPARAM(0xC0000001_u32 as isize),
            )
            .unwrap();
        }
    }

    // Native menus may start without a selection, or with a mouse-hover selection.
    // Wait for each arrow key to change the actual highlighted item before proceeding.
    // Messages stay within this app; no desktop keyboard input or settings changes.
    for label in ["Appearance", "Layout"] {
        let menu = popup_menu(popup);
        let target = (0..unsafe { GetMenuItemCount(menu) } as u32)
            .find(|index| {
                let mut text = [0_u16; 256];
                let count = unsafe { GetMenuStringW(menu, *index, Some(&mut text), MF_BYPOSITION) };
                String::from_utf16_lossy(&text[..count as usize]) == label
            })
            .unwrap();
        for _ in 0..unsafe { GetMenuItemCount(menu) } {
            let before = selected(menu);
            if before == Some(target) {
                break;
            }
            press_key(popup, 0x28);
            let deadline = Instant::now() + Duration::from_secs(3);
            while selected(menu) == before {
                assert!(
                    Instant::now() < deadline,
                    "Down did not move the menu selection toward {label}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        assert_eq!(selected(menu), Some(target), "could not select {label}");
        let submenu = unsafe { GetSubMenu(menu, target as i32) };
        press_key(popup, 0x27);
        let deadline = Instant::now() + Duration::from_secs(3);
        let class = wide("#32768");
        popup = loop {
            let mut after = HWND::default();
            let mut found = None;
            while let Ok(hwnd) = unsafe {
                FindWindowExW(
                    HWND::default(),
                    after,
                    PCWSTR(class.as_ptr()),
                    PCWSTR::null(),
                )
            } {
                if unsafe { GetWindowThreadProcessId(hwnd, None) } == thread
                    && unsafe { IsWindowVisible(hwnd) }.as_bool()
                    && popup_menu(hwnd) == submenu
                {
                    found = Some(hwnd);
                    break;
                }
                after = hwnd;
            }
            if let Some(hwnd) = found {
                break hwnd;
            }
            assert!(Instant::now() < deadline, "Right did not open {label}");
            std::thread::sleep(Duration::from_millis(10));
        };
    }
}

#[test]
fn widget_embeds_in_taskbar_with_automatic_placement() {
    let shell = wide("Shell_TrayWnd");
    if unsafe { FindWindowW(PCWSTR(shell.as_ptr()), PCWSTR::null()) }.is_err() {
        eprintln!(
            "::warning::Desktop lifecycle suite SKIPPED: Explorer taskbar is unavailable; no window-layer assertions ran. Run this test on a developer desktop."
        );
        return;
    }
    let class = wide("NetSpeedWidgetTaskbar");
    assert!(
        find_widget().is_none()
            && unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.is_err(),
        "Quit the running widget before running the real-window regression test"
    );
    let data = std::env::temp_dir().join(format!("netspeed-ui-test-{}", std::process::id()));
    std::fs::create_dir_all(&data).unwrap();
    // UI verification must not start an automatic data-consuming transfer test.
    let config_dir = data.join("NetSpeedWidget");
    std::fs::create_dir_all(&config_dir).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    std::fs::write(
        config_dir.join("config.json"),
        serde_json::to_string(&serde_json::json!({
            "speedtest": { "down_mbps": 312.0, "up_mbps": 42.0, "ts": now },
            "speedtest_source": "UI test fixture",
            "placement": "manual",
            "tray_offset": 123
        }))
        .unwrap(),
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_netspeed-widget"))
        .env("APPDATA", &data)
        .spawn()
        .unwrap();
    let mut app = RunningApp { child, data };
    let deadline = Instant::now() + Duration::from_secs(10);
    let widget = loop {
        if let Some(hwnd) = find_widget() {
            break hwnd;
        }
        assert!(
            app.child.try_wait().unwrap().is_none(),
            "widget exited at startup"
        );
        assert!(Instant::now() < deadline, "widget did not start");
        std::thread::sleep(Duration::from_millis(50));
    };
    let parent = unsafe { GetParent(widget) }.expect("widget must be a taskbar child");
    let style = unsafe { GetWindowLongW(widget, GWL_STYLE) } as u32;
    assert_ne!(style & WS_CHILD.0, 0);
    assert_eq!(style & WS_POPUP.0, 0);
    assert_eq!(
        unsafe { GetWindowLongW(widget, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
        0
    );
    responsive(widget, WM_NULL);
    let class = wide("NetSpeedWidgetController");
    let controller = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.unwrap();
    assert_ne!(
        unsafe { GetWindowLongW(controller, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0,
        0,
        "the hidden menu owner must keep settings above the taskbar"
    );
    let capture = unsafe { GetCapture() };
    responsive(widget, WM_LBUTTONDOWN);
    assert_eq!(
        unsafe { GetCapture() },
        capture,
        "clicking must not capture the mouse"
    );
    responsive(widget, WM_LBUTTONUP);
    responsive(widget, WM_NULL);
    // A normal click opens accessible details, not a data-consuming speedtest.
    let details_class = wide("NetSpeedDetails");
    let details = unsafe { FindWindowW(PCWSTR(details_class.as_ptr()), PCWSTR::null()) }.unwrap();
    assert!(unsafe { IsWindowVisible(details) }.as_bool());
    let body =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgItem(details, DETAILS_BODY_ID) }
            .unwrap();
    let length = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(body) };
    let mut text = vec![0_u16; length as usize + 1];
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(body, &mut text);
    }
    assert!(String::from_utf16_lossy(&text).contains("UI test fixture"));
    unsafe {
        PostMessageW(
            body,
            windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN,
            WPARAM(0x1B),
            LPARAM(0),
        )
    }
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while unsafe { IsWindowVisible(details) }.as_bool() {
        assert!(Instant::now() < deadline, "Escape did not dismiss details");
        std::thread::sleep(Duration::from_millis(10));
    }
    // Display preferences persist without disturbing taskbar parenting.
    for (command_id, key, value) in [
        (CMD_LAYOUT_COMPACT, "layout", "compact"),
        (CMD_UNITS_BYTES, "units", "bytes"),
        (CMD_LAYOUT_SINGLE_LINE, "layout", "single_line"),
        (CMD_LAYOUT_BALANCED, "layout", "balanced"),
        (CMD_UNITS_BITS, "units", "bits"),
    ] {
        command(controller, command_id);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(config_dir.join("config.json")).unwrap())
                .unwrap();
        assert_eq!(saved[key], value);
        assert_eq!(saved["placement"], "tray");
        assert!(saved.get("tray_offset").is_none());
        assert_eq!(unsafe { GetParent(widget) }.unwrap(), parent);
    }
    for (id, key, value) in [
        (CMD_AUTO_FIT, "auto_fit", false),
        (CMD_SUBTLE_BACKGROUND, "subtle_background", true),
        (CMD_GRAPH_FILL, "graph_fill", true),
        (CMD_TOGGLE_GRAPH, "show_graph", false),
        (CMD_TOGGLE_GRAPH, "show_graph", true),
    ] {
        command(controller, id);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(config_dir.join("config.json")).unwrap())
                .unwrap();
        assert_eq!(saved[key], value);
    }
    command(controller, CMD_THEME_BASE);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_dir.join("config.json")).unwrap())
            .unwrap();
    assert_eq!(saved["theme"], "Default");
    for (id, placement) in [(CMD_PLACEMENT_LEFT, "left"), (CMD_PLACEMENT_TRAY, "tray")] {
        command(controller, id);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(config_dir.join("config.json")).unwrap())
                .unwrap();
        assert_eq!(saved["placement"], placement);
        assert!(saved.get("tray_offset").is_none());
    }
    // Removed commands must not re-enable dragging or manual placement.
    for id in [1006, 1010, 1011] {
        command(controller, id);
    }
    mouse_movement_preserves_position_and_sibling_capture(widget, parent);
    // Let several refreshes execute, rather than only testing the startup path.
    std::thread::sleep(Duration::from_secs(3));
    responsive(widget, WM_NULL);
    assert_eq!(unsafe { GetParent(widget) }.unwrap(), parent);
    let mut rect = RECT::default();
    unsafe { GetWindowRect(widget, &mut rect) }.unwrap();
    // This point is empty System background, not text or a graph. Real mouse
    // hit-testing must reach the widget so right-click can open its menu.
    assert_eq!(
        unsafe {
            WindowFromPoint(POINT {
                x: rect.left + 100,
                y: rect.top + 2,
            })
        },
        widget,
        "transparent background must receive mouse input instead of passing it to the taskbar"
    );
    let taskbar_class = wide("Shell_TrayWnd");
    if let Ok(shell) = unsafe { FindWindowW(PCWSTR(taskbar_class.as_ptr()), PCWSTR::null()) } {
        responsive(shell, WM_NULL);
        let mut taskbar_rect = RECT::default();
        unsafe { GetWindowRect(shell, &mut taskbar_rect) }.unwrap();
        assert_eq!(
            rect.top,
            taskbar_rect.top + 1,
            "widget must leave the taskbar's top separator uncovered"
        );
        assert_eq!(
            rect.bottom, taskbar_rect.bottom,
            "widget must meet the bottom of the taskbar"
        );
    }
    popup_stays_in_front_of_widget(
        widget,
        controller,
        widget,
        WM_RBUTTONUP,
        WPARAM(0),
        LPARAM(0),
    );
    // Exercise the tray's right-click callback without clicking the system tray.
    popup_stays_in_front_of_widget(
        widget,
        controller,
        controller,
        windows::Win32::UI::WindowsAndMessaging::WM_USER + 20,
        WPARAM(0),
        LPARAM(WM_RBUTTONUP as isize),
    );
    command(controller, CMD_DETAILS);
    let details = unsafe { FindWindowW(PCWSTR(details_class.as_ptr()), PCWSTR::null()) }.unwrap();
    // The Network details Settings button uses native control ID 2.
    popup_stays_in_front_of_widget(
        widget,
        controller,
        details,
        WM_COMMAND,
        WPARAM(2),
        LPARAM(0),
    );
    responsive(controller, WM_NULL);
    // Visibility repairs must never undo an explicit manual hide.
    responsive(controller, WM_HOTKEY);
    std::thread::sleep(Duration::from_millis(600));
    assert!(!unsafe { IsWindowVisible(widget) }.as_bool());
    responsive(controller, WM_HOTKEY);
    assert!(unsafe { IsWindowVisible(widget) }.as_bool());
    // Losing the child (as when Explorer destroys its taskbar) must leave the
    // controller alive and allow its retry timer to rebuild the attachment.
    unsafe { PostMessageW(widget, WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while unsafe { IsWindow(widget) }.as_bool() {
        assert!(Instant::now() < deadline, "old widget was not destroyed");
        std::thread::sleep(Duration::from_millis(10));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let recovered = loop {
        if let Some(hwnd) = find_widget() {
            break hwnd;
        }
        assert!(Instant::now() < deadline, "taskbar child was not recreated");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(unsafe { GetParent(recovered) }.unwrap(), parent);
    responsive(recovered, WM_NULL);
    // A recreated child has a new handle; its asynchronous occupancy scan
    // must finish before automatic placement can show it again.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !unsafe { IsWindowVisible(recovered) }.as_bool() {
        assert!(Instant::now() < deadline, "recreated widget stayed hidden");
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe { PostMessageW(controller, WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = app.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "widget did not shut down");
        std::thread::sleep(Duration::from_millis(50));
    }
}
