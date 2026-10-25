//! Best-effort taskbar occupancy. UI Automation runs off the UI thread because
//! an Explorer or third-party accessibility provider can stop responding.

use std::cell::Cell;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetParent, IsChild, IsWindowVisible, PostMessageW, EVENT_OBJECT_CREATE,
    EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_REORDER,
    EVENT_OBJECT_SHOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP,
};

use super::taskbar;
use crate::config::{DisplayLayout, Placement};

/// Taskbar controls changed; request a fresh off-thread occupancy scan.
pub const WM_TASKBAR_LAYOUT_CHANGED: u32 = WM_APP + 2;
/// An occupancy scan finished; consume it without waiting for the paint timer.
pub const WM_TASKBAR_LAYOUT_READY: u32 = WM_APP + 3;
/// Minimum spacing between event-driven scans and their timer retries.
pub const LAYOUT_REFRESH_MS: u32 = 32;

#[derive(Clone, Copy, Default)]
struct EventTarget {
    controller: isize,
    taskbar: isize,
    widget: isize,
    posted: bool,
}

thread_local! {
    // Out-of-context WinEvent callbacks run on the installing UI thread. They
    // never acquire application locks or query accessibility providers.
    static EVENT_TARGET: Cell<EventTarget> = Cell::new(EventTarget::default());
}

struct TaskbarEvents(HWINEVENTHOOK);

impl TaskbarEvents {
    fn new(controller: HWND) -> Self {
        if controller.is_invalid() {
            return Self(HWINEVENTHOOK::default());
        }
        EVENT_TARGET.set(EventTarget {
            controller: controller.0 as isize,
            ..EventTarget::default()
        });
        // SAFETY: The callback has the required ABI and outlives the hook. This
        // UI thread pumps messages until the hook is unregistered in Drop.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_CREATE,
                EVENT_OBJECT_LOCATIONCHANGE,
                None,
                Some(taskbar_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            crate::logger::warning(
                "placement",
                "taskbar event hook unavailable; using layout polling",
            );
        }
        Self(hook)
    }

    fn watch(&self, taskbar: HWND, widget: HWND) {
        if !self.0.is_invalid() {
            EVENT_TARGET.set(EventTarget {
                taskbar: taskbar.0 as isize,
                widget: widget.0 as isize,
                ..EVENT_TARGET.get()
            });
        }
    }
}

impl Drop for TaskbarEvents {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            EVENT_TARGET.set(EventTarget::default());
            // SAFETY: This is the hook owned by this value, on its installing thread.
            if !unsafe { UnhookWinEvent(self.0) }.as_bool() {
                crate::logger::warning("placement", "could not remove taskbar event hook");
            }
        }
    }
}

unsafe extern "system" fn taskbar_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if !matches!(
        event,
        EVENT_OBJECT_CREATE
            | EVENT_OBJECT_DESTROY
            | EVENT_OBJECT_SHOW
            | EVENT_OBJECT_HIDE
            | EVENT_OBJECT_REORDER
            | EVENT_OBJECT_LOCATIONCHANGE
    ) || hwnd.is_invalid()
    {
        return;
    }
    let target = EVENT_TARGET.get();
    if target.controller == 0
        || target.taskbar == 0
        || target.posted
        || hwnd.0 as isize == target.widget
    {
        return;
    }
    let taskbar = HWND(target.taskbar as *mut _);
    // SAFETY: IsChild accepts window handles from event delivery and safely
    // returns false if a window has already been destroyed.
    if hwnd == taskbar || unsafe { IsChild(taskbar, hwnd) }.as_bool() {
        // SAFETY: Posting does not invoke application code or dereference state.
        if unsafe {
            PostMessageW(
                HWND(target.controller as *mut _),
                WM_TASKBAR_LAYOUT_CHANGED,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .is_ok()
        {
            EVENT_TARGET.set(EventTarget {
                posted: true,
                ..target
            });
        }
    }
}

/// Density changes require a stable candidate, independent of paint frequency.
#[derive(Default)]
pub struct DensitySettler {
    pending: Option<(DisplayLayout, Instant)>,
}

impl DensitySettler {
    pub fn reset(&mut self) {
        self.pending = None;
    }

    pub fn choose(
        &mut self,
        current: DisplayLayout,
        candidate: DisplayLayout,
        immediate: bool,
        now: Instant,
    ) -> DisplayLayout {
        if current == candidate || immediate {
            self.reset();
            return candidate;
        }
        match self.pending {
            Some((pending, since)) if pending == candidate => {
                if now.duration_since(since) >= Duration::from_secs(2) {
                    self.reset();
                    candidate
                } else {
                    current
                }
            }
            _ => {
                self.pending = Some((candidate, now));
                current
            }
        }
    }
}

/// Apply available positions immediately, but tolerate short-lived layout failures.
#[derive(Default)]
pub struct PlacementSettler {
    pending: Option<(Option<RECT>, Instant)>,
}

impl PlacementSettler {
    pub fn reset(&mut self) {
        self.pending = None;
    }

    pub fn ready(&mut self, current: Option<RECT>, target: Option<RECT>, now: Instant) -> bool {
        if current == target || target.is_some() {
            self.reset();
            return true;
        }
        match self.pending {
            Some((candidate, since)) if candidate == target => {
                // Keep brief provider failures from flashing the widget off.
                now.duration_since(since) >= Duration::from_secs(1)
            }
            _ => {
                self.pending = Some((target, now));
                false
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Request {
    taskbar: isize,
    widget: isize,
    rect: RECT,
}

struct Snapshot {
    request: Request,
    started: Instant,
    occupied: Result<Vec<RECT>, String>,
}

/// One worker, at most one outstanding scan, and no waits on the UI thread.
pub struct OccupancyProbe {
    events: TaskbarEvents,
    requests: SyncSender<Request>,
    results: Receiver<Snapshot>,
    pending: bool,
    last_request: Option<Instant>,
    snapshot: Option<Snapshot>,
    refresh: bool,
}

impl OccupancyProbe {
    pub fn new(controller: HWND) -> Self {
        let events = TaskbarEvents::new(controller);
        let controller = controller.0 as isize;
        let (requests, incoming) = mpsc::sync_channel::<Request>(1);
        let (outgoing, results) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("taskbar-layout".to_string())
            .spawn(move || {
                // COM objects are created, used and released on this thread.
                if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                    crate::logger::warning("placement", "could not initialize taskbar accessibility; widget will remain tray-only");
                    return;
                }
                {
                    let automation: windows::core::Result<IUIAutomation> =
                        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
                    if let Ok(automation) = automation {
                        let mut failed = false;
                        while let Ok(request) = incoming.recv() {
                            let started = Instant::now();
                            let occupied = scan(&automation, request).map_err(|err| err.to_string());
                            if let Err(err) = &occupied {
                                if !failed {
                                    crate::logger::warning("placement", &format!("taskbar layout unavailable: {err}; keeping tray icon only"));
                                }
                            } else if failed {
                                crate::logger::info("placement", "taskbar layout is available again");
                            }
                            failed = occupied.is_err();
                            if outgoing
                                .send(Snapshot {
                                    request,
                                    started,
                                    occupied,
                                })
                                .is_err()
                            {
                                break;
                            }
                            if controller != 0 {
                                // SAFETY: Only a notification is posted; the UI
                                // owns the window and drains the result channel.
                                if unsafe { PostMessageW(HWND(controller as *mut _), WM_TASKBAR_LAYOUT_READY, WPARAM(0), LPARAM(0)) }.is_err() {
                                    crate::logger::warning("placement", "could not notify controller of taskbar scan; using timer fallback");
                                }
                            }
                        }
                    } else {
                        crate::logger::warning("placement", "could not create taskbar accessibility client; widget will remain tray-only");
                    }
                }
                unsafe { CoUninitialize() };
            });
        if let Err(err) = spawned {
            crate::logger::warning(
                "placement",
                &format!("could not start taskbar layout worker: {err}"),
            );
        }
        Self {
            events,
            requests,
            results,
            pending: false,
            last_request: None,
            snapshot: None,
            refresh: false,
        }
    }

    /// Coalesce native event bursts, including changes arriving during a scan.
    pub fn invalidate(&mut self) {
        EVENT_TARGET.set(EventTarget {
            posted: false,
            ..EVENT_TARGET.get()
        });
        self.refresh = true;
    }

    /// Unknown, failed or stale layouts are not treated as empty space.
    pub fn occupied(&mut self, bar: HWND, widget: HWND, rect: RECT) -> Option<&[RECT]> {
        self.events.watch(bar, widget);
        let request = Request {
            taskbar: bar.0 as isize,
            widget: widget.0 as isize,
            rect,
        };
        while let Ok(snapshot) = self.results.try_recv() {
            self.pending = false;
            self.snapshot = Some(snapshot);
        }
        let changed = self.snapshot.as_ref().is_none_or(|s| s.request != request);
        let interval = if self.refresh {
            Duration::from_millis(u64::from(LAYOUT_REFRESH_MS))
        } else {
            Duration::from_secs(1)
        };
        if !self.pending && (changed || self.last_request.is_none_or(|t| t.elapsed() >= interval)) {
            if self.requests.try_send(request).is_ok() {
                self.pending = true;
                self.refresh = false;
            }
            self.last_request = Some(Instant::now());
        }
        self.snapshot
            .as_ref()
            .filter(|s| s.request == request && s.started.elapsed() < Duration::from_secs(3))?
            .occupied
            .as_deref()
            .ok()
    }
}

fn scan(automation: &IUIAutomation, request: Request) -> windows::core::Result<Vec<RECT>> {
    unsafe {
        let bar = HWND(request.taskbar as *mut _);
        let root = automation.ElementFromHandle(bar)?;
        let walker = automation.ControlViewWalker()?;
        let mut occupied = Vec::new();
        let mut budget = 2048;
        walk(&walker, &root, request, &mut occupied, &mut budget, 0)?;
        // No exposed controls means the provider is unavailable/unsupported,
        // not that the entire taskbar is safe to cover.
        if occupied.is_empty() {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "Taskbar exposes no occupied controls",
            ));
        }
        native_tools(bar, HWND(request.widget as *mut _), &mut occupied);
        if taskbar::taskbar_rect(bar) != Some(request.rect) {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "Taskbar moved during layout scan",
            ));
        }
        Ok(occupied)
    }
}

unsafe fn walk(
    walker: &IUIAutomationTreeWalker,
    parent: &IUIAutomationElement,
    request: Request,
    occupied: &mut Vec<RECT>,
    budget: &mut usize,
    depth: usize,
) -> windows::core::Result<()> {
    if depth > 32 || *budget == 0 {
        return Err(windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "Taskbar accessibility tree exceeds scan limits",
        ));
    }
    // UIA reports a null child/sibling as an error at the end of a branch.
    // SAFETY: The caller keeps the UIA walker and parent on the initialized COM
    // thread. Returned elements remain owned COM references during recursion.
    let mut child = optional_element(unsafe { walker.GetFirstChildElement(parent) })?;
    while let Some(element) = child {
        if *budget == 0 {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "Taskbar accessibility tree exceeds scan limits",
            ));
        }
        *budget -= 1;
        // SAFETY: element is a live UIA reference on this COM thread.
        let handle = unsafe { element.CurrentNativeWindowHandle() }?;
        if handle.0 as isize != request.widget
            // SAFETY: The same live element is queried without releasing it.
            && !unsafe { element.CurrentIsOffscreen() }?.as_bool()
        {
            // SAFETY: element remains a live UIA reference on this COM thread.
            let kind = unsafe { element.CurrentControlType() }?;
            // Container rectangles often span the whole taskbar. Only reserve
            // actual controls, not the XAML host or task-list background.
            if [
                UIA_ButtonControlTypeId,
                UIA_CheckBoxControlTypeId,
                UIA_RadioButtonControlTypeId,
                UIA_ListItemControlTypeId,
                UIA_MenuItemControlTypeId,
                UIA_HyperlinkControlTypeId,
                UIA_EditControlTypeId,
                UIA_SliderControlTypeId,
                UIA_TextControlTypeId,
                UIA_ImageControlTypeId,
                UIA_SplitButtonControlTypeId,
            ]
            .contains(&kind)
            {
                // SAFETY: element remains a live UIA reference on this COM thread.
                let rect = unsafe { element.CurrentBoundingRectangle() }?;
                if overlaps(rect, request.rect) {
                    occupied.push(rect);
                }
            }
            // SAFETY: Recursion uses the same COM thread and live references.
            unsafe { walk(walker, &element, request, occupied, budget, depth + 1) }?;
        }
        // SAFETY: walker and element are live on the initialized COM thread.
        child = optional_element(unsafe { walker.GetNextSiblingElement(&element) })?;
    }
    Ok(())
}

fn optional_element(
    result: windows::core::Result<IUIAutomationElement>,
) -> windows::core::Result<Option<IUIAutomationElement>> {
    match result {
        Ok(element) => Ok(Some(element)),
        // windows-rs 0.58 maps a successful null COM pointer to Error::empty()
        // (S_OK), not E_POINTER. Real provider failures must abort the scan.
        Err(err) if err.code() == windows::Win32::Foundation::S_OK => Ok(None),
        Err(err) => Err(err),
    }
}

/// Third-party child windows may not implement UIA. Reserve their full bounds,
/// but don't mistake Explorer's full-width host windows for occupied controls.
unsafe fn native_tools(bar: HWND, widget: HWND, occupied: &mut Vec<RECT>) {
    struct Search {
        bar: HWND,
        widget: HWND,
        rects: *mut Vec<RECT>,
    }
    unsafe extern "system" fn visit(
        hwnd: HWND,
        data: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::BOOL {
        // SAFETY: EnumChildWindows invokes this synchronously with the exclusive
        // Search pointer below. Its output vector lives for the whole scan.
        let search = unsafe { &mut *(data.0 as *mut Search) };
        if hwnd != search.widget
            // SAFETY: hwnd is supplied by the active window enumeration.
            && unsafe { GetParent(hwnd) }.is_ok_and(|parent| parent == search.bar)
            // SAFETY: Visibility is queried for the same enumerated window.
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
        {
            let class = taskbar::class_name(hwnd);
            if !matches!(
                class.as_str(),
                "ReBarWindow32"
                    | "MSTaskSwWClass"
                    | "MSTaskListWClass"
                    | "Windows.UI.Composition.DesktopWindowContentBridge"
                    | "Windows.UI.Input.InputSite.WindowClass"
                    | "WorkerW"
            ) {
                if let Some(rect) = taskbar::window_rect(hwnd) {
                    // SAFETY: The caller lends occupied exclusively throughout
                    // enumeration and no other reference accesses it here.
                    unsafe { (*search.rects).push(rect) };
                }
            }
        }
        windows::Win32::Foundation::BOOL(1)
    }
    let mut search = Search {
        bar,
        widget,
        rects: occupied,
    };
    // SAFETY: search and its exclusive output vector outlive this synchronous
    // enumeration; visit uses the matching system ABI and pointer type.
    unsafe {
        let _ = EnumChildWindows(
            bar,
            Some(visit),
            windows::Win32::Foundation::LPARAM(&mut search as *mut _ as isize),
        );
    }
}

fn overlaps(a: RECT, b: RECT) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

/// Choose the gap closest to the preferred coordinate. Tray placement keeps a
/// safe current position only while its anchor is blocked; left placement always
/// returns toward the left edge. Half-open occupied intervals allow an exact fit.
pub fn choose_x(
    bar: RECT,
    occupied: &[RECT],
    width: i32,
    preferred: i32,
    current: Option<i32>,
    margin: i32,
    placement: Placement,
) -> Option<i32> {
    if width <= 0 || bar.bottom - bar.top > bar.right - bar.left {
        return None; // Vertical taskbars are not supported by this widget.
    }
    let left = bar.left + margin;
    let right = bar.right - margin;
    let mut spans: Vec<_> = occupied
        .iter()
        .filter(|r| overlaps(**r, bar))
        .map(|r| ((r.left - margin).max(left), (r.right + margin).min(right)))
        .filter(|(l, r)| l < r)
        .collect();
    spans.sort_unstable();
    let mut gaps = Vec::new();
    let mut start = left;
    for (l, r) in spans {
        if l - start >= width {
            gaps.push((start, l - width));
        }
        start = start.max(r);
    }
    if right - start >= width {
        gaps.push((start, right - width));
    }
    if placement == Placement::Tray {
        if gaps.iter().any(|(l, r)| preferred >= *l && preferred <= *r) {
            return Some(preferred);
        }
        if let Some(x) = current.filter(|x| gaps.iter().any(|(l, r)| x >= l && x <= r)) {
            return Some(x);
        }
    }
    gaps.iter()
        .map(|(l, r)| preferred.clamp(*l, *r))
        .min_by_key(|x| (i64::from(*x) - i64::from(preferred)).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_ignores_transient_candidates_and_honors_explicit_changes() {
        use DisplayLayout::{Balanced, Compact, SingleLine};
        let mut settler = DensitySettler::default();
        let now = Instant::now();
        assert_eq!(settler.choose(Balanced, Compact, false, now), Balanced);
        assert_eq!(
            settler.choose(Balanced, Compact, false, now + Duration::from_secs(1)),
            Balanced
        );
        assert_eq!(
            settler.choose(Balanced, Balanced, false, now + Duration::from_secs(1)),
            Balanced
        );
        assert_eq!(
            settler.choose(Balanced, Compact, false, now + Duration::from_secs(2)),
            Balanced
        );
        assert_eq!(
            settler.choose(Balanced, Compact, false, now + Duration::from_secs(4)),
            Compact
        );
        assert_eq!(
            settler.choose(Compact, SingleLine, true, now + Duration::from_secs(4)),
            SingleLine
        );
    }
    fn rect(left: i32, right: i32) -> RECT {
        RECT {
            left,
            right,
            top: 1000,
            bottom: 1048,
        }
    }

    #[test]
    fn avoids_left_aligned_apps_and_other_tools() {
        assert_eq!(
            choose_x(
                rect(0, 1000),
                &[rect(0, 400), rect(900, 1000)],
                200,
                0,
                None,
                8,
                Placement::Left
            ),
            Some(408)
        );
    }

    #[test]
    fn positions_apply_immediately_but_transient_failures_do_not_hide_the_widget() {
        let mut settler = PlacementSettler::default();
        let now = Instant::now();
        let current = Some(rect(700, 900));
        let other = Some(rect(8, 208));
        assert!(settler.ready(None, current, now));
        assert!(settler.ready(current, other, now));
        assert!(!settler.ready(other, None, now));
        assert!(!settler.ready(other, None, now + Duration::from_millis(999)));
        // A usable position cancels the pending hide without a movement delay.
        assert!(settler.ready(other, current, now + Duration::from_secs(1)));
        assert!(!settler.ready(current, None, now + Duration::from_secs(2)));
        assert!(!settler.ready(current, None, now + Duration::from_millis(2999)));
        assert!(settler.ready(current, None, now + Duration::from_secs(3)));
    }

    #[test]
    fn left_returns_to_edge_even_when_current_position_is_safe() {
        let bar = rect(0, 1000);
        assert_eq!(
            choose_x(
                bar,
                &[rect(400, 700)],
                200,
                0,
                Some(100),
                8,
                Placement::Left
            ),
            Some(8)
        );
        assert_eq!(
            choose_x(
                bar,
                &[rect(250, 700)],
                200,
                0,
                Some(100),
                8,
                Placement::Left
            ),
            Some(8)
        );
    }

    #[test]
    fn left_reclaims_space_when_another_widget_shrinks_or_disappears() {
        for (offset, margin) in [(0, 8), (-1000, 16)] {
            let bar = rect(offset, offset + 1000);
            let mut current = None;
            for blocker_width in [300, 150, 0, 300] {
                let mut occupied = vec![rect(offset + 900, offset + 1000)];
                if blocker_width > 0 {
                    occupied.push(rect(offset, offset + blocker_width));
                }
                current = choose_x(
                    bar,
                    &occupied,
                    200,
                    bar.left,
                    current,
                    margin,
                    Placement::Left,
                );
                assert_eq!(current, Some(offset + blocker_width + margin));
            }
        }
    }

    #[test]
    fn left_skips_small_gaps_and_reclaims_space_without_a_movement_delay() {
        let bar = rect(0, 1000);
        let occupied = [rect(100, 300), rect(900, 1000)];
        let current_x = choose_x(bar, &occupied, 200, 0, Some(500), 8, Placement::Left);
        assert_eq!(current_x, Some(308));
        let current = current_x.map(|x| rect(x, x + 200));
        let target = choose_x(
            bar,
            &[rect(900, 1000)],
            200,
            0,
            current_x,
            8,
            Placement::Left,
        )
        .map(|x| rect(x, x + 200));
        assert_eq!(target, Some(rect(8, 208)));

        let now = Instant::now();
        let mut settler = PlacementSettler::default();
        assert!(settler.ready(current, target, now));
        assert!(settler.ready(target, current, now));
    }

    #[test]
    fn tray_reanchors_when_notification_area_shrinks_or_expands() {
        for (offset, margin) in [(0, 8), (-1000, 16)] {
            let bar = rect(offset, offset + 1000);
            let width = 200;
            let mut current = None;
            for tray_left in [800, 900, 750] {
                let anchor = offset + tray_left - width - margin;
                current = choose_x(
                    bar,
                    &[rect(offset + tray_left, offset + 1000)],
                    width,
                    anchor,
                    current,
                    margin,
                    Placement::Tray,
                );
                assert_eq!(current, Some(anchor));
            }
            assert_eq!(
                choose_x(
                    bar,
                    &[
                        rect(offset, offset + 600),
                        rect(offset + 750, offset + 1000)
                    ],
                    width,
                    offset + 750 - width - margin,
                    current,
                    margin,
                    Placement::Tray,
                ),
                None
            );
        }
    }

    #[test]
    fn tray_keeps_safe_fallback_until_anchor_blocker_clears() {
        let bar = rect(0, 1000);
        let tray = rect(900, 1000);
        let blocker = rect(750, 800);
        let anchor = 692;
        assert_eq!(
            choose_x(
                bar,
                &[tray, blocker],
                200,
                anchor,
                Some(100),
                8,
                Placement::Tray
            ),
            Some(100)
        );
        // An unsafe current position falls back to the nearest feasible gap.
        let fallback = choose_x(
            bar,
            &[tray, blocker],
            200,
            anchor,
            Some(anchor),
            8,
            Placement::Tray,
        );
        assert_eq!(fallback, Some(542));
        assert_eq!(
            choose_x(bar, &[tray], 200, anchor, fallback, 8, Placement::Tray),
            Some(anchor)
        );
    }

    #[test]
    fn tray_reanchoring_has_no_movement_delay() {
        let bar = rect(0, 1000);
        let current = Some(rect(592, 792));
        let now = Instant::now();
        let mut settler = PlacementSettler::default();
        let target = |tray_left| {
            choose_x(
                bar,
                &[rect(tray_left, 1000)],
                200,
                tray_left - 200 - 8,
                current.map(|r| r.left),
                8,
                Placement::Tray,
            )
            .map(|x| rect(x, x + 200))
        };
        let moved = target(900);
        assert_eq!(moved, Some(rect(692, 892)));
        assert!(settler.ready(current, moved, now));
        assert!(settler.ready(moved, target(800), now));
        assert!(settler.ready(moved, moved, now));
    }

    #[test]
    fn falls_back_to_other_gap_or_tray_only() {
        let bar = rect(0, 1000);
        assert_eq!(
            choose_x(bar, &[rect(100, 600)], 200, 0, None, 8, Placement::Left),
            Some(608)
        );
        assert_eq!(
            choose_x(bar, &[rect(100, 900)], 200, 0, None, 8, Placement::Left),
            None
        );
    }
    #[test]
    fn handles_negative_monitors_overlaps_and_exact_fit() {
        assert_eq!(
            choose_x(
                rect(-1000, 0),
                &[rect(-700, -400), rect(-500, -200)],
                284,
                -1000,
                None,
                8,
                Placement::Left
            ),
            Some(-992)
        );
        assert_eq!(
            choose_x(rect(0, 300), &[], 284, 0, None, 8, Placement::Left),
            Some(8)
        );
        assert_eq!(
            choose_x(rect(0, 300), &[], 285, 0, None, 8, Placement::Left),
            None
        );
    }
    #[test]
    fn ignores_controls_outside_taskbar() {
        let outside = RECT {
            top: 0,
            bottom: 40,
            ..rect(0, 1000)
        };
        assert_eq!(
            choose_x(
                rect(0, 1000),
                &[outside],
                200,
                800,
                None,
                8,
                Placement::Left
            ),
            Some(792)
        );
    }

    #[test]
    fn right_preference_and_space_reappearing() {
        let bar = rect(0, 1000);
        assert_eq!(
            choose_x(
                bar,
                &[rect(0, 300), rect(900, 1000)],
                200,
                1000,
                None,
                8,
                Placement::Tray
            ),
            Some(692)
        );
        assert_eq!(
            choose_x(bar, &[rect(0, 1000)], 200, 0, None, 8, Placement::Left),
            None
        );
        assert_eq!(
            choose_x(bar, &[rect(0, 400)], 200, 0, None, 8, Placement::Left),
            Some(408)
        );
        let vertical = RECT {
            left: 0,
            right: 48,
            top: 0,
            bottom: 1000,
        };
        assert_eq!(
            choose_x(vertical, &[], 200, 0, None, 8, Placement::Left),
            None
        );
    }

    #[test]
    fn null_tree_nodes_are_distinct_from_provider_failures() {
        assert!(optional_element(Err(windows::core::Error::empty()))
            .unwrap()
            .is_none());
        assert!(optional_element(Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_POINTER
        )))
        .is_err());
    }

    #[test]
    fn probe_rejects_failed_stale_and_wrong_taskbar_snapshots_then_recovers() {
        let (requests, _incoming) = mpsc::sync_channel(1);
        let (outgoing, results) = mpsc::channel();
        let request = Request {
            taskbar: 1,
            widget: 2,
            rect: rect(0, 1000),
        };
        let bar = HWND(std::ptr::without_provenance_mut(1));
        let widget = HWND(std::ptr::without_provenance_mut(2));
        let mut probe = OccupancyProbe {
            events: TaskbarEvents::new(HWND::default()),
            requests,
            results,
            pending: false,
            last_request: Some(Instant::now()),
            snapshot: None,
            refresh: false,
        };
        let snapshot = |request, started, occupied| Snapshot {
            request,
            started,
            occupied,
        };
        outgoing
            .send(snapshot(
                request,
                Instant::now(),
                Err("provider failed".to_string()),
            ))
            .unwrap();
        assert!(probe.occupied(bar, widget, request.rect).is_none());
        outgoing
            .send(snapshot(
                request,
                Instant::now() - Duration::from_secs(4),
                Ok(vec![rect(0, 100)]),
            ))
            .unwrap();
        assert!(probe.occupied(bar, widget, request.rect).is_none());
        outgoing
            .send(snapshot(
                Request {
                    taskbar: 3,
                    ..request
                },
                Instant::now(),
                Ok(vec![rect(0, 100)]),
            ))
            .unwrap();
        assert!(probe.occupied(bar, widget, request.rect).is_none());
        outgoing
            .send(snapshot(request, Instant::now(), Ok(vec![rect(0, 100)])))
            .unwrap();
        assert_eq!(
            probe.occupied(bar, widget, request.rect),
            Some([rect(0, 100)].as_slice())
        );
        assert!(probe.occupied(bar, widget, rect(-1000, 0)).is_none());
    }

    #[test]
    fn taskbar_events_filter_unrelated_windows_and_coalesce_bursts() {
        use windows::core::w;
        use windows::Win32::Foundation::HINSTANCE;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, PeekMessageW, HMENU, MSG, PM_REMOVE, WINDOW_EX_STYLE,
            WS_CHILD, WS_POPUP,
        };

        struct TestWindow(HWND);
        impl TestWindow {
            fn new(parent: HWND) -> Self {
                // SAFETY: STATIC is a built-in window class. Test windows are
                // hidden, owned by this thread, and destroyed below.
                Self(unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        w!("STATIC"),
                        w!("placement test"),
                        if parent.is_invalid() {
                            WS_POPUP
                        } else {
                            WS_CHILD
                        },
                        0,
                        0,
                        100,
                        40,
                        parent,
                        HMENU::default(),
                        HINSTANCE::default(),
                        None,
                    )
                    .unwrap()
                })
            }
        }
        impl Drop for TestWindow {
            fn drop(&mut self) {
                // SAFETY: This value owns a window on the current test thread.
                unsafe { DestroyWindow(self.0) }.unwrap();
            }
        }
        let controller = TestWindow::new(HWND::default());
        let bar = TestWindow::new(HWND::default());
        let widget = TestWindow::new(bar.0);
        let other = TestWindow::new(bar.0);
        let unrelated = TestWindow::new(HWND::default());
        let events = TaskbarEvents::new(controller.0);
        assert!(!events.0.is_invalid());
        events.watch(bar.0, widget.0);
        let notify = |hwnd, event| {
            // SAFETY: Exercise the callback on its installing thread, using
            // only test-owned handles and no application state.
            unsafe { taskbar_event(events.0, event, hwnd, 0, 0, 0, 0) };
        };
        let take_message = || {
            let mut msg = MSG::default();
            // SAFETY: Only remove our notification from the test window's queue.
            unsafe {
                PeekMessageW(
                    &mut msg,
                    controller.0,
                    WM_TASKBAR_LAYOUT_CHANGED,
                    WM_TASKBAR_LAYOUT_CHANGED,
                    PM_REMOVE,
                )
            }
            .as_bool()
        };
        notify(widget.0, EVENT_OBJECT_LOCATIONCHANGE);
        notify(unrelated.0, EVENT_OBJECT_HIDE);
        assert!(!take_message());
        for event in [
            EVENT_OBJECT_HIDE,
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_LOCATIONCHANGE,
        ] {
            notify(other.0, event);
        }
        assert!(take_message());
        assert!(!take_message());
        EVENT_TARGET.set(EventTarget {
            posted: false,
            ..EVENT_TARGET.get()
        });
        notify(bar.0, EVENT_OBJECT_REORDER);
        assert!(take_message());
        drop(events);
        assert_eq!(EVENT_TARGET.get().controller, 0);
    }

    #[test]
    fn event_refresh_is_throttled_and_survives_an_inflight_scan() {
        let (requests, incoming) = mpsc::sync_channel(1);
        let (outgoing, results) = mpsc::channel();
        let request = Request {
            taskbar: 1,
            widget: 2,
            rect: rect(0, 1000),
        };
        let snapshot = || Snapshot {
            request,
            started: Instant::now(),
            occupied: Ok(vec![rect(900, 1000)]),
        };
        let mut probe = OccupancyProbe {
            events: TaskbarEvents::new(HWND::default()),
            requests,
            results,
            pending: false,
            last_request: Some(Instant::now()),
            snapshot: Some(snapshot()),
            refresh: false,
        };
        let bar = HWND(std::ptr::without_provenance_mut(1));
        let widget = HWND(std::ptr::without_provenance_mut(2));
        probe.invalidate();
        probe.occupied(bar, widget, request.rect);
        assert!(incoming.try_recv().is_err()); // Rate-limited, not forgotten.
        assert!(probe.refresh);
        probe.last_request = Some(Instant::now() - Duration::from_millis(150));
        probe.occupied(bar, widget, request.rect);
        assert_eq!(incoming.try_recv().unwrap(), request);
        assert!(probe.pending);
        assert!(!probe.refresh);

        probe.invalidate(); // A second event arrives before the scan finishes.
        probe.last_request = Some(Instant::now() - Duration::from_millis(150));
        probe.occupied(bar, widget, request.rect);
        assert!(incoming.try_recv().is_err());
        outgoing.send(snapshot()).unwrap();
        probe.occupied(bar, widget, request.rect);
        assert_eq!(incoming.try_recv().unwrap(), request);
        assert!(!probe.refresh);

        outgoing.send(snapshot()).unwrap();
        probe.occupied(bar, widget, request.rect);
        assert!(incoming.try_recv().is_err()); // Idle fallback is not high-frequency.
    }

    #[test]
    #[ignore = "read-only smoke test requiring a running Explorer taskbar"]
    fn live_taskbar_occupancy() {
        let bar = taskbar::find_taskbars()
            .into_iter()
            .next()
            .expect("Explorer taskbar");
        let mut probe = OccupancyProbe::new(HWND::default());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(rects) = probe.occupied(bar.hwnd, HWND(-1_isize as *mut _), bar.rect) {
                eprintln!("Taskbar {:?}; occupied: {rects:?}", bar.rect);
                assert!(!rects.is_empty());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no usable occupancy snapshot: {:?}",
                probe.snapshot.as_ref().map(|s| &s.occupied)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
