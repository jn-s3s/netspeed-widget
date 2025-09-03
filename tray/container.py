import threading
from typing import Any, Optional

from PIL import Image, UnidentifiedImageError
from pystray import Icon, Menu, MenuItem

from utils import paths
from utils.config import get_opacity
from utils.logger import info

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
        self._speedtest_check: bool = False
        self._speedtest_summary: str = ""


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
            MenuItem(lambda *_: self._menu_status_text(), None, enabled=False),
            MenuItem("Check speedtest", self._on_check_speedtest, enabled=lambda *_: not self._speedtest_check),
            self._opacity_submenu(),
            MenuItem("Show", lambda *_: self.app.ui_call(self.app.show_window)),
            MenuItem("Hide", lambda *_: self.app.ui_call(self.app.hide_window)),
            MenuItem("Quit", self.on_quit),
        )
        self.icon = Icon(self.app_name, img, self.app_name, menu)

        # Run tray on its own thread so it doesn't block Tk's mainloop
        self.thread = threading.Thread(target=self.icon.run, daemon=True)
        self.thread.start()
        info("[TRAY] System tray started")


    def on_quit(self, *_: Any) -> None:
        """
        Stop the tray icon and close the Tk application.
        This is safe to call from the tray thread.
        """
        info("[TRAY] Quit requested")
        if self.icon:
            self.icon.stop()
        # Ensure Tk shutdown runs on its own main thread
        self.app.ui_call(self.app.root.destroy)


    def _opacity_levels(self):
        """
        Returns the list of opacity levels to show in the submenu.
        """
        return [1.00, 0.90, 0.85, 0.80, 0.72, 0.60, 0.50, 0.45, 0.40]


    def _opacity_checked(self, level: float):
        """
        Pystray 'checked' callback that marks the current level.
        """
        def _checked(_item):
            try:
                return abs(get_opacity() - level) < 0.01
            except Exception:
                return False
        return _checked


    def _make_set_opacity(self, level: float):
        """
        Returns a handler that sets opacity via the Tk app and refreshes the tray menu.
        """
        def _handler(_icon=None, _item=None):
            # Prefer the app API if available to keep UI thread safe and persist config
            if hasattr(self.app, "set_opacity"):
                self.app.set_opacity(level)
                info(f"[TRAY] Opacity chosen {level:.2f}")
            # After change, refresh menu so the check moves
            try:
                if hasattr(self.icon, "update_menu"):
                    self.icon.update_menu()
            except Exception:
                pass
        return _handler


    def _opacity_submenu(self):
        """
        Builds the 'Opacity' submenu with radio-like items.
        """
        items = []
        for lvl in self._opacity_levels():
            label = f"{int(lvl * 100)}%"
            items.append(
                MenuItem(
                    label,
                    self._make_set_opacity(lvl),
                    checked=self._opacity_checked(lvl),
                )
            )
        return MenuItem("Opacity", Menu(*items))


    def update_speedtest_summary(self, summary: str) -> None:
        """
        Sets a short summary that appears in the tray title.
        """
        self._speedtest_summary = summary or ""
        try:
            if self.icon:
                base = self.app_name
                title = f"{base}\n{self._speedtest_summary}" if self._speedtest_summary else base
                self.icon.title = title
                self.icon.update_menu()
        except Exception:
            pass


    def start_speedtest_check(self) -> None:
        """
        Marks speedtest as running.
        """
        if self._speedtest_check:
            return
        self._speedtest_check = True
        try:
            self.update_speedtest_summary("Speedtest is running...")
            if getattr(self, "icon", None):
                self.icon.update_menu()
        except Exception:
            pass


    def stop_speedtest_check(self) -> None:
        """
        Clears running flag and refreshes the menu.
        The title will already hold the latest summary pushed by the app.
        """
        self._speedtest_check = False
        try:
            if getattr(self, "icon", None):
                self.icon.update_menu()
        except Exception:
            pass


    def _on_check_speedtest(self, *_: Any) -> None:
        """
        Tray action handler. Asks the app to run a speedtest now.
        """
        try:
            if hasattr(self.app, "run_speedtest_now"):
                self.app.run_speedtest_now(manual=True)
        except Exception:
            pass


    def _menu_status_text(self) -> str:
        return self._speedtest_summary or "Speedtest: --"