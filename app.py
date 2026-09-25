"""NetSpeed Widget - a small always-on-top network monitor for Windows.

The UI is a single borderless "pill" drawn on one canvas with a
transparent-color window, which gives real rounded corners. All UI work
runs on the Tk main thread. Throughput sampling and latency probing run
on daemon threads and publish snapshots that the UI polls through
`root.after`. The saved window position is validated against the
currently connected monitors so the widget can never be stranded on a
screen that is no longer attached.

Work with its own inputs and outputs lives beside this module:
`utils.monitors` for display geometry, `utils.graph` for the traffic plot,
`utils.schedule` for when the next speedtest is due and
`utils.hotkey_dialog` for the capture window.
"""

import threading
import time
import tkinter as tk
import traceback
from collections.abc import Callable
from tkinter import font as tkfont
from tkinter import messagebox
from typing import Any

import win32api
import win32con

from tray.container import TrayController
from utils import hotkey_dialog, monitors
from utils.config import (
    OPACITY_LEVELS,
    clamp_opacity,
    get_hide_on_hover,
    get_hotkey,
    get_opacity,
    get_position,
    get_speedtest,
    set_hide_on_hover,
    set_hotkey,
    set_position,
    set_speedtest,
)
from utils.config import (
    set_opacity as config_set_opacity,
)
from utils.format import format_speed
from utils.graph import TrafficGraph
from utils.hotkeys import GlobalHotkey, format_hotkey
from utils.latency import LatencyProbe
from utils.logger import get, section, startup
from utils.paths import icon_path
from utils.sampler import NetSample, NetSampler
from utils.schedule import next_speedtest_due
from utils.speedtest import measure_speed
from utils.theme import (
    BAD_COLOR,
    BORDER,
    BORDER_HOVER,
    DOWN_COLOR,
    FG,
    FG_DIM,
    FONT_FAMILY,
    GOOD_COLOR,
    SURFACE,
    TRANSPARENT,
    UP_COLOR,
    WARN_COLOR,
    rounded_rect,
    status_download_color,
    status_upload_color,
)
from utils.version import APP_NAME

_log = get("app")
_net_log = get("sampler")

UI_TICK_MS = 250
GRAPH_SAMPLES = 60
CORNER_MARGIN = 12
MONITOR_CHECK_TICKS = 16
TICK_REPORT_EVERY = 40
SPEEDTEST_INTERVAL_SEC = 4 * 60 * 60
SPEEDTEST_STARTUP_GRACE_SEC = 20
SPEEDTEST_RETRY_SEC = 15 * 60

# Pill geometry (fixed layout: two text rows plus a large graph)
PILL_W = 340
PILL_H = 52
PILL_R = 16
DOT_X, DOT_R = 15, 3.5
ROW1_Y = 16
ROW2_Y = 38
DOWN_ARROW_X, DOWN_VAL_X = 24, 36
UP_ARROW_X, UP_VAL_X = 74, 86
UNIT_X = 122
PING_X = 148
ST_DOWN_X, ST_UP_X, ST_UNIT_X = 24, 66, 108
GRAPH_X0, GRAPH_W = 190, 142
GRAPH_TOP, GRAPH_BOTTOM = 6, PILL_H - 6

HOVER_POLL_MS = 120
# Escape hatch: a cursor parked on the pill keeps it hidden by design, but a
# stuck cursor query must not hide the widget for the rest of the run.
HOVER_POLL_LIMIT = 2500


def _mouse_button_held() -> bool:
    """True while either mouse button is physically down.

    A right-click taken during a drag steals mouse capture, so the left
    release is never delivered to us and the drag state has to be resolved
    from the button itself rather than from an event. Assuming "held" on a
    failed query only delays the repair; guessing "released" would cut a
    legitimate drag short.
    """
    try:
        state = win32api.GetAsyncKeyState(win32con.VK_LBUTTON)
        state |= win32api.GetAsyncKeyState(win32con.VK_RBUTTON)
    except Exception as err:  # noqa: BLE001 - pywin32 raises its own error type
        _log.warning(f"could not read the mouse button state: {err}")
        return True
    return bool(state & 0x8000)


class NetSpeedWidget:
    """Floating widget for live network speed, latency and history.

    The widget owns the Tk window and every interaction. Background work
    (sampling, latency probing and speedtests) lives in dedicated helpers
    and reaches the UI only through the `root.after` tick loop.
    """

    def __init__(self, root: tk.Tk) -> None:
        """Build the UI, start background services and the tick loop."""
        self.root = root
        self._run = True
        self._stop_event = threading.Event()
        self._rounded = False

        self._configure_window()
        self._build_ui()
        self._place_window()

        self.opacity = get_opacity()
        self._apply_alpha()

        self.hotkey = GlobalHotkey(lambda: self.ui_call(self.toggle_window))
        self._hotkey_combo = get_hotkey()
        ok, err = self.hotkey.apply_combo(self._hotkey_combo)
        self._hotkey_ok = ok
        if not ok:
            _log.warning(f"'{self._hotkey_combo}' unavailable at startup: {err}")

        self.sampler = NetSampler(interval=1.0, history=GRAPH_SAMPLES)
        self.probe = LatencyProbe()
        self.sampler.start()
        self.probe.start()

        self._last_sample_ts = 0.0
        self._smooth_down = 0.0
        self._smooth_up = 0.0
        self._max_up_seen = 0.0
        self._max_down_seen = 0.0
        self._tick_count = 0
        self._tick_failures = 0
        self._status_color = GOOD_COLOR
        self._hover_guard_active = False
        self._dragging = False
        self._drag_offset = (0, 0)
        self._speedtest_summary = ""
        self.tray: TrayController | None = None

        self._speedtest_running = False
        self._speedtest_next_due = self._compute_next_speedtest_due()
        self._load_saved_speedtest()
        threading.Thread(
            target=self._speedtest_scheduler_loop,
            name="speedtest-scheduler",
            daemon=True,
        ).start()

        self._bind_input()
        self.root.bind("<Enter>", self._on_mouse_enter)
        self.root.bind("<Leave>", self._on_mouse_leave)
        self.root.protocol("WM_DELETE_WINDOW", self.shutdown)

        startup(APP_NAME)
        _log.info(
            f"initialized at x={self.win_x} y={self.win_y} "
            f"size={self.win_width}x{self.win_height} opacity={self.opacity:.2f}"
        )

        self.root.after(UI_TICK_MS, self._tick)

    # ---------- Window and layout ----------

    def _configure_window(self) -> None:
        """Set window flags: borderless, always on top, transparent key."""
        self.root.withdraw()
        self.root.overrideredirect(True)
        self.root.attributes("-topmost", True)
        try:
            self.root.attributes("-toolwindow", True)
        except tk.TclError:
            pass  # documented contract: unsupported on some platforms
        self.root.title(APP_NAME)
        self._apply_icon()
        self.root.configure(bg=TRANSPARENT)
        try:
            self.root.attributes("-transparentcolor", TRANSPARENT)
            self._rounded = True
        except tk.TclError:
            _log.warning("transparency key unsupported; using flat background")

    def _apply_icon(self) -> None:
        """Set the window icon, falling back to the default when unusable.

        The icon is decoration, so a missing or unreadable file must not
        stop the widget from starting. The tray already treats the same file
        as optional.
        """
        try:
            self.root.iconbitmap(str(icon_path()))
        except tk.TclError as err:
            _log.warning(f"window icon unavailable, using the default: {err}")

    def _build_ui(self) -> None:
        """Draw the pill, texts and graph slots on a single canvas."""
        self.font_value = tkfont.Font(family=FONT_FAMILY, size=10, weight="bold")
        self.font_arrow = tkfont.Font(family=FONT_FAMILY, size=9)
        self.font_unit = tkfont.Font(family=FONT_FAMILY, size=6)
        self.font_ping = tkfont.Font(family=FONT_FAMILY, size=8, weight="bold")
        self.font_mini = tkfont.Font(family=FONT_FAMILY, size=7)

        canvas_bg = TRANSPARENT if self._rounded else SURFACE
        self.canvas = tk.Canvas(
            self.root,
            width=PILL_W,
            height=PILL_H,
            bg=canvas_bg,
            highlightthickness=0,
            bd=0,
        )
        self.canvas.pack(fill="both", expand=True)

        if self._rounded:
            self._pill = rounded_rect(
                self.canvas,
                1,
                1,
                PILL_W - 1,
                PILL_H - 1,
                PILL_R,
                fill=SURFACE,
                outline=BORDER,
                width=1,
            )
        else:
            self._pill = self.canvas.create_rectangle(
                0, 0, PILL_W, PILL_H, fill=SURFACE, outline=BORDER, width=1
            )

        center_y = PILL_H / 2
        self._dot_item = self.canvas.create_oval(
            DOT_X - DOT_R,
            center_y - DOT_R,
            DOT_X + DOT_R,
            center_y + DOT_R,
            fill=FG_DIM,
            outline="",
        )

        # Row 1: live speeds and latency
        self._t_down_arrow = self.canvas.create_text(
            DOWN_ARROW_X,
            ROW1_Y,
            text="↓",
            font=self.font_arrow,
            fill=DOWN_COLOR,
            anchor="w",
        )
        self._t_down_val = self.canvas.create_text(
            DOWN_VAL_X,
            ROW1_Y,
            text="0.00",
            font=self.font_value,
            fill=DOWN_COLOR,
            anchor="w",
        )
        self._t_up_arrow = self.canvas.create_text(
            UP_ARROW_X,
            ROW1_Y,
            text="↑",
            font=self.font_arrow,
            fill=UP_COLOR,
            anchor="w",
        )
        self._t_up_val = self.canvas.create_text(
            UP_VAL_X,
            ROW1_Y,
            text="0.00",
            font=self.font_value,
            fill=UP_COLOR,
            anchor="w",
        )
        self._t_unit = self.canvas.create_text(
            UNIT_X,
            ROW1_Y + 3,
            text="Mb/s",
            font=self.font_unit,
            fill=FG_DIM,
            anchor="w",
        )
        self._t_ping = self.canvas.create_text(
            PING_X,
            ROW1_Y,
            text="-- ms",
            font=self.font_ping,
            fill=FG_DIM,
            anchor="w",
        )

        # Row 2: last speedtest result, dimmer
        self._t_st_down = self.canvas.create_text(
            ST_DOWN_X,
            ROW2_Y,
            text="↓ --",
            font=self.font_mini,
            fill=DOWN_COLOR,
            anchor="w",
        )
        self._t_st_up = self.canvas.create_text(
            ST_UP_X,
            ROW2_Y,
            text="↑ --",
            font=self.font_mini,
            fill=UP_COLOR,
            anchor="w",
        )
        self._t_st_unit = self.canvas.create_text(
            ST_UNIT_X,
            ROW2_Y,
            text="Mb/s speedtest",
            font=self.font_unit,
            fill=FG_DIM,
            anchor="w",
        )

        self._graph = TrafficGraph(
            canvas=self.canvas,
            x0=GRAPH_X0,
            width=GRAPH_W,
            top=GRAPH_TOP,
            bottom=GRAPH_BOTTOM,
            capacity=GRAPH_SAMPLES,
        )
        self.menu = self._build_menu()

    def _place_window(self) -> None:
        """Size the window, then restore the saved or default position.

        A saved position that no longer sits on any connected monitor is
        discarded, so the widget never ends up invisible on a screen that
        has been detached or disabled.
        """
        self.win_width = PILL_W
        self.win_height = PILL_H
        position = get_position()
        if position is None:
            position = self._default_position()
        elif not self._window_on_active_monitor(*position):
            _log.warning("saved position is off all active monitors; using the primary")
            position = self._default_position()
            set_position(*position)
        self.win_x, self.win_y = position
        self.root.geometry(
            f"{self.win_width}x{self.win_height}+{self.win_x}+{self.win_y}"
        )
        self.root.deiconify()

    def _default_position(self) -> tuple[int, int]:
        """Bottom-right corner of the primary monitor work area."""
        width, height = self.win_width, self.win_height
        return monitors.default_position(width, height, CORNER_MARGIN)

    def _window_on_active_monitor(self, x: int, y: int) -> bool:
        """True when the window center is inside any connected monitor."""
        return monitors.point_on_active_monitor(x, y, self.win_width, self.win_height)

    def _ensure_visible(self) -> None:
        """Snap back to the primary screen if stranded off-monitor.

        Covers display changes while the app is running, for example
        closing the laptop lid so only the external screen stays on.
        """
        if self._dragging or self._hover_guard_active:
            return
        if self._window_on_active_monitor(self.win_x, self.win_y):
            return
        _log.info("display layout changed; repositioning to the primary screen")
        self.win_x, self.win_y = self._default_position()
        self.root.geometry(f"+{self.win_x}+{self.win_y}")
        set_position(self.win_x, self.win_y)

    def reset_position(self) -> None:
        """Move the widget back to the default corner and persist it."""
        self.win_x, self.win_y = self._default_position()
        self.root.geometry(f"+{self.win_x}+{self.win_y}")
        set_position(self.win_x, self.win_y)
        _log.info("position reset to the default corner")

    # ---------- Input ----------

    def _bind_input(self) -> None:
        """Bind drag, menu and speedtest gestures on the canvas."""
        canvas = self.canvas
        canvas.bind("<ButtonPress-1>", self._start_drag)
        canvas.bind("<B1-Motion>", self._on_drag)
        canvas.bind("<ButtonRelease-1>", self._end_drag)
        canvas.bind("<Double-Button-1>", lambda _e: self.run_speedtest_now(manual=True))
        canvas.bind("<Button-3>", self._popup_menu)

    def _start_drag(self, event: tk.Event) -> None:
        """Remember the grab offset at drag start."""
        self._dragging = True
        self._drag_offset = (
            event.x_root - self.root.winfo_x(),
            event.y_root - self.root.winfo_y(),
        )

    def _on_drag(self, event: tk.Event) -> None:
        """Move the window with the cursor."""
        self.win_x = event.x_root - self._drag_offset[0]
        self.win_y = event.y_root - self._drag_offset[1]
        self.root.geometry(f"+{self.win_x}+{self.win_y}")

    def _end_drag(self, _event: Any = None) -> None:
        """Persist the dropped position."""
        self._dragging = False
        set_position(self.win_x, self.win_y)

    def _heal_stuck_drag(self) -> None:
        """Close a drag whose release event was taken by the context menu.

        Posting the Tk menu grabs mouse capture, so a right-click pressed
        before the left button comes up swallows `<ButtonRelease-1>` and
        `_dragging` would stay set for the rest of the run, disabling the
        off-monitor rescue along with it.
        """
        if self._dragging and not _mouse_button_held():
            _log.warning("drag release never arrived; ending the stuck drag")
            self._end_drag()

    def _build_menu(self) -> tk.Menu:
        """Create the right-click context menu."""
        menu = tk.Menu(
            self.root,
            tearoff=0,
            bg=SURFACE,
            fg=FG,
            activebackground=BORDER,
            activeforeground=FG,
            disabledforeground=FG_DIM,
            relief="flat",
            borderwidth=1,
            font=(FONT_FAMILY, 9),
        )
        menu.add_command(
            label="Run speedtest",
            command=lambda: self.run_speedtest_now(manual=True),
        )
        menu.add_command(label="Session: --", state="disabled")
        self._menu_session_index = menu.index("end")
        menu.add_command(label="Last speedtest: --", state="disabled")
        self._menu_speedtest_index = menu.index("end")

        opacity_menu = tk.Menu(
            menu,
            tearoff=0,
            bg=SURFACE,
            fg=FG,
            activebackground=BORDER,
            activeforeground=FG,
            font=(FONT_FAMILY, 9),
        )
        for level in OPACITY_LEVELS:
            opacity_menu.add_command(
                label=f"{int(level * 100)}%",
                command=lambda lvl=level: self.set_opacity(lvl),
            )
        menu.add_cascade(label="Opacity", menu=opacity_menu)

        self._hover_var = tk.BooleanVar(value=get_hide_on_hover())
        menu.add_checkbutton(
            label="Auto-hide on hover",
            variable=self._hover_var,
            command=self._toggle_hover_hide,
        )
        menu.add_command(label="Change hotkey...", command=self.open_hotkey_dialog)
        self._menu_hotkey_index = menu.index("end")
        menu.add_separator()
        menu.add_command(label="Reset position", command=self.reset_position)
        menu.add_command(label="Hide", command=self.hide_window)
        menu.add_command(label="Quit", command=self.shutdown)
        return menu

    def _popup_menu(self, event: tk.Event) -> None:
        """Refresh dynamic entries, then show the context menu."""
        down_mb, up_mb = self.sampler.session_totals
        self.menu.entryconfig(
            self._menu_session_index,
            label=f"Session: {down_mb:.1f} MB down, {up_mb:.1f} MB up",
        )
        self.menu.entryconfig(
            self._menu_speedtest_index,
            label=self._speedtest_summary or "Last speedtest: --",
        )
        if self._hotkey_ok:
            hotkey_label = f"Change hotkey ({format_hotkey(self._hotkey_combo)})"
        else:
            hotkey_label = "Set hotkey (none active)"
        self.menu.entryconfig(self._menu_hotkey_index, label=hotkey_label)
        self._hover_var.set(get_hide_on_hover())
        self.menu.post(event.x_root, event.y_root)

    def _toggle_hover_hide(self) -> None:
        """Persist the auto-hide checkbox state."""
        set_hide_on_hover(self._hover_var.get())

    def toggle_hover_hide(self) -> None:
        """Flip the auto-hide setting. Called from the tray menu."""
        enabled = not get_hide_on_hover()
        set_hide_on_hover(enabled)
        self._hover_var.set(enabled)
        _log.info(f"auto-hide on hover: {enabled}")

    # ---------- Tick loop and rendering ----------

    def _tick(self) -> None:
        """UI refresh loop. Runs on the Tk main thread via `after`.

        A failing body is logged and the next tick is still scheduled
        from `finally`, so one bad render can never freeze the UI
        silently while the sampler threads keep running.
        """
        if not self._run:
            return
        try:
            self._heal_stuck_drag()
            sample = self.sampler.latest
            if sample is not None and sample.ts != self._last_sample_ts:
                self._last_sample_ts = sample.ts
                self._render_sample(sample)
            self._render_latency()
            self._tick_count += 1
            if self._tick_count % 8 == 0:
                self._push_tray_status()
            if self._tick_count % MONITOR_CHECK_TICKS == 0:
                self._ensure_visible()
        except Exception as err:  # noqa: BLE001 - a render failure must not stick
            self._report_tick_failure(err)
        finally:
            self.root.after(UI_TICK_MS, self._tick)

    def _report_tick_failure(self, err: Exception) -> None:
        """Log a failing render once, then every TICK_REPORT_EVERY repeats.

        A render that fails every tick would otherwise write four lines a
        second to a file that is also the app's only diagnostic surface.
        """
        self._tick_failures += 1
        if self._tick_failures == 1 or self._tick_failures % TICK_REPORT_EVERY == 0:
            _log.error(
                f"UI tick failed {self._tick_failures} times in a row: {err}\n"
                f"{traceback.format_exc(limit=3)}"
            )

    def _render_sample(self, sample: NetSample) -> None:
        """Update texts, peak logging and the graph for one sample."""
        self._smooth_down += (sample.down_mbps - self._smooth_down) * 0.45
        self._smooth_up += (sample.up_mbps - self._smooth_up) * 0.45
        self.canvas.itemconfig(self._t_down_val, text=format_speed(self._smooth_down))
        self.canvas.itemconfig(self._t_up_val, text=format_speed(self._smooth_up))

        if sample.up_mbps > self._max_up_seen and sample.up_mbps >= 1.0:
            self._max_up_seen = sample.up_mbps
            _net_log.info(f"new upstream peak {sample.up_mbps:.2f} Mb/s")
        if sample.down_mbps > self._max_down_seen and sample.down_mbps >= 1.0:
            self._max_down_seen = sample.down_mbps
            _net_log.info(f"new downstream peak {sample.down_mbps:.2f} Mb/s")

        self._graph.draw(self.sampler.history, self._status_color)

    def _render_latency(self) -> None:
        """Refresh the latency text and status dot."""
        if self._speedtest_running:
            self.canvas.itemconfig(self._t_ping, text="test...", fill=FG_DIM)
            return
        result = self.probe.latest
        if not result.ok:
            self.canvas.itemconfig(self._t_ping, text="offline", fill=BAD_COLOR)
            self._set_status(BAD_COLOR)
        elif result.ms is None:
            self.canvas.itemconfig(self._t_ping, text="-- ms", fill=FG_DIM)
            self._set_status(WARN_COLOR)
        else:
            ms = result.ms
            if ms < 80:
                color = GOOD_COLOR
            elif ms < 180:
                color = WARN_COLOR
            else:
                color = BAD_COLOR
            self.canvas.itemconfig(self._t_ping, text=f"{ms:.0f} ms", fill=color)
            self._set_status(color)

    def _set_status(self, color: str) -> None:
        """Recolor status-dependent UI and redraw the graph for `color`."""
        if color != self._status_color:
            self._status_color = color
            download_color = status_download_color(color)

            self.canvas.itemconfig(self._t_down_arrow, fill=download_color)
            self.canvas.itemconfig(self._t_down_val, fill=download_color)
            self.canvas.itemconfig(self._t_st_down, fill=download_color)
            self.canvas.itemconfig(self._dot_item, fill=color)
            upload_color = status_upload_color(color)
            self.canvas.itemconfig(self._t_up_arrow, fill=upload_color)
            self.canvas.itemconfig(self._t_up_val, fill=upload_color)
            self.canvas.itemconfig(self._t_st_up, fill=upload_color)
            self._graph.draw(self.sampler.history, color)

    def _push_tray_status(self) -> None:
        """Update the tray tooltip with current speeds and latency."""
        tray = self.tray
        if tray is None:
            return
        result = self.probe.latest
        if not result.ok:
            ping_text = "offline"
        elif result.ms is None:
            ping_text = "-- ms"
        else:
            ping_text = f"{result.ms:.0f} ms"
        tray.update_live_status(
            f"D {format_speed(self._smooth_down)} Mb/s | "
            f"U {format_speed(self._smooth_up)} Mb/s | {ping_text}"
        )

    # ---------- Hover ----------

    def _on_mouse_enter(self, _event: Any = None) -> None:
        """Highlight the pill border; hide on hover if enabled.

        A drag in progress is never interrupted by hiding, because the
        withdraw would break the mouse grab the drag depends on.
        """
        self.canvas.itemconfig(self._pill, outline=BORDER_HOVER)
        blocked = self._hover_guard_active or self._dragging
        if blocked or not get_hide_on_hover():
            return
        self._hover_guard_active = True
        _log.info("hover hide")
        self.root.withdraw()
        self._poll_cursor_and_restore()

    def _on_mouse_leave(self, _event: Any = None) -> None:
        """Restore the default pill border."""
        self.canvas.itemconfig(self._pill, outline=BORDER)

    def _poll_cursor_and_restore(self, attempts: int = 0) -> None:
        """Restore the window once the cursor leaves its bounds.

        The guard is released before the restore is attempted: a failed
        cursor query on a locked desktop would otherwise strand the flag,
        killing auto-hide and the off-monitor rescue for the rest of the run.
        """
        if not self._hover_guard_active:
            return
        inside = self._cursor_inside_pill()
        if inside and attempts < HOVER_POLL_LIMIT:
            self.root.after(
                HOVER_POLL_MS, lambda: self._poll_cursor_and_restore(attempts + 1)
            )
            return
        if inside:
            _log.warning(f"hover hide stuck after {attempts} polls; forcing restore")
        self._hover_guard_active = False
        self._restore_after_hover()

    def _cursor_inside_pill(self) -> bool:
        """Whether the physical cursor is over the pill, False when unsure."""
        try:
            x, y = win32api.GetCursorPos()
        except Exception as err:  # noqa: BLE001 - a secure desktop blocks queries
            _log.warning(f"cursor query failed during hover hide: {err}")
            return False
        return (
            self.win_x <= x <= self.win_x + self.win_width
            and self.win_y <= y <= self.win_y + self.win_height
        )

    def _restore_after_hover(self) -> None:
        """Bring the window back and drop the hover highlight."""
        try:
            self.root.deiconify()
            self.canvas.itemconfig(self._pill, outline=BORDER)
        except tk.TclError as err:
            _log.warning(f"hover restore could not redraw the window: {err}")
            return
        _log.info("hover restore")

    # ---------- Lifecycle ----------

    def ui_call(self, func: Callable[..., None], *args: Any, **kwargs: Any) -> None:
        """Schedule a callable on the Tk main thread. Safe from any thread.

        Calls that land after the window is destroyed are dropped; that
        is the normal shutdown race for background workers, not an
        error worth reporting. Cross-thread calls after mainloop exit
        raise RuntimeError, not TclError, so both are caught.
        """
        try:
            self.root.after(0, lambda: func(*args, **kwargs))
        except (tk.TclError, RuntimeError):
            pass  # documented contract: Tk gone or mainloop exited, drop call

    def show_window(self) -> None:
        """Show the widget and keep it on top."""
        _log.info("show window")
        self._hover_guard_active = False
        self.root.deiconify()
        self.root.attributes("-topmost", True)

    def hide_window(self) -> None:
        """Hide the widget."""
        _log.info("hide window")
        self._hover_guard_active = False
        self.root.withdraw()

    def toggle_window(self) -> None:
        """Show the widget when hidden, hide it when shown."""
        if self.root.state() == "withdrawn":
            self.show_window()
        else:
            self.hide_window()

    def open_hotkey_dialog(self) -> None:
        """Open a capture dialog that listens for the next combo."""
        dialog = getattr(self, "_hotkey_dialog", None)
        if dialog is not None and dialog.winfo_exists():
            dialog.lift()
            dialog.focus_force()
            return
        self._hotkey_dialog = hotkey_dialog.show(
            self.root, self._hotkey_combo, self._apply_hotkey
        )

    def _apply_hotkey(self, combo: str) -> tuple[bool, str | None]:
        """Register `combo` globally and keep config in step with the result.

        Returns the outcome so the capture dialog can show it. A rejection
        clears the active flag rather than leaving the menu claiming a
        binding that the listener may have just dropped.
        """
        ok, err = self.hotkey.apply_combo(combo)
        if not ok:
            self._hotkey_ok = False
            return False, err
        set_hotkey(combo)
        self._hotkey_combo = combo
        self._hotkey_ok = True
        _log.info(f"global hotkey set to {combo}")
        return True, None

    def set_opacity(self, value: float) -> None:
        """Update the window opacity from any thread and persist it.

        Reached directly from the pystray thread, so the work is marshaled
        through `ui_call`, which drops it once the window is gone instead
        of raising into the tray callback.
        """
        try:
            target = clamp_opacity(value)
        except (TypeError, ValueError):
            return
        self.ui_call(self._apply_opacity, target)

    def _apply_opacity(self, target: float) -> None:
        """Apply an already-clamped opacity on the Tk thread."""
        self.opacity = config_set_opacity(target)
        self._apply_alpha()
        _log.info(f"opacity set to {self.opacity:.2f}")

    def _apply_alpha(self) -> None:
        """Set the window alpha, ignoring platforms that lack it."""
        try:
            self.root.attributes("-alpha", self.opacity)
        except tk.TclError:
            pass  # documented contract: alpha unsupported on some systems

    def attach_tray(self, tray: TrayController) -> None:
        """Attach the tray controller and push the current summary."""
        self.tray = tray
        self.tray.update_speedtest_summary(self._speedtest_summary)

    def shutdown(self) -> None:
        """Stop background services and the tray, then destroy the window."""
        if not self._run:
            return
        section("App exit")
        self._run = False
        self._stop_event.set()
        self._hover_guard_active = False
        self.sampler.stop()
        self.probe.stop()
        self.hotkey.stop()
        if self.tray is not None:
            self.tray.stop()
        self.root.destroy()

    # ---------- Speedtest ----------

    def run_speedtest_now(self, manual: bool = True) -> None:
        """Launch a speedtest on a worker thread. No-op if one runs.

        Must run on the Tk thread (directly or via ui_call) so the
        running-flag check-then-set stays serialized across triggers.
        """
        if self._speedtest_running:
            return
        self._speedtest_running = True
        section("Speedtest run (manual)" if manual else "Speedtest run (scheduled)")

        if self.tray is not None:
            self.tray.start_speedtest_check()

        try:
            threading.Thread(
                target=self._speedtest_worker, name="speedtest-worker", daemon=True
            ).start()
        except OSError as err:
            # Without the worker there is no finally to unwind the latch, so
            # clear it here or no later speedtest could ever start.
            _log.error(f"could not start the speedtest worker: {err}")
            self._reset_speedtest_run()

    def _reset_speedtest_run(self) -> None:
        """Clear the running latch and the tray busy marker."""
        self._speedtest_running = False
        if self.tray is not None:
            self.tray.stop_speedtest_check()

    def _speedtest_worker(self) -> None:
        """Measure, persist and publish one speedtest result.

        Only a real measurement is stored. The passive estimate describes
        incidental traffic, so showing it as the last speedtest or letting
        it set the next due time would present an unmeasured link as known.
        The latch is cleared in `finally` because a raise on any publish path
        would otherwise stop every later run.
        """
        retry_after = SPEEDTEST_RETRY_SEC
        try:
            result = measure_speed()
            if result.measured:
                saved = set_speedtest(result.down_mbps, result.up_mbps)
                value = f"{saved['down_mbps']:.1f} D | {saved['up_mbps']:.1f} U"
                retry_after = SPEEDTEST_INTERVAL_SEC
            else:
                value = f"{result.down_mbps:.1f} D | {result.up_mbps:.1f} U"
                retry_after = SPEEDTEST_RETRY_SEC

            label = "Speedtest" if result.measured else "Speedtest estimate"
            self.ui_call(
                self._apply_speedtest_result,
                result.down_mbps,
                result.up_mbps,
                result.measured,
            )
            _log.info(f"{label}: {value} Mb/s, source={result.source}")
            self._notify_tray(f"{label}: {value}")
        except Exception as err:  # noqa: BLE001 - a failed run must not stop the app
            _log.error(f"speedtest run failed: {err}")
            self._notify_tray("Speedtest: failed")
        finally:
            self._finish_speed_run(retry_after)

    def _finish_speed_run(self, retry_after_sec: float) -> None:
        """Clear the running latch and schedule the next automatic run."""
        self._speedtest_running = False
        self._speedtest_next_due = time.time() + retry_after_sec
        if self.tray is not None:
            self.tray.stop_speedtest_check()

    def _speedtest_scheduler_loop(self) -> None:
        """Trigger a scheduled run whenever the due time passes.

        The launch is marshaled to the Tk thread so the running-flag
        check-then-set in run_speedtest_now serializes with the manual
        triggers from the menu, double-click and tray. The local flag
        check is only a cheap pre-filter.
        """
        while not self._stop_event.wait(5):
            if self._speedtest_running or time.time() < self._speedtest_next_due:
                continue
            self.ui_call(self.run_speedtest_now, manual=False)

    def _compute_next_speedtest_due(self) -> float:
        """Epoch time of the next automatic speedtest, from saved state."""
        return next_speedtest_due(
            get_speedtest(),
            time.time(),
            SPEEDTEST_INTERVAL_SEC,
            SPEEDTEST_STARTUP_GRACE_SEC,
        )

    def _load_saved_speedtest(self) -> None:
        """Reflect the saved speedtest in the menu and tray, if present."""
        saved = get_speedtest()
        if saved is not None:
            self._apply_speedtest_result(
                saved["down_mbps"], saved["up_mbps"], measured=True
            )

    def _apply_speedtest_result(
        self, down_mbps: float, up_mbps: float, measured: bool = True
    ) -> None:
        """Show the last result on the second row, labelled by its kind."""
        kind = "speedtest" if measured else "estimate"
        self._speedtest_summary = (
            f"Last {kind}: {down_mbps:.1f} D | {up_mbps:.1f} U Mb/s"
        )
        self.canvas.itemconfig(self._t_st_down, text=f"↓ {down_mbps:.1f}")
        self.canvas.itemconfig(self._t_st_up, text=f"↑ {up_mbps:.1f}")
        self.canvas.itemconfig(self._t_st_unit, text=f"Mb/s {kind}")

    def _notify_tray(self, message: str) -> None:
        """Send a summary string to the tray, if attached."""
        if self.tray is not None:
            self.tray.update_speedtest_summary(message)


def main() -> None:
    """Build the widget, attach the tray and run the Tk mainloop.

    Raises:
        SystemExit: 1 after reporting any startup failure, because a build
            with no console would otherwise vanish without a trace.
    """
    root = tk.Tk()
    try:
        app = NetSpeedWidget(root)
        tray = TrayController(app, APP_NAME)
        tray.start()
        app.attach_tray(tray)
        root.mainloop()
    except Exception as err:
        _log.error(f"fatal startup error: {err}\n{traceback.format_exc()}")
        messagebox.showerror(APP_NAME, f"NetSpeed Widget failed to start:\n{err}")
        raise SystemExit(1) from err


if __name__ == "__main__":
    main()
