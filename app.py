import tkinter as tk
import psutil
import threading
import time
import win32api

class NetSpeedWidget:

    def __init__(self, root):
        self.root = root
        self.root.title("NetSpeed Widget by jn-s3s")
        self.root.configure(bg="black")
        self.root.overrideredirect(True)
        self.root.attributes("-topmost", True)
        self.root.attributes("-alpha", 0.7)

        work_area = win32api.GetMonitorInfo(win32api.MonitorFromPoint((0, 0)))['Work']
        _, _, right, bottom = work_area

        self.width = 240
        self.height = 45
        self.graph_width = 130
        self.graph_height = (self.height - 10)

        x_pos = (right - self.width)
        y_pos = (bottom - self.height)
        self.root.geometry(f"{self.width}x{self.height}+{x_pos}+{y_pos}")

        self.main_frame = tk.Frame(root, bg="black")
        self.main_frame.pack(fill="both", expand=True)

        self.label = tk.Label(
            self.main_frame,
            text="⬇ 0.00 Mbps\n⬆ 0.00 Mbps",
            font=("Segoe UI", 8),
            fg="lime",
            bg="black",
            justify="left"
        )
        self.label.pack(side="left", padx=5, pady=5)

        self.canvas = tk.Canvas(
            self.main_frame,
            width=self.graph_width,
            height=self.graph_height,
            bg="black",
            highlightthickness=0
        )
        self.canvas.pack(side="right", padx=5, pady=5)

        counters = psutil.net_io_counters()
        self.last_bytes_sent = counters.bytes_sent
        self.last_bytes_recv = counters.bytes_recv
        self.last_dropin = counters.dropin
        self.last_dropout = counters.dropout

        self.download_speeds = []
        self.upload_speeds = []
        self.download_drops = []
        self.upload_drops = []

        self.update_speed()

        self.root.bind("<Enter>", self.on_mouse_enter)
        self.root.bind("<Leave>", self.on_mouse_leave)

    def on_mouse_enter(self, event):
        self.root.withdraw()

    def on_mouse_leave(self, event):
        self.root.deiconify()

    def update_speed(self):
        def run():
            while True:
                time.sleep(1)
                counters = psutil.net_io_counters()
                new_sent = counters.bytes_sent
                new_recv = counters.bytes_recv
                new_dropin = counters.dropin
                new_dropout = counters.dropout

                upload_speed = (new_sent - self.last_bytes_sent) * 8 / 1_000_000
                download_speed = (new_recv - self.last_bytes_recv) * 8 / 1_000_000

                drop_download = (new_dropin > self.last_dropin)
                drop_upload = (new_dropout > self.last_dropout)

                self.last_bytes_sent = new_sent
                self.last_bytes_recv = new_recv
                self.last_dropin = new_dropin
                self.last_dropout = new_dropout

                self.upload_speeds.append(upload_speed)
                self.download_speeds.append(download_speed)
                self.upload_drops.append(drop_upload)
                self.download_drops.append(drop_download)

                if (len(self.upload_speeds) > 10):
                    self.upload_speeds.pop(0)
                    self.download_speeds.pop(0)
                    self.upload_drops.pop(0)
                    self.download_drops.pop(0)

                self.label.config(
                    text=f"⬇ {download_speed:.2f} Mbps\n⬆ {upload_speed:.2f} Mbps"
                )

                self.draw_graph()

        threading.Thread(target=run, daemon=True).start()

    def draw_graph(self):
        self.canvas.delete("all")
        max_speed = max(self.download_speeds + self.upload_speeds + [1])

        def draw_line(data, drops, color, offset_y):
            points = []
            half = (self.graph_height // 2)
            for i, val in enumerate(data):
                x = (i * (self.graph_width / 9))
                y = (half - (val / max_speed) * (half - 2))
                y += offset_y
                points.append((x, y))

            for i in range(1, len(points)):
                prev = points[i - 1]
                curr = points[i]
                line_color = "red" if drops[i] else color
                self.canvas.create_line(prev[0], prev[1], curr[0], curr[1], fill=line_color, width=2)

        draw_line(self.download_speeds, self.download_drops, "lime", offset_y=0)
        draw_line(self.upload_speeds, self.upload_drops, "cyan", offset_y=self.graph_height // 2)

if __name__ == "__main__":
    root = tk.Tk()
    app = NetSpeedWidget(root)
    root.mainloop()
