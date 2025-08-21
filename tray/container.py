import threading
from typing import Any, Optional

from PIL import Image, UnidentifiedImageError
from pystray import Icon, Menu, MenuItem

from utils import paths

ICON_FILE = "icon.ico"


class TrayController:
    """
    System tray controller that owns a pystray Icon instance, runs it on a
    background thread, and exposes actions to show/hide/quit the Tk app.
    """

    def __init__(self, app: Any, app_name: str) -> None:
        """
        Args:
            app: Reference to the main Tk application instance.
            app_name: Display name for the tray icon's tooltip and menu.
        """
        self.app = app
        self.app_name = app_name
        self.icon: Optional[Any] = None
        self.thread: Optional[threading.Thread] = None
        self.app.root.iconbitmap(paths.resource_path(ICON_FILE))

    def _load_icon(self) -> Image.Image:
        """
        Load the tray icon image from bundled resources.
        """
        icon_path = paths.resource_path(ICON_FILE)
        return Image.open(icon_path)

    def start(self) -> None:
        """
        Create and run the system tray icon on a daemon thread.
        """
        try:
            img = self._load_icon()
        except (FileNotFoundError, UnidentifiedImageError):
            # Fallback: create a tiny 16x16 blank image if icon is missing/broken
            img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))

        menu = Menu(
            MenuItem("Show", lambda *_: self.app.ui_call(self.app.show_window)),
            MenuItem("Hide", lambda *_: self.app.ui_call(self.app.hide_window)),
            MenuItem("Quit", self.on_quit),
        )
        self.icon = Icon(self.app_name, img, self.app_name, menu)

        # Run tray on its own thread so it doesn't block Tk's mainloop
        self.thread = threading.Thread(target=self.icon.run, daemon=True)
        self.thread.start()

    def on_quit(self, *_: Any) -> None:
        """
        Stop the tray icon and close the Tk application.
        This is safe to call from the tray thread.
        """
        if self.icon:
            self.icon.stop()
        # Ensure Tk shutdown runs on its own main thread
        self.app.ui_call(self.app.root.destroy)
