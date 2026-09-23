"""System tray integration built on pystray.

The icon runs on a daemon thread so the Tk main loop stays free. Menu
actions that touch Tk are forwarded to the app through `ui_call`.
"""

import threading
from typing import Any

from PIL import Image, UnidentifiedImageError
from pystray import Icon, Menu, MenuItem

from utils import paths
from utils.config import get_hide_on_hover, get_opacity
from utils.logger import info

ICON_FILE = "icon.ico"


class TrayController:
    """Owns the pystray icon and exposes app actions as menu items."""

    def __init__(self, app: Any, app_name: str) -> None:
        """Store the app reference and set the window icon.

        Args:
            app: The main NetSpeedWidget instance.
            app_name: Display name for the tooltip and menu.
        """
        self.app = app
        self.app_name = app_name
        self.icon: Icon | None = None
        self.thread: threading.Thread | None = None
        self.app.root.iconbitmap(paths.resource_path(ICON_FILE))
        self._speedtest_check = False
        self._speedtest_summary = ""
        self._live_status = ""

    def start(self) -> None:
        """Create the tray icon and run it on a daemon thread."""
        try:
            image = self._load_icon()
        except (FileNotFoundError, UnidentifiedImageError):
            image = Image.new("RGBA", (16, 16), (0, 0, 0, 0))

        menu = Menu(
            MenuItem(lambda *_: self._menu_status_text(), None, enabled=False),
            MenuItem(
                "Check speedtest",
                self._on_check_speedtest,
                enabled=lambda *_: not self._speedtest_check,
            ),
            self._opacity_submenu(),
            MenuItem(
                "Auto-hide on hover",
                self._on_toggle_hover_hide,
                checked=lambda *_: get_hide_on_hover(),
            ),
            MenuItem(
                "Change hotkey...",
                lambda *_: self.app.ui_call(self.app.open_hotkey_dialog),
            ),
            MenuItem("Reset position", self._on_reset_position),
            MenuItem("Show", lambda *_: self.app.ui_call(self.app.show_window)),
            MenuItem("Hide", lambda *_: self.app.ui_call(self.app.hide_window)),
            MenuItem("Quit", self.on_quit),
        )
        self.icon = Icon(self.app_name, image, self.app_name, menu)
        self.thread = threading.Thread(target=self.icon.run, daemon=True)
        self.thread.start()
        info("[TRAY] System tray started")

    def on_quit(self, *_: Any) -> None:
        """Stop the tray icon and shut the app down on the Tk thread."""
        info("[TRAY] Quit requested")
        if self.icon is not None:
            self.icon.stop()
        self.app.ui_call(self.app.shutdown)

    def update_speedtest_summary(self, summary: str) -> None:
        """Set the speedtest line shown in the tooltip and menu."""
        self._speedtest_summary = summary or ""
        self._refresh_title()
        self._refresh_menu()

    def update_live_status(self, status: str) -> None:
        """Set the live speeds line shown in the tooltip."""
        self._live_status = status or ""
        self._refresh_title()

    def start_speedtest_check(self) -> None:
        """Mark a speedtest as running and disable the menu action."""
        if self._speedtest_check:
            return
        self._speedtest_check = True
        self.update_speedtest_summary("Speedtest is running...")

    def stop_speedtest_check(self) -> None:
        """Clear the running flag and refresh the menu."""
        self._speedtest_check = False
        self._refresh_menu()

    # ---------- Internals ----------

    def _load_icon(self) -> Image.Image:
        """Load the tray icon. Raises FileNotFoundError if missing."""
        image_path = paths.resource_path(ICON_FILE)
        image = Image.open(image_path)
        image.load()
        return image

    def _menu_status_text(self) -> str:
        """Compose the disabled first menu line."""
        lines = [self._speedtest_summary, self._live_status]
        return " | ".join(line for line in lines if line) or "No data yet"

    def _refresh_title(self) -> None:
        """Push the combined status into the tray tooltip."""
        if self.icon is None:
            return
        lines = [self.app_name, self._speedtest_summary, self._live_status]
        self.icon.title = "\n".join(line for line in lines if line)

    def _refresh_menu(self) -> None:
        """Ask pystray to re-evaluate dynamic menu items."""
        if self.icon is not None:
            self.icon.update_menu()

    def _opacity_submenu(self) -> MenuItem:
        """Build the opacity submenu with radio-style checkmarks."""
        return MenuItem(
            "Opacity",
            Menu(
                *(
                    MenuItem(
                        f"{int(level * 100)}%",
                        self._make_set_opacity(level),
                        checked=lambda *_, lvl=level: abs(get_opacity() - lvl) < 1e-6,
                    )
                    for level in (1.0, 0.9, 0.8, 0.7, 0.6, 0.5)
                )
            ),
        )

    def _make_set_opacity(self, level: float) -> Any:
        """Wrap a menu click into an opacity change on the app."""
        return lambda *_: self.app.set_opacity(level)

    def _on_toggle_hover_hide(self, *_: Any) -> None:
        """Forward the hover-hide toggle to the app."""
        self.app.ui_call(self.app.toggle_hover_hide)

    def _on_reset_position(self, *_: Any) -> None:
        """Move the widget back to the default corner."""
        self.app.ui_call(self.app.reset_position)

    def _on_check_speedtest(self, *_: Any) -> None:
        """Trigger a manual speedtest from the tray menu."""
        self.app.ui_call(self.app.run_speedtest_now, manual=True)
