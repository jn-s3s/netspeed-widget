"""NetSpeed Widget - a small always-on-top network monitor for Windows.

The UI is a single borderless "pill" drawn on one canvas with a
transparent-color window, which gives real rounded corners. All UI work
runs on the Tk main thread. Throughput sampling and latency probing run
on daemon threads and publish snapshots that the UI polls through
`root.after`. The saved window position is validated against the
currently connected monitors so the widget can never be stranded on a
screen that is no longer attached.
"""

import threading
import time
import tkinter as tk
from tkinter import font as tkfont
from typing import Any, Callable

import win32api

from tray.container import TrayController
from utils.config import (
    get_hide_on_hover,
    get_hotkey,
    get_opacity,
    get_position,
    get_speedtest as config_get_speedtest,
    set_hide_on_hover as config_set_hide_on_hover,
    set_hotkey as config_set_hotkey,
    set_opacity as config_set_opacity,
    set_position as config_set_position,
    set_speedtest as config_set_speedtest,
)
from utils.hotkeys import GlobalHotkey, combo_from_tk_event, format_hotkey
from utils.latency import LatencyProbe
from utils.logger import info, section, startup, warn
from utils.sampler import NetSample, NetSampler
from utils.speedtest import measure_speed

APP_VERSION = "2.2.0"
APP_NAME = f"NetSpeed Widget v{APP_VERSION} by jn-s3s"

# Theme
SURFACE = "#161b22"
BORDER = "#2c333d"
BORDER_HOVER = "#3d4757"
FG = "#e6edf3"
FG_DIM = "#8b949e"
DOWN_COLOR = "#3fb950"
UP_COLOR = "#58a6ff"
DOWN_FILL = "#12321c"
GOOD_COLOR = "#3fb950"
WARN_COLOR = "#d29922"
BAD_COLOR = "#f85149"
TRANSPARENT = "#ff00ff"

FONT_FAMILY = "Segoe UI"
UI_TICK_MS = 250
GRAPH_SAMPLES = 60
CORNER_MARGIN = 12
MONITOR_CHECK_TICKS = 16
SPEEDTEST_INTERVAL_SEC = 4 * 60 * 60
SPEEDTEST_STARTUP_GRACE_SEC = 20

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


def _format_speed(mbps: float) -> str:
    """Format a speed with precision matched to its magnitude."""
    if mbps >= 100.0:
        return f"{mbps:.0f}"
    if mbps >= 10.0:
        return f"{mbps:.1f}"
    return f"{mbps:.2f}"


def _rounded_rect(
    canvas: tk.Canvas, x1: int, y1: int, x2: int, y2: int, radius: int, **kw: Any
) -> int:
    """Draw a rounded rectangle as a smoothed polygon. Returns item id."""
    points = [
        x1 + radius, y1,
        x2 - radius, y1,
        x2, y1,
        x2, y1 + radius,
        x2, y2 - radius,
        x2, y2,
        x2 - radius, y2,
        x1 + radius, y2,
        x1, y2,
        x1, y2 - radius,
        x1, y1 + radius,
        x1, y1,
    ]
    return canvas.create_polygon(points, smooth=True, **kw)


def _active_monitor_rects() -> list[tuple[int, int, int, int]]:
    """Return (left, top, right, bottom) for every connected monitor."""
    try:
        return [tuple(rect) for _, _, rect in win32api.EnumDisplayMonitors()]
    except Exception as err:
        warn(f"[APP] Could not enumerate monitors: {err}")
        return []


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
            warn(f"[HOTKEY] '{self._hotkey_combo}' unavailable at startup: {err}")

        self.sampler = NetSampler(interval=1.0, history=GRAPH_SAMPLES)
        self.probe = LatencyProbe()
        self.sampler.start()
        self.probe.start()

        self._last_sample_ts = 0.0
        self._smooth_down = 0.0
        self._smooth_up = 0.0
        self._graph_scale = 1.0
        self._max_up_seen = 0.0
        self._max_down_seen = 0.0
        self._tick_count = 0
        self._status_color = ""
        self._hover_guard_active = False
        self._dragging = False
        self._drag_offset = (0, 0)
        self._speedtest_summary = ""
        self.tray: Any = None

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
        info(
            f"[APP] Initialized at x={self.win_x} y={self.win_y} "
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
        self.root.configure(bg=TRANSPARENT)
        try:
            self.root.attributes("-transparentcolor", TRANSPARENT)
            self._rounded = True
        except tk.TclError:
            warn("[APP] Transparency key unsupported; using flat background")

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
            self._pill = _rounded_rect(
                self.canvas, 1, 1, PILL_W - 1, PILL_H - 1, PILL_R,
                fill=SURFACE, outline=BORDER, width=1,
            )
        else:
            self._pill = self.canvas.create_rectangle(
                0, 0, PILL_W, PILL_H, fill=SURFACE, outline=BORDER, width=1
            )

        cy = PILL_H / 2
        self._dot_item = self.canvas.create_oval(
            DOT_X - DOT_R, cy - DOT_R, DOT_X + DOT_R, cy + DOT_R,
            fill=FG_DIM, outline="",
        )

        # Row 1: live speeds and latency
        self._t_down_arrow = self.canvas.create_text(
            DOWN_ARROW_X, ROW1_Y, text="↓", font=self.font_arrow,
            fill=DOWN_COLOR, anchor="w",
        )
        self._t_down_val = self.canvas.create_text(
            DOWN_VAL_X, ROW1_Y, text="0.00", font=self.font_value,
            fill=DOWN_COLOR, anchor="w",
        )
        self._t_up_arrow = self.canvas.create_text(
            UP_ARROW_X, ROW1_Y, text="↑", font=self.font_arrow,
            fill=UP_COLOR, anchor="w",
        )
        self._t_up_val = self.canvas.create_text(
            UP_VAL_X, ROW1_Y, text="0.00", font=self.font_value,
            fill=UP_COLOR, anchor="w",
        )
        self._t_unit = self.canvas.create_text(
            UNIT_X, ROW1_Y + 3, text="Mb/s", font=self.font_unit,
            fill=FG_DIM, anchor="w",
        )
        self._t_ping = self.canvas.create_text(
            PING_X, ROW1_Y, text="-- ms", font=self.font_ping,
            fill=FG_DIM, anchor="w",
        )

        # Row 2: last speedtest result, dimmer
        self._t_st_down = self.canvas.create_text(
            ST_DOWN_X, ROW2_Y, text="↓ --", font=self.font_mini,
            fill=DOWN_COLOR, anchor="w",
        )
        self._t_st_up = self.canvas.create_text(
            ST_UP_X, ROW2_Y, text="↑ --", font=self.font_mini,
            fill=UP_COLOR, anchor="w",
        )
        self._t_st_unit = self.canvas.create_text(
            ST_UNIT_X, ROW2_Y, text="Mb/s speedtest", font=self.font_unit,
            fill=FG_DIM, anchor="w",
        )

        self.menu = self._build_menu()


    def _place_window(self) -> None:
        """Size the window, then restore the saved or default position.

        A saved position that no longer sits on any connected monitor is
        discarded, so the widget never ends up invisible on a screen
        that has been detached or disabled.
        """
        self.win_width = PILL_W
        self.win_height = PILL_H
        position = get_position()
        if position is None:
            position = self._default_position()
        elif not self._window_on_active_monitor(*position):
            warn(
                "[APP] Saved position is off all active monitors; "
                "moving to the primary screen"
            )
            position = self._default_position()
            config_set_position(*position)
        self.win_x, self.win_y = position
        self.root.geometry(
            f"{self.win_width}x{self.win_height}+{self.win_x}+{self.win_y}"
        )
        self.root.deiconify()

    def _default_position(self) -> tuple[int, int]:
        """Bottom-right corner of the primary monitor work area."""
        monitor = win32api.MonitorFromPoint((0, 0))
        _, _, right, bottom = win32api.GetMonitorInfo(monitor)["Work"]
        return (
            right - self.win_width - CORNER_MARGIN,
            bottom - self.win_height - CORNER_MARGIN,
        )

    def _window_on_active_monitor(self, x: int, y: int) -> bool:
        """True when the window center is inside any connected monitor."""
        rects = _active_monitor_rects()
        if not rects:
            return True  # cannot verify, assume visible
        cx = x + self.win_width // 2
        cy = y + self.win_height // 2
        return any(l <= cx < r and t <= cy < b for l, t, r, b in rects)

    def _ensure_visible(self) -> None:
        """Snap back to the primary screen if stranded off-monitor.

        Covers display changes while the app is running, for example
        closing the laptop lid so only the external screen stays on.
        """
        if self._dragging or self._hover_guard_active:
            return
        if self._window_on_active_monitor(self.win_x, self.win_y):
            return
        info("[APP] Display layout changed; repositioning to primary screen")
        self.win_x, self.win_y = self._default_position()
        self.root.geometry(f"+{self.win_x}+{self.win_y}")
        config_set_position(self.win_x, self.win_y)

    def reset_position(self) -> None:
        """Move the widget back to the default corner and persist it."""
        self.win_x, self.win_y = self._default_position()
        self.root.geometry(f"+{self.win_x}+{self.win_y}")
        config_set_position(self.win_x, self.win_y)
        info("[APP] Position reset to default corner")

    # ---------- Input ----------

    def _bind_input(self) -> None:
        """Bind drag, menu and speedtest gestures on the canvas."""
        canvas = self.canvas
        canvas.bind("<ButtonPress-1>", self._start_drag)
        canvas.bind("<B1-Motion>", self._on_drag)
        canvas.bind("<ButtonRelease-1>", self._end_drag)
        canvas.bind(
            "<Double-Button-1>", lambda _e: self.run_speedtest_now(manual=True)
        )
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

    def _end_drag(self, _event: tk.Event) -> None:
        """Persist the dropped position."""
        self._dragging = False
        config_set_position(self.win_x, self.win_y)


    def _build_menu(self) -> tk.Menu:
        """Create the right-click context menu."""
        menu = tk.Menu(
            self.root, tearoff=0, bg=SURFACE, fg=FG,
            activebackground=BORDER, activeforeground=FG,
            disabledforeground=FG_DIM, relief="flat", borderwidth=1,
            font=(FONT_FAMILY, 9),
        )
        menu.add_command(
            label="Run speedtest",
            command=lambda: self.run_speedtest_now(manual=True),
        )
        menu.add_command(label="Session: --", state="disabled")
        menu.add_command(label="Last speedtest: --", state="disabled")
        self._menu_session_index = 1
        self._menu_speedtest_index = 2

        opacity_menu = tk.Menu(
            menu, tearoff=0, bg=SURFACE, fg=FG,
            activebackground=BORDER, activeforeground=FG,
            font=(FONT_FAMILY, 9),
        )
        for level in (1.0, 0.9, 0.8, 0.7, 0.6, 0.5):
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
        self._menu_hotkey_index = 5
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
        config_set_hide_on_hover(self._hover_var.get())

    def toggle_hover_hide(self) -> None:
        """Flip the auto-hide setting. Called from the tray menu."""
        enabled = not get_hide_on_hover()
        config_set_hide_on_hover(enabled)
        self._hover_var.set(enabled)
        info(f"[APP] Auto-hide on hover: {enabled}")

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
        except Exception as err:
            warn(f"[APP] UI tick failed: {err}")
        finally:
            self.root.after(UI_TICK_MS, self._tick)

    def _render_sample(self, sample: NetSample) -> None:
        """Update texts, peak logging and the graph for one sample."""
        self._smooth_down += (sample.down_mbps - self._smooth_down) * 0.45
        self._smooth_up += (sample.up_mbps - self._smooth_up) * 0.45
        self.canvas.itemconfig(
            self._t_down_val, text=_format_speed(self._smooth_down)
        )
        self.canvas.itemconfig(
            self._t_up_val, text=_format_speed(self._smooth_up)
        )

        if sample.up_mbps > self._max_up_seen and sample.up_mbps >= 1.0:
            self._max_up_seen = sample.up_mbps
            info(f"[NET] New upstream peak {sample.up_mbps:.2f} Mb/s")
        if sample.down_mbps > self._max_down_seen and sample.down_mbps >= 1.0:
            self._max_down_seen = sample.down_mbps
            info(f"[NET] New downstream peak {sample.down_mbps:.2f} Mb/s")

        self._draw_graph()


    def _draw_graph(self) -> None:
        """Redraw the rolling traffic graph inside its canvas slot."""
        canvas = self.canvas
        canvas.delete("graph")

        samples = self.sampler.history
        if len(samples) < 2:
            return

        down_peak = max(s.down_mbps for s in samples)
        up_peak = max(s.up_mbps for s in samples)
        peak = max(down_peak, up_peak, 1.0)
        if peak > self._graph_scale:
            self._graph_scale = peak
        else:
            self._graph_scale = max(peak, self._graph_scale * 0.97)

        x0 = GRAPH_X0
        width = GRAPH_W
        y_base = GRAPH_BOTTOM
        g_h = GRAPH_BOTTOM - GRAPH_TOP
        step = width / (GRAPH_SAMPLES - 1)
        start_x = x0 + width - (len(samples) - 1) * step

        def to_y(value: float) -> float:
            return y_base - (value / self._graph_scale) * (g_h - 2)

        canvas.create_line(
            x0, y_base + 0.5, x0 + width, y_base + 0.5,
            fill=BORDER, tags="graph",
        )

        down_points: list[float] = []
        up_points: list[float] = []
        for i, sample in enumerate(samples):
            x = start_x + i * step
            down_points.extend((x, to_y(sample.down_mbps)))
            up_points.extend((x, to_y(sample.up_mbps)))

        end_x = start_x + (len(samples) - 1) * step
        canvas.create_polygon(
            start_x, y_base, *down_points, end_x, y_base,
            fill=DOWN_FILL, outline="", tags="graph",
        )
        canvas.create_line(*down_points, fill=DOWN_COLOR, width=2, tags="graph")
        canvas.create_line(*up_points, fill=UP_COLOR, width=1, tags="graph")

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
        """Recolor the status dot, skipping redundant canvas updates."""
        if color != self._status_color:
            self._status_color = color
            self.canvas.itemconfig(self._dot_item, fill=color)

    def _push_tray_status(self) -> None:
        """Update the tray tooltip with current speeds and latency."""
        if self.tray is None:
            return
        result = self.probe.latest
        if not result.ok:
            ping_text = "offline"
        elif result.ms is None:
            ping_text = "-- ms"
        else:
            ping_text = f"{result.ms:.0f} ms"
        self.tray.update_live_status(
            f"D {_format_speed(self._smooth_down)} Mb/s | "
            f"U {_format_speed(self._smooth_up)} Mb/s | {ping_text}"
        )


    # ---------- Hover ----------

    def _on_mouse_enter(self, _event: Any = None) -> None:
        """Highlight the pill border; hide on hover if enabled."""
        self.canvas.itemconfig(self._pill, outline=BORDER_HOVER)
        if self._hover_guard_active or not get_hide_on_hover():
            return
        self._hover_guard_active = True
        info("[APP] Hover hide")
        self.root.withdraw()
        self._poll_cursor_and_restore()

    def _on_mouse_leave(self, _event: Any = None) -> None:
        """Restore the default pill border."""
        self.canvas.itemconfig(self._pill, outline=BORDER)

    def _poll_cursor_and_restore(self) -> None:
        """Restore the window once the cursor leaves its bounds."""
        if not self._hover_guard_active:
            return
        x, y = win32api.GetCursorPos()
        inside = (
            self.win_x <= x <= self.win_x + self.win_width
            and self.win_y <= y <= self.win_y + self.win_height
        )
        if inside:
            self.root.after(120, self._poll_cursor_and_restore)
        else:
            info("[APP] Hover restore")
            self.root.deiconify()
            self.canvas.itemconfig(self._pill, outline=BORDER)
            self._hover_guard_active = False

    # ---------- Lifecycle ----------

    def ui_call(self, func: Callable[..., None], *args: Any, **kwargs: Any) -> None:
        """Schedule a callable on the Tk main thread. Safe from any thread."""
        self.root.after(0, lambda: func(*args, **kwargs))

    def show_window(self) -> None:
        """Show the widget and keep it on top."""
        info("[TRAY] Show window")
        self.root.deiconify()
        self.root.attributes("-topmost", True)

    def hide_window(self) -> None:
        """Hide the widget."""
        info("[TRAY] Hide window")
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

        top = tk.Toplevel(self.root)
        self._hotkey_dialog = top
        top.title("Set hotkey")
        top.attributes("-topmost", True)
        top.configure(bg=SURFACE, padx=16, pady=14)
        top.resizable(False, False)
        tk.Label(
            top, text="Press your new show/hide hotkey.",
            font=(FONT_FAMILY, 9, "bold"), fg=FG, bg=SURFACE,
        ).pack(anchor="w")
        tk.Label(
            top, text="Must include Ctrl or Alt. Esc cancels.",
            font=(FONT_FAMILY, 8), fg=FG_DIM, bg=SURFACE,
        ).pack(anchor="w", pady=(4, 0))
        status = tk.Label(
            top, text=f"Current: {format_hotkey(self._hotkey_combo)}",
            font=(FONT_FAMILY, 8), fg=FG_DIM, bg=SURFACE,
        )
        status.pack(anchor="w", pady=(8, 0))
        top.bind("<KeyPress>", lambda e: self._capture_hotkey(e, top, status))
        top.focus_force()

    def _capture_hotkey(
        self, event: tk.Event, top: tk.Toplevel, status: tk.Label
    ) -> None:
        """Validate and register the combo pressed in the capture dialog."""
        if event.keysym == "Escape":
            top.destroy()
            return
        try:
            combo = combo_from_tk_event(event)
        except ValueError as err:
            status.config(text=str(err), fg=BAD_COLOR)
            return
        if combo is None:
            return  # modifier-only press, keep listening

        ok, err = self.hotkey.apply_combo(combo)
        if not ok:
            warn(f"[HOTKEY] '{combo}' rejected: {err}")
            status.config(text=f"Unavailable: {err}", fg=BAD_COLOR)
            return
        config_set_hotkey(combo)
        self._hotkey_combo = combo
        self._hotkey_ok = True
        info(f"[HOTKEY] Global hotkey set to {combo}")
        status.config(text=f"Hotkey set: {format_hotkey(combo)}", fg=GOOD_COLOR)
        top.after(700, top.destroy)

    def set_opacity(self, value: float) -> None:
        """Update the window opacity from any thread and persist it."""
        try:
            target = max(0.40, min(1.00, float(value)))
        except (TypeError, ValueError):
            return

        def _apply() -> None:
            self.opacity = config_set_opacity(target)
            self._apply_alpha()
            info(f"[APP] Opacity set to {self.opacity:.2f}")

        self.root.after(0, _apply)

    def _apply_alpha(self) -> None:
        try:
            self.root.attributes("-alpha", self.opacity)
        except tk.TclError:
            pass  # documented contract: alpha unsupported on some systems

    def attach_tray(self, tray: Any) -> None:
        """Attach the tray controller and push the current summary."""
        self.tray = tray
        self.tray.update_speedtest_summary(self._speedtest_summary)

    def shutdown(self) -> None:
        """Stop background services and destroy the window."""
        if not self._run:
            return
        section("App exit")
        self._run = False
        self._stop_event.set()
        self._hover_guard_active = False
        self.sampler.stop()
        self.probe.stop()
        self.hotkey.stop()
        self.root.destroy()


    # ---------- Speedtest ----------

    def run_speedtest_now(self, manual: bool = True) -> None:
        """Launch a speedtest on a worker thread. No-op if one runs."""
        if self._speedtest_running:
            return
        self._speedtest_running = True
        section("Speedtest run (manual)" if manual else "Speedtest run (scheduled)")

        if self.tray is not None:
            self.tray.start_speedtest_check()

        threading.Thread(
            target=self._speedtest_worker, name="speedtest-worker", daemon=True
        ).start()

    def _speedtest_worker(self) -> None:
        """Measure, persist and publish one speedtest result."""
        try:
            result = measure_speed()
            saved = config_set_speedtest(result.down_mbps, result.up_mbps)
            self.ui_call(
                self._apply_speedtest_result,
                result.down_mbps,
                result.up_mbps,
            )
            info(
                f"[SPEEDTEST] Result: down={result.down_mbps:.2f} Mb/s, "
                f"up={result.up_mbps:.2f} Mb/s"
            )
            self._notify_tray(
                f"Speedtest: {saved['down_mbps']:.1f} D | "
                f"{saved['up_mbps']:.1f} U Mb/s"
            )
        except Exception as err:
            warn(f"[SPEEDTEST] run failed: {err}")
            self._notify_tray("Speedtest: failed")
        finally:
            self._speedtest_running = False
            self._speedtest_next_due = time.time() + SPEEDTEST_INTERVAL_SEC
            if self.tray is not None:
                self.tray.stop_speedtest_check()

    def _speedtest_scheduler_loop(self) -> None:
        """Trigger a scheduled run whenever the due time passes."""
        while not self._stop_event.wait(5):
            due = time.time() >= self._speedtest_next_due
            if not self._speedtest_running and due:
                self.run_speedtest_now(manual=False)

    def _compute_next_speedtest_due(self) -> float:
        """Compute the next epoch time for an automatic speedtest.

        If a saved result exists, schedule one interval after it. If that
        time already passed, or no result exists, apply the startup grace.
        """
        now = time.time()
        speedtest = config_get_speedtest(None)
        if speedtest and "ts" in speedtest:
            due = float(speedtest["ts"]) + SPEEDTEST_INTERVAL_SEC
            return due if due > now else now + SPEEDTEST_STARTUP_GRACE_SEC
        return now + SPEEDTEST_STARTUP_GRACE_SEC

    def _load_saved_speedtest(self) -> None:
        """Reflect the saved speedtest in the menu and tray, if present."""
        speedtest = config_get_speedtest(None)
        if speedtest:
            self._apply_speedtest_result(
                float(speedtest["down_mbps"]), float(speedtest["up_mbps"])
            )

    def _apply_speedtest_result(self, down_mbps: float, up_mbps: float) -> None:
        """Store and show the last speedtest result."""
        self._speedtest_summary = (
            f"Last speedtest: {down_mbps:.1f} D | {up_mbps:.1f} U Mb/s"
        )
        self.canvas.itemconfig(self._t_st_down, text=f"↓ {down_mbps:.1f}")
        self.canvas.itemconfig(self._t_st_up, text=f"↑ {up_mbps:.1f}")

    def _notify_tray(self, message: str) -> None:
        """Send a summary string to the tray, if attached."""
        if self.tray is not None:
            self.tray.update_speedtest_summary(message)


if __name__ == "__main__":
    root = tk.Tk()
    app = NetSpeedWidget(root)

    tray = TrayController(app, APP_NAME)
    tray.start()
    app.attach_tray(tray)

    root.mainloop()

