import tkinter as tk
import psutil
import threading
import time

class NetSpeedWidget:

    def __init__(self, root):
        self.root = root
        self.root.title("NetSpeed Widget by jn-s3s")
        screen_height = self.root.winfo_screenheight()
        widget_height = 50
        y_pos = screen_height - widget_height

        self.root.geometry(f"130x{widget_height}+0+{y_pos}")
        self.root.configure(bg="black")
        self.root.overrideredirect(True)
        self.root.attributes("-topmost", True)

        self.label = tk.Label(root, text="⬇ 0.00 Mbps\n⬆ 0.00 Mbps", font=("Segoe UI", 10), fg="lime", bg="black", justify="left")
        self.label.pack(padx=5, pady=5)

        self.last_bytes_sent = psutil.net_io_counters().bytes_sent
        self.last_bytes_recv = psutil.net_io_counters().bytes_recv

        self.update_speed()

    def update_speed(self):
        def run():
            while True:
                time.sleep(1)
                counters = psutil.net_io_counters()
                new_sent = counters.bytes_sent
                new_recv = counters.bytes_recv

                upload_speed = (new_sent - self.last_bytes_sent) * 8 / 1_000_000
                download_speed = (new_recv - self.last_bytes_recv) * 8 / 1_000_000

                self.last_bytes_sent = new_sent
                self.last_bytes_recv = new_recv

                speed_text = f"⬇ {download_speed:.2f} Mbps\n⬆ {upload_speed:.2f} Mbps"
                self.label.config(text=speed_text)

        threading.Thread(target=run, daemon=True).start()

# Run app
if __name__ == "__main__":
    root = tk.Tk()
    app = NetSpeedWidget(root)
    root.mainloop()
