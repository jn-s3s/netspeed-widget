"""System tray integration built on pystray.

The icon runs on a daemon thread so the Tk main loop stays free. Menu actions
are forwarded to the app through `ui_call`, except `set_opacity` and `set_theme`,
which marshal themselves; the app is reached only through the `WidgetActions` protocol
below, never through its window, so the tray cannot touch Tk state it does
not own.

pystray mutates its icon from three threads: the icon thread, the Tk
thread pushing live status and the speedtest worker pushing a result. One
lock guards the icon reference so a quit cannot make a refresh helper
dereference an icon that has just been dropped.
"""

import threading
from collections.abc import Callable
from typing import Protocol

from PIL import Image, UnidentifiedImageError
from pystray import Icon, Menu, MenuItem

from utils.config import OPACITY_LEVELS, get_hide_on_hover, get_opacity, get_theme
from utils.logger import get
from utils.paths import icon_path
from utils.theme import THEMES

_log = get("tray")


class WidgetActions(Protocol):
    """The app surface the tray menu is allowed to drive."""

    def ui_call(
        self, func: Callable[..., None], *args: object, **kw: object
    ) -> None: ...
    def show_window(self) -> None: ...
    def hide_window(self) -> None: ...
    def reset_position(self) -> None: ...
    def toggle_hover_hide(self) -> None: ...
    def open_hotkey_dialog(self) -> None: ...
    def run_speedtest_now(self, manual: bool = ...) -> None: ...
    def set_opacity(self, value: float) -> None: ...
    def set_theme(self, name: str) -> None: ...
    def shutdown(self) -> None: ...


class TrayController:
    """Owns the pystray icon and exposes app actions as menu items."""

    def __init__(self, app: WidgetActions, app_name: str) -> None:
        """Store the app reference and the status the tooltip will show.

        Args:
            app: The widget instance, used only through WidgetActions.
            app_name: Display name for the tooltip and menu title.
        """
        self.app = app
        self.app_name = app_name
        self._icon: Icon | None = None
        self._icon_lock = threading.Lock()
        self.thread: threading.Thread | None = None
        self._speedtest_check = False
        self._speedtest_summary = ""
        self._live_status = ""

    @property
    def icon(self) -> Icon | None:
        """The live pystray icon, or None once the tray has stopped."""
        with self._icon_lock:
            return self._icon

    def start(self) -> None:
        """Create the tray icon and run it on a daemon thread."""
        try:
            image = self._load_icon()
        except (FileNotFoundError, UnidentifiedImageError, OSError) as err:
            _log.warning(f"tray icon image unusable, using a blank placeholder: {err}")
            image = Image.new("RGBA", (16, 16), (0, 0, 0, 0))

        self._icon = Icon(self.app_name, image, self.app_name, self._build_menu())
        self.thread = threading.Thread(target=self._run_icon, daemon=True)
        self.thread.start()
        _log.info("system tray started")

    def _build_menu(self) -> Menu:
        """Assemble the tray menu with its dynamic status and checked states."""
        return Menu(
            MenuItem(lambda *_: self._menu_status_text(), None, enabled=False),
            MenuItem(
                "Check speedtest",
                self._on_check_speedtest,
                enabled=lambda *_: not self._speedtest_check,
            ),
            self._theme_submenu(),
            self._opacity_submenu(),
            MenuItem(
                "Auto-hide on hover",
                self._on_toggle_hover_hide,
                checked=lambda *_: get_hide_on_hover(),
            ),
            MenuItem("Change hotkey...", self._on_change_hotkey),
            MenuItem("Reset position", self._on_reset_position),
            MenuItem("Show", lambda *_: self.app.ui_call(self.app.show_window)),
            MenuItem("Hide", lambda *_: self.app.ui_call(self.app.hide_window)),
            MenuItem("Quit", self.on_quit),
        )

    def _run_icon(self) -> None:
        """Run the pystray message loop, reporting why it exited early."""
        icon = self.icon
        if icon is None:
            return
        try:
            icon.run()
        except Exception as err:  # noqa: BLE001 - the tray is not worth a crash
            _log.error(f"tray loop exited unexpectedly: {err}")
            with self._icon_lock:
                if self._icon is icon:
                    self._icon = None

    def on_quit(self, *_: object) -> None:
        """Stop the tray icon and shut the app down on the Tk thread."""
        _log.info("quit requested")
        self.stop()
        self.app.ui_call(self.app.shutdown)

    def stop(self) -> None:
        """Stop the tray icon once, removing it from the notification area.

        Every quit path funnels through here so none can leave a ghost
        icon or a live menu wired to a destroyed Tk root. Dropping the
        reference also turns the refresh helpers into no-ops afterwards.
        """
        with self._icon_lock:
            icon, self._icon = self._icon, None
        if icon is not None:
            icon.stop()

    def update_speedtest_summary(self, summary: str) -> None:
        """Set the speedtest line shown in the tooltip and menu."""
        self._speedtest_summary = summary or ""
        self._refresh_title()
        self._refresh_menu()

    def update_live_status(self, status: str) -> None:
        """Set the live speeds line shown in the tooltip."""
        self._live_status = status or ""
        self._refresh_title()

    def update_settings(self) -> None:
        """Refresh checked menu items after settings change in the app."""
        self._refresh_menu()

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
        """Load the tray icon image. Raises OSError if it cannot be read."""
        with Image.open(icon_path()) as image:
            image.load()
            return image.copy()

    def _menu_status_text(self) -> str:
        """Compose the disabled first menu line."""
        lines = [self._speedtest_summary, self._live_status]
        return " | ".join(line for line in lines if line) or "No data yet"

    def _refresh_title(self) -> None:
        """Push the combined status into the tray tooltip."""
        icon = self.icon
        if icon is None:
            return
        lines = [self.app_name, self._speedtest_summary, self._live_status]
        icon.title = "\n".join(line for line in lines if line)

    def _refresh_menu(self) -> None:
        """Ask pystray to re-evaluate dynamic menu items."""
        icon = self.icon
        if icon is not None:
            icon.update_menu()

    def _opacity_submenu(self) -> MenuItem:
        """Build the opacity submenu with radio-style checkmarks."""
        return MenuItem(
            "Opacity",
            Menu(
                *(
                    MenuItem(
                        _opacity_label(level),
                        self._on_set_opacity(level),
                        checked=lambda *_a, lvl=level: abs(get_opacity() - lvl) < 1e-6,
                    )
                    for level in OPACITY_LEVELS
                )
            ),
        )

    def _theme_submenu(self) -> MenuItem:
        """Build theme choices with a live check on the persisted selection."""
        return MenuItem(
            "Theme",
            Menu(
                *(
                    MenuItem(
                        name,
                        self._on_set_theme(name),
                        checked=lambda *_a, theme=name: get_theme() == theme,
                    )
                    for name in THEMES
                )
            ),
        )

    def _on_set_theme(self, name: str):
        """Return the handler that asks the app to switch themes safely."""
        return lambda *_a: self.app.set_theme(name)

    def _on_set_opacity(self, level: float):
        """Return the handler that asks the app to change opacity.

        `set_opacity` marshals itself, so it is safe to call from here.
        """
        return lambda *_a: self.app.set_opacity(level)

    def _on_toggle_hover_hide(self, *_: object) -> None:
        """Forward the hover-hide toggle to the app."""
        self.app.ui_call(self.app.toggle_hover_hide)

    def _on_reset_position(self, *_: object) -> None:
        """Move the widget back to the default corner."""
        self.app.ui_call(self.app.reset_position)

    def _on_change_hotkey(self, *_: object) -> None:
        """Open the hotkey capture dialog on the Tk thread."""
        self.app.ui_call(self.app.open_hotkey_dialog)

    def _on_check_speedtest(self, *_: object) -> None:
        """Trigger a manual speedtest from the tray menu."""
        self.app.ui_call(self.app.run_speedtest_now, manual=True)


def _opacity_label(level: float) -> str:
    """Menu label for an opacity level, shared with the widget menu."""
    return f"{int(level * 100)}%"
