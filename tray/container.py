import threading
from pystray import Menu, MenuItem, Icon
from PIL import Image
from utils import paths

ICON_FILE = "icon.ico"

class TrayController:

    def __init__(self, app, app_name):
        self.app = app
        self.app_name = app_name
        self.icon = None
        self.thread = None

    def start(self):
        img = Image.open(paths.resource_path(ICON_FILE))
        menu = Menu(
            MenuItem("Show", lambda *_: self.app.ui_call(self.app.show_window)),
            MenuItem("Hide", lambda *_: self.app.ui_call(self.app.hide_window)),
            MenuItem("Quit", self.on_quit),
        )
        self.icon = Icon(self.app_name, img, self.app_name, menu)

        # Run tray on its own thread
        self.thread = threading.Thread(target=self.icon.run, daemon=True)
        self.thread.start()

    def on_quit(self, *_):
        # Stop tray then exit app (from Tk main thread)
        if self.icon:
            self.icon.stop()
        self.app.ui_call(self.app.root.destroy)