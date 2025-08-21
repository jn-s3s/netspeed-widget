import tkinter as tk
from tkinter import font as tkfont
import psutil
import threading
import time
import subprocess
import win32api
from utils.hotkeys import Hotkeys
from tray.container import TrayController

APP_NAME = "NetSpeed Widget by jn-s3s"
PING_HOST = "google.com"
PING_TIMEOUT_MS = 1200

class NetSpeedWidget:

    def __init__(self, root: tk.Tk):
        self.root = root
        self.root.title(f"{APP_NAME}")
        self.root.configure(bg="black")
        self.root.overrideredirect(True)
        self.root.attributes("-topmost", True)

        # Bind Hotkeys
        Hotkeys(self).bind()

        # Layout Size
        self.height = 45
        self.graph_width = 100
        self.graph_height = self.height - 10

        # Fonts
        self.font_value = tkfont.Font(family="Segoe UI", size=10, weight="bold")
        self.font_unit  = tkfont.Font(family="Segoe UI", size=7)

        # Layout Frame
        self.main_frame = tk.Frame(root, bg="black")
        self.main_frame.pack(fill="both", expand=True)
        self.text_frame = tk.Frame(self.main_frame, bg="black")
        self.text_frame.pack(side="left", padx=(6, 4), pady=4)

        # Download row
        self.lbl_down_val = tk.Label(self.text_frame, text="0.00", font=self.font_value, fg="lime", bg="black")
        self.lbl_down_unit = tk.Label(self.text_frame, text="Mb/s", font=self.font_unit, fg="#ECF8F8", bg="black")
        self.lbl_down_arrow = tk.Label(self.text_frame, text="⬇", font=self.font_unit, fg="lime", bg="black")
        self._pack_row(self.lbl_down_arrow, self.lbl_down_val, self.lbl_down_unit)

        # Upload row
        self.lbl_up_val = tk.Label(self.text_frame, text="0.00", font=self.font_value, fg="cyan", bg="black")
        self.lbl_up_unit = tk.Label(self.text_frame, text="Mb/s", font=self.font_unit, fg="#ECF8F8", bg="black")
        self.lbl_up_arrow = tk.Label(self.text_frame, text="⬆", font=self.font_unit, fg="cyan", bg="black")
        self._pack_row(self.lbl_up_arrow, self.lbl_up_val, self.lbl_up_unit)

        self.canvas = tk.Canvas(
            self.main_frame,
            width=self.graph_width,
            height=self.graph_height,
            bg="black",
            highlightthickness=0
        )
        self.canvas.pack(side="right", padx=(4, 6), pady=4)

        # Window Geometry (bottom-right corner)
        self.root.update_idletasks()
        work_area = win32api.GetMonitorInfo(win32api.MonitorFromPoint((0, 0)))['Work']
        _, _, right, bottom = work_area
        total_width = self.text_frame.winfo_reqwidth() + self.graph_width + 16

        # Store values, for cursor pointer
        self.win_width = total_width
        self.win_height = self.height
        self.win_x = right - total_width
        self.win_y = bottom - self.height
        self.root.geometry(f"{self.win_width}x{self.win_height}+{self.win_x}+{self.win_y}")

        # Counters
        counters = psutil.net_io_counters()
        self.last_bytes_sent = counters.bytes_sent
        self.last_bytes_recv = counters.bytes_recv

        # Series (10 points)
        self.upload_speeds: list[float] = []
        self.download_speeds: list[float] = []
        # ping loss flags aligned to samples; True means the segment ending at i is a drop
        self.ping_loss: list[bool] = []

        # Updater Thread
        self._run = True
        threading.Thread(target=self.update_loop, daemon=True).start()

        # No blink on hover
        self._hover_guard_active = False
        self.root.bind("<Enter>", self._on_mouse_enter)

        # Clean Exit
        self.root.protocol("WM_DELETE_WINDOW", self._on_close)

    def _pack_row(self, *widgets: tk.Widget):
        row = tk.Frame(self.text_frame, bg="black")
        row.pack(anchor="w")
        for w in widgets:
            w.master = row
            w.pack(side="left")

    def _on_mouse_enter(self, _event=None):
        if not self._hover_guard_active:
            self._hover_guard_active = True
            self.root.withdraw()
            self._poll_cursor_and_restore()

    def _poll_cursor_and_restore(self):
        if not self._hover_guard_active:
            return

        # Current global cursor position
        x, y = win32api.GetCursorPos()
        inside = (self.win_x <= x <= self.win_x + self.win_width and
                  self.win_y <= y <= self.win_y + self.win_height)
        if inside:
            # keeps waiting, then retry
            self.root.after(120, self._poll_cursor_and_restore)
        else:
            # restore once pointer is outside
            self.root.deiconify()
            self._hover_guard_active = False

    def _ping_once(self) -> bool:
        cmd = ["ping", PING_HOST, "-n", "1", "-w", str(PING_TIMEOUT_MS)]
        try:
            res = subprocess.run(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=0x08000000,
                check=False
            )
            return res.returncode == 0
        except Exception:
            return False

    def update_loop(self):
        while self._run:
            start = time.time()
            counters = psutil.net_io_counters()
            new_sent = counters.bytes_sent
            new_recv = counters.bytes_recv

            up_mbps = (new_sent - self.last_bytes_sent) * 8.0 / 1_000_000.0
            down_mbps = (new_recv - self.last_bytes_recv) * 8.0 / 1_000_000.0

            self.last_bytes_sent = new_sent
            self.last_bytes_recv = new_recv

            # Ping for this tick
            ok = self._ping_once()
            dropped = (not ok)

            # Append Series
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

    def draw_graph(self):
        self.canvas.delete("all")

        # Base scale on max of both series
        max_speed = max(self.download_speeds + self.upload_speeds + [1.0])

        def draw_line(data, loss_flags, base_color, offset_y):
            n = len(data)
            if n < 2:
                return

            half = self.graph_height // 2

            # Map points
            pts = []
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

        # Download and Upload line
        draw_line(self.download_speeds, self.ping_loss, "lime", 0)
        draw_line(self.upload_speeds,   self.ping_loss, "cyan", self.graph_height // 2)

    def _on_close(self):
        self._run = False
        self._hover_guard_active = False
        self.root.destroy()

    def ui_call(self, func, *args, **kwargs):
        self.root.after(0, lambda: func(*args, **kwargs))

    def show_window(self):
        self.root.deiconify()
        self.root.attributes("-topmost", True)  # keep floating

    def hide_window(self):
        self.root.withdraw()

if __name__ == "__main__":
    root = tk.Tk()
    app = NetSpeedWidget(root)

    # Start system tray (separate thread)
    tray = TrayController(app, APP_NAME)
    tray.start()

    root.mainloop()
