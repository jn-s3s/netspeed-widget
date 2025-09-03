import os
import threading
import time
import subprocess
import tkinter as tk
import json
import shutil
from tkinter import font as tkfont
from typing import Any, Callable

import psutil
import win32api

from utils.hotkeys import Hotkeys
from tray.container import TrayController
from utils.config import (
    get_opacity,
    set_opacity as config_set_opacity,
    get_speedtest as config_get_speedtest,
    set_speedtest as config_set_speedtest,
)
from utils.paths import resource_path
from utils.logger import startup, info, warn, section

try:
    import speedtest as _speedtest
except Exception:
    _speedtest = None

APP_VERSION = "1.1.0"
APP_NAME = f"NetSpeed Widget v{APP_VERSION} by jn-s3s"
FONT_FAMILY = "Segoe UI"
PING_HOST = "fast.com"
PING_TIMEOUT_MS = 1200
SPEEDTEST_INTERVAL_SEC = 4 * 60 * 60  # 4 hours
SPEEDTEST_STARTUP_GRACE_SEC = 20 # 20 seconds

class NetSpeedWidget:
    """
    Floating mini widget that shows current network up/down speeds with a tiny
    two-line graph. It auto-hides on hover and restores when the cursor leaves.
    Tracks recent samples, highlights ping loss, and can run periodic speed tests.
    """

    def __init__(self, root: tk.Tk) -> None:
        """
        Initialize the widget UI and services.

        This sets window flags, binds hotkeys, restores saved opacity, builds labels
        and canvas, positions the window in the primary work area, and starts:
          - the 1 Hz UI refresh loop,
          - the background speedtest scheduler,
          - the hover guard that hides/restores the window.
        """
        self.root = root
        self.root.title(APP_NAME)
        self.root.configure(bg="black")
        self.root.overrideredirect(True)          # borderless
        self.root.attributes("-topmost", True)    # always-on-top

        # --- Log state helpers ---
        self._last_ping_ok: bool = True
        self._max_up_seen: float = 0.0
        self._max_down_seen: float = 0.0

        # Bind global hotkeys (opacity control)
        Hotkeys(self).bind()

        # Apply saved opacity
        self.opacity = get_opacity()
        try:
            self.root.attributes("-alpha", self.opacity)
        except Exception:
            # Some environments may not support alpha; ignore safely
            pass

        # --- Layout sizing ---
        self.height: int = 45
        self.graph_width: int = 150
        self.graph_height: int = self.height - 10

        # --- Fonts ---
        self.font_value = tkfont.Font(family=FONT_FAMILY, size=10, weight="bold")
        self.font_unit = tkfont.Font(family=FONT_FAMILY, size=7)
        self.font_xs = tkfont.Font(family=FONT_FAMILY, size=6)

        # --- Layout Frames ---
        self.main_frame = tk.Frame(self.root, bg="black")
        self.main_frame.pack(fill="both", expand=True)

        self.text_frame = tk.Frame(self.main_frame, bg="black")
        self.text_frame.pack(side="left", padx=(6, 4), pady=4)

        # --- Download row ---
        self.lbl_down_val = tk.Label(self.text_frame, text="0.00", font=self.font_value, fg="lime", bg="black")
        self.lbl_down_unit = tk.Label(self.text_frame, text="Mb/s", font=self.font_unit, fg="#ECF8F8", bg="black")
        self.lbl_down_arrow = tk.Label(self.text_frame, text="⬇", font=self.font_unit, fg="lime", bg="black")
        self._pack_row(self.lbl_down_arrow, self.lbl_down_val, self.lbl_down_unit)

        # --- Upload row ---
        self.lbl_up_val = tk.Label(self.text_frame, text="0.00", font=self.font_value, fg="cyan", bg="black")
        self.lbl_up_unit = tk.Label(self.text_frame, text="Mb/s", font=self.font_unit, fg="#ECF8F8", bg="black")
        self.lbl_up_arrow = tk.Label(self.text_frame, text="⬆", font=self.font_unit, fg="cyan", bg="black")
        self._pack_row(self.lbl_up_arrow, self.lbl_up_val, self.lbl_up_unit)

        # Two tiny speedtest readouts
        self.lbl_down_st = tk.Label(self.text_frame, text="↓ -- Mb/s", font=self.font_xs, fg="lime", bg="black")
        self.lbl_down_st.pack(side="top", anchor="w", padx=6, pady=(0, 0))
        self.lbl_up_st = tk.Label(self.text_frame, text="↑ -- Mb/s", font=self.font_xs, fg="cyan", bg="black")
        self.lbl_up_st.pack(side="top", anchor="w", padx=6, pady=(0, 4))

        # --- Graph canvas ---
        self.canvas = tk.Canvas(
            self.main_frame,
            width=self.graph_width,
            height=self.graph_height,
            bg="black",
            highlightthickness=0,
        )
        self.canvas.pack(side="right", padx=(4, 6), pady=4)

        # --- Window geometry (bottom-right corner of primary monitor work area) ---
        self.root.update_idletasks()
        work_area = win32api.GetMonitorInfo(win32api.MonitorFromPoint((0, 0)))["Work"]
        _, _, right, bottom = work_area
        total_width = self.text_frame.winfo_reqwidth() + self.graph_width + 16

        # Store geometry values; also used by hover-guard hit test
        self.win_width: int = total_width
        self.win_height: int = self.height
        self.win_x: int = right - total_width
        self.win_y: int = bottom - self.height
        self.root.geometry(f"{self.win_width}x{self.win_height}+{self.win_x}+{self.win_y}")

        # --- Counters baseline ---
        counters = psutil.net_io_counters()
        self.last_bytes_sent: int = counters.bytes_sent
        self.last_bytes_recv: int = counters.bytes_recv

        # --- Sample series buffers (10 points) ---
        self.upload_speeds: list[float] = []
        self.download_speeds: list[float] = []
        # True means the segment ending at index i had a ping drop
        self.ping_loss: list[bool] = []

        startup(APP_NAME)
        info(f"[APP] Initialized at x={self.win_x} y={self.win_y} size={self.win_width}x{self.win_height} opacity={self.opacity:.2f}")

        # --- Updater thread ---
        self._run: bool = True
        threading.Thread(target=self.update_loop, daemon=True).start()

        # Persisted last speedtest + scheduler
        self._speedtest_running = False
        self._speedtest_next_due = self._compute_next_speedtest_due()

        # Reflect saved result in the UI if available
        self._apply_saved_speedtest_labels()

        # Background scheduler that triggers a run
        threading.Thread(target=self._speedtest_scheduler_loop, daemon=True).start()

        # --- Hover/restore behavior ---
        self._hover_guard_active: bool = False
        self.root.bind("<Enter>", self._on_mouse_enter)

        # --- Clean exit ---
        self.root.protocol("WM_DELETE_WINDOW", self._on_close)


    def _pack_row(self, *widgets: Any) -> None:
        """
        Pack a horizontal row of small labels inside text_frame.
        """
        row = tk.Frame(self.text_frame, bg="black")
        row.pack(anchor="w")
        for w in widgets:
            # Ensure the widget packs into the row container
            w.master = row  # type: ignore[attr-defined]
            w.pack(side="left")


    def _on_mouse_enter(self, _event: Any = None) -> None:
        """
        When the cursor enters the window, hide it and begin polling until
        the cursor leaves the widget bounds, then restore.
        """
        if not self._hover_guard_active:
            self._hover_guard_active = True
            info("[APP] Hover hide")
            self.root.withdraw()
            self._poll_cursor_and_restore()


    def _poll_cursor_and_restore(self) -> None:
        """
        Poll the global cursor position; if it's still inside the last-known
        window rect, keep waiting; otherwise restore the window once it leaves.
        """
        if not self._hover_guard_active:
            return

        # Current global cursor position
        x, y = win32api.GetCursorPos()
        inside = (self.win_x <= x <= self.win_x + self.win_width and
                  self.win_y <= y <= self.win_y + self.win_height)
        if inside:
            # keep waiting, then retry
            self.root.after(120, self._poll_cursor_and_restore)
        else:
            # restore once pointer is outside
            info("[APP] Hover restore")
            self.root.deiconify()
            self._hover_guard_active = False


    def _ping_once(self) -> bool:
        """
        Perform a single ICMP ping. Returns True if ping succeeds, False otherwise.
        """
        cmd = ["ping", PING_HOST, "-n", "1", "-w", str(PING_TIMEOUT_MS)]
        try:
            res = subprocess.run(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=0x08000000,  # CREATE_NO_WINDOW
                check=False,
            )
            return res.returncode == 0
        except Exception:
            return False


    def update_loop(self) -> None:
        """
        Background loop (~1Hz) that samples net I/O, updates labels, and redraws the graph.
        """
        while self._run:
            start = time.time()
            counters = psutil.net_io_counters()
            new_sent = counters.bytes_sent
            new_recv = counters.bytes_recv

            up_mbps = (new_sent - self.last_bytes_sent) * 8.0 / 1_000_000.0
            down_mbps = (new_recv - self.last_bytes_recv) * 8.0 / 1_000_000.0

            # Log new peaks with a small threshold to avoid noise
            if up_mbps > self._max_up_seen and up_mbps >= 1.0:
                self._max_up_seen = up_mbps
                info(f"[NET] New upstream peak {up_mbps:.2f} Mb/s")

            if down_mbps > self._max_down_seen and down_mbps >= 1.0:
                self._max_down_seen = down_mbps
                info(f"[NET] New downstream peak {down_mbps:.2f} Mb/s")

            self.last_bytes_sent = new_sent
            self.last_bytes_recv = new_recv

            # Ping for this tick
            ok = self._ping_once()
            dropped = not ok

            # Ping state edge logging
            if ok and not self._last_ping_ok:
                info("[NET] Ping restored")
            elif not ok and self._last_ping_ok:
                info("[NET] Ping dropped")
            self._last_ping_ok = ok

            # Append series
            self.upload_speeds.append(up_mbps)
            self.download_speeds.append(down_mbps)
            self.ping_loss.append(dropped)

            # Keep last 10 samples
            for arr in (self.upload_speeds, self.download_speeds, self.ping_loss):
                if len(arr) > 10:
                    arr.pop(0)

            # Update labels
            self.lbl_down_val.config(text=f"{down_mbps:.2f}")
            self.lbl_up_val.config(text=f"{up_mbps:.2f}")

            # Redraw
            self.draw_graph()

            # Pace to ~1s
            elapsed = time.time() - start
            time.sleep(max(0.0, 1.0 - elapsed))


    def draw_graph(self) -> None:
        """
        Draw two polylines (download top, upload bottom). Red segment indicates ping loss.
        """
        self.canvas.delete("all")

        # Base scale on max of both series
        max_speed = max(self.download_speeds + self.upload_speeds + [1.0])

        def draw_line(data: list[float], loss_flags: list[bool], base_color: str, offset_y: int) -> None:
            n = len(data)
            if n < 2:
                return

            half = self.graph_height // 2

            # Map points
            pts: list[tuple[float, float]] = []
            for i, val in enumerate(data):
                x = i * (self.graph_width / 9.0)  # 10 samples -> 9 segments
                y = (half - (val / max_speed) * (half - 2)) + offset_y
                pts.append((x, y))

            # Draw segments individually (color per-segment)
            for i in range(1, n):
                x0, y0 = pts[i - 1]
                x1, y1 = pts[i]
                seg_color = "red" if (i < len(loss_flags) and loss_flags[i]) else base_color
                self.canvas.create_line(x0, y0, x1, y1, fill=seg_color, width=2)

        # Download and Upload lines
        draw_line(self.download_speeds, self.ping_loss, "lime", 0)
        draw_line(self.upload_speeds, self.ping_loss, "cyan", self.graph_height // 2)

    # ---------- App lifecycle / tray helpers ----------

    def _on_close(self) -> None:
        """Stop loop and destroy the window."""
        section("App exit")
        self._run = False
        self._hover_guard_active = False
        self.root.destroy()


    def ui_call(self, func: Callable[..., None], *args: Any, **kwargs: Any) -> None:
        """
        Schedule a callable to run on the Tk main thread.
        Safe to call from other threads.
        """
        self.root.after(0, lambda: func(*args, **kwargs))


    def show_window(self) -> None:
        """Show (deiconify) the widget and keep it on top."""
        info("[TRAY] Show window")
        self.root.deiconify()
        self.root.attributes("-topmost", True)


    def hide_window(self) -> None:
        """Hide (withdraw) the widget."""
        info("[TRAY] Hide window")
        self.root.withdraw()


    def set_opacity(self, value: float) -> None:
        """
        Safely update the window opacity from any thread and persist it.
        """
        try:
            target = max(0.40, min(1.00, float(value)))
        except Exception:
            return

        def _apply():
            # persist then apply
            self.opacity = config_set_opacity(target)
            try:
                self.root.attributes("-alpha", self.opacity)
                info(f"[APP] Opacity set to {self.opacity:.2f}")
            except Exception:
                pass

        self.root.after(0, _apply)


    def _apply_saved_speedtest_labels(self) -> None:
        """
        If a saved speedtest exists, reflect it in the tiny Mb/s labels.
        """
        st = config_get_speedtest(None)
        if not st:
            return
        try:
            self.lbl_down_st.config(text=f"↓ {st['down_mbps']:.2f} Mb/s")
            self.lbl_up_st.config(text=f"↑ {st['up_mbps']:.2f} Mb/s")
        except Exception:
            pass


    def _format_speedtest_summary(self) -> str:
        """
        Build a short one-line summary for the tray tooltip.
        """
        st = config_get_speedtest(None)
        if not st:
            return "Speedtest: --"
        return f"Speedtest: {st['down_mbps']:.1f}↓ | {st['up_mbps']:.1f}↑ Mb/s"


    def _speedtest_scheduler_loop(self) -> None:
        """
        Background scheduler that triggers speedtests on schedule.

        Every 5 seconds it checks the due time. If no speedtest is running and the
        due time has passed, it calls `run_speedtest_now(manual=False)`.
        """
        while getattr(self, "_run", True):
            try:
                now = time.time()
                if not self._speedtest_running and now >= self._speedtest_next_due:
                    self.run_speedtest_now(manual=False)
            except Exception:
                pass
            time.sleep(5)


    def run_speedtest_now(self, manual: bool = True) -> None:
        """
        Launch a speedtest on a worker thread.
        """
        section("Speedtest run (manual)" if manual else "Speedtest run (scheduled)")
        if self._speedtest_running:
            return
        self._speedtest_running = True

        # On manual trigger, reset the small labels to placeholders for a fresh look
        if manual:
            try:
                self.ui_call(self.lbl_down_st.config, text="↓ -- Mb/s")
                self.ui_call(self.lbl_up_st.config,   text="↑ -- Mb/s")
            except Exception:
                pass

        # Notify the tray that a test is starting so it can show a busy indicator
        if hasattr(self, "tray") and self.tray:
            try:
                self.tray.start_speedtest_check()
            except Exception:
                pass

        # Spawn the background worker that performs the actual speed test
        thread = threading.Thread(target=self._speedtest_worker, daemon=True)
        thread.start()


    def _speedtest_worker(self) -> None:
        """
        Measure, persist, update UI, and notify the tray.
        """
        try:
            down_mbps, up_mbps = self._measure_speed()
            saved_speedtest = config_set_speedtest(down_mbps, up_mbps)
            self._update_speedtest_ui(down_mbps, up_mbps)
            info(f"[SPEEDTEST] Result: down={down_mbps:.2f} Mb/s, up={up_mbps:.2f} Mb/s")
            self._notify_tray(f"Speedtest: {saved_speedtest['down_mbps']:.1f}↓ | {saved_speedtest['up_mbps']:.1f}↑ Mb/s")
        except Exception:
            self._notify_tray("Speedtest: failed")
        finally:
            self._speedtest_running = False
            self._speedtest_next_due = time.time() + SPEEDTEST_INTERVAL_SEC
            self._stop_tray_spinner()


    def _measure_speed(self) -> tuple[float, float]:
        """
        Try providers in order and return the first successful (down, up) in Mb/s.
        """
        for provider in (self._measure_fast_cli, self._measure_python_speedtest, self._measure_passive_estimate):
            result = self._safe(provider)
            if result is not None:
                return result
        raise RuntimeError("All speed providers failed")


    def _measure_fast_cli(self) -> tuple[float, float] | None:
        """
        Measure using fast.com via `fast` or `fast-cli`.
        """
        d_fast, u_fast = self._run_fast_cli()
        if d_fast is None or u_fast is None:
            warn(f"[SPEEDTEST] fast-cli unavailable, trying next backend")
            return None
        return float(d_fast), float(u_fast)


    def _measure_python_speedtest(self) -> tuple[float, float] | None:
        """
        Measure using the `speedtest-cli` Python library via its in-process API.

        Converts bits per second to Mb/s. Threads/pre-allocation arguments are
        attempted with fallbacks for older library versions.
        """
        info("[SPEEDTEST] Backend: speedtest-cli (python module)")
        if _speedtest is None:
            warn(f"[SPEEDTEST] speedtest-cli unavailable, trying next backend")
            return None
        try:
            tester = _speedtest.Speedtest()
            tester.get_servers(None)
            tester.get_best_server()
            self._configure_speedtest(tester)

            try:
                tester.download(threads=8)
            except TypeError:
                tester.download()

            try:
                tester.upload(threads=8, pre_allocate=True)
            except TypeError:
                try:
                    tester.upload(pre_allocate=True)
                except TypeError:
                    tester.upload()

            result = tester.results.dict()  # bits per second
            down_mbps = float(result.get("download", 0.0)) / 1_000_000.0
            up_mbps   = float(result.get("upload",   0.0)) / 1_000_000.0
            return down_mbps, up_mbps
        except Exception:
            warn(f"[SPEEDTEST] speedtest-cli unavailable, trying next backend")
            return None


    def _measure_passive_estimate(self) -> tuple[float, float] | None:
        """
        Estimate throughput by sampling OS network counters for ~10 seconds.
        """
        info("[SPEEDTEST] Backend: psutil (fallback)")
        current_time = time.time()
        net_io_1 = psutil.net_io_counters()
        while time.time() - current_time < 10 and getattr(self, "_run", True):
            time.sleep(0.5)
        net_io_2 = psutil.net_io_counters()

        elapsed_time = max(time.time() - current_time, 1e-6)
        d_bytes = net_io_2.bytes_recv - net_io_1.bytes_recv
        u_bytes = net_io_2.bytes_sent - net_io_1.bytes_sent
        down_mbps = (d_bytes * 8.0) / (elapsed_time * 1_000_000.0)
        up_mbps   = (u_bytes * 8.0) / (elapsed_time * 1_000_000.0)
        return down_mbps, up_mbps


    def _update_speedtest_ui(self, down_mbps: float, up_mbps: float) -> None:
        """
        Update the small Mb/s labels on the main thread.
        """
        try:
            self.ui_call(self.lbl_down_st.config, text=f"↓ {down_mbps:.2f} Mb/s")
            self.ui_call(self.lbl_up_st.config,   text=f"↑ {up_mbps:.2f} Mb/s")
        except Exception:
            pass


    def _notify_tray(self, message: str) -> None:
        """
        Send a summary string to the tray if a tray controller is attached.
        """
        if hasattr(self, "tray") and self.tray:
            try:
                self.tray.update_speedtest_summary(message)
            except Exception:
                pass


    def _stop_tray_spinner(self) -> None:
        """
        Stop the tray busy indicator if present.
        """
        if hasattr(self, "tray") and self.tray:
            try:
                self.tray.stop_speedtest_check()
            except Exception:
                pass


    def _safe(self, fn):
        """
        This is a small helper for optional providers that can fail without
        affecting the overall flow.
        """
        try:
            return fn()
        except Exception:
            return None


    def attach_tray(self, tray: Any) -> None:
        """
        Attach a tray controller and push the current summary.
        """
        self.tray = tray
        try:
            self.tray.update_speedtest_summary(self._format_speedtest_summary())
        except Exception:
            pass


    def _configure_speedtest(self, tester) -> None:
        """
        Bias speedtest-cli toward larger upload payloads on Windows.

        Larger chunks reduce under-reporting by saturating the pipe more consistently.
        """
        try:
            config = tester.get_config()
            sizes = config.get("sizes", {})
            sizes["upload"] = [
                256 * 1024,
                512 * 1024,
                1 * 1024 * 1024,
                2 * 1024 * 1024,
                5 * 1024 * 1024,
                10 * 1024 * 1024,
                20 * 1024 * 1024,
                30 * 1024 * 1024,
            ]
            sizes["upload_min"] = 256 * 1024
            sizes["upload_max"] = 30 * 1024 * 1024
            config["sizes"] = sizes
            tester.config.update(config)
        except Exception:
            pass

    def _run_fast_cli(self) -> tuple[float | None, float | None]:
        """
        Run fast.com via a bundled Node fast-cli first, then via PATH fallback.
        """
        spawn_kw = self._fast_spawn_settings()
        data = self._run_node_bundle_fast(**spawn_kw) or self._run_path_fast(**spawn_kw)
        return self._parse_fast_result(data)


    def _fast_spawn_settings(self) -> dict:
        """
        Build `subprocess.run` keyword args that suppress child windows on Windows.
        """
        startupinfo = None
        creationflags = 0
        if os.name == "nt":
            startupinfo = subprocess.STARTUPINFO()
            startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            startupinfo.wShowWindow = 0
            creationflags = subprocess.CREATE_NO_WINDOW
        return {"startupinfo": startupinfo, "creationflags": creationflags}


    def _run_node_bundle_fast(self, **spawn_kw) -> dict | None:
        """
        Execute the bundled `node.exe` + fast-cli `cli.js` with `--json`.
        """
        info("[SPEEDTEST] Backend: fast-cli (bundled Node)")
        node_exe = resource_path(os.path.join("third_party", "node", "node.exe"))
        cli_js = resource_path(os.path.join(
            "third_party", "fast-bundle", "node_modules", "fast-cli", "distribution", "cli.js"
        ))
        bundle_cwd = resource_path(os.path.join("third_party", "fast-bundle"))

        if not (os.path.isfile(node_exe) and os.path.isfile(cli_js)):
            return None

        try:
            process = subprocess.run(
                [node_exe, cli_js, "--upload", "--json"],
                cwd=bundle_cwd,
                capture_output=True,
                text=True,
                timeout=180,
                check=True,
                **spawn_kw,
            )
            return json.loads(process.stdout.strip() or "{}")
        except Exception:
            return None


    def _run_path_fast(self, **spawn_kw) -> dict | None:
        """
        Execute `fast` or `fast-cli` from PATH with `--json`.
        """
        info("[SPEEDTEST] Backend: fast-cli (PATH)")
        on_path = shutil.which("fast") or shutil.which("fast-cli")
        if not on_path:
            return None

        try:
            process = subprocess.run(
                [on_path, "--upload", "--json"],
                capture_output=True,
                text=True,
                timeout=180,
                check=True,
                **spawn_kw,
            )
            return json.loads(process.stdout.strip() or "{}")
        except Exception:
            return None


    def _parse_fast_result(self, data: dict | None) -> tuple[float | None, float | None]:
        """
        Parse fast.com JSON emitted by fast-cli.
        """
        if not isinstance(data, dict):
            return None, None

        candidates = [
            (data, ("downloadSpeed", "download"), ("uploadSpeed", "upload")),
        ]

        speeds = data.get("speeds")
        if isinstance(speeds, dict):
            candidates.append((speeds, ("download",), ("upload",)))

        for src, dkeys, ukeys in candidates:
            down = self._first_float(src, dkeys)
            up = self._first_float(src, ukeys)
            if down is not None and up is not None:
                return down, up

        return None, None


    def _first_float(self, d: dict | None, keys: tuple[str, ...]) -> float | None:
        """
        Return the first value under any key that converts to float.
        """
        if not isinstance(d, dict):
            return None
        for key in keys:
            value = d.get(key)
            if value is None:
                continue
            try:
                return float(value)
            except Exception:
                continue
        return None


    def _compute_next_speedtest_due(self) -> float:
        """
        Compute the next epoch time for an automatic speedtest.

        Policy:
            - If a saved result exists with timestamp `ts`, schedule `ts + interval`.
            - If that time is already past, apply a short startup grace.
            - If no saved result exists, use the startup grace from now.

        This prevents an immediate auto-run at startup while maintaining the cadence.
        """
        now = time.time()
        speedtest = config_get_speedtest(None)
        if speedtest and isinstance(speedtest, dict) and "ts" in speedtest:
            due = float(speedtest["ts"]) + SPEEDTEST_INTERVAL_SEC
            return due if due > now else now + SPEEDTEST_STARTUP_GRACE_SEC
        return now + SPEEDTEST_STARTUP_GRACE_SEC


if __name__ == "__main__":
    root = tk.Tk()
    app = NetSpeedWidget(root)

    # Start system tray (separate thread)
    tray = TrayController(app, APP_NAME)
    tray.start()
    app.attach_tray(tray)

    root.mainloop()
