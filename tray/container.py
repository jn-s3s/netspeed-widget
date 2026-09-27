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
import time
from collections.abc import Callable
from typing import Protocol

from PIL import Image, UnidentifiedImageError
from pystray import Icon, Menu, MenuItem

from utils.config import OPACITY_LEVELS, get_hide_on_hover, get_opacity, get_theme
from utils.logger import get
from utils.menu_labels import (
    auto_hide_label,
    hotkey_label,
    opacity_choice_text,
    opacity_percent_label,
    session_label,
    speedtest_label,
)
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

    @property
    def session_totals(self) -> tuple[float, float]:
        """Cumulative transfer this session.

        Lock-protected in NetSampler; safe to read from any thread.
        """
        ...

    @property
    def hotkey_combo(self) -> str:
        """Active hotkey combo.

        Set on Tk thread; GIL-atomic read from tray thread
        (may lag one update).
        """
        ...

    @property
    def hotkey_ok(self) -> bool:
        """Whether the hotkey is registered.

        Set on Tk thread; GIL-atomic read from tray thread.
        """
        ...

    @property
    def speedtest_summary(self) -> str:
        """Last speedtest line.

        Set on Tk thread; GIL-atomic read from tray thread
        (may lag one update).
        """
        ...


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
        self._last_session_refresh: float = 0.0

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
        """Assemble the tray menu mirroring the widget's context menu."""
        return Menu(
            MenuItem(
                "Run speedtest",
                self._on_run_speedtest,
                enabled=lambda *_: not self._speedtest_check,
            ),
            MenuItem(self._session_label, None, enabled=False),
            MenuItem(self._last_speedtest_label, None, enabled=False),
            self._theme_submenu(),
            self._opacity_submenu(),
            MenuItem(self._auto_hide_label, self._on_toggle_hover_hide),
            MenuItem(self._hotkey_label, self._on_change_hotkey),
            Menu.SEPARATOR,
            MenuItem("Reset position", self._on_reset_position),
            MenuItem("Hide", lambda *_: self.app.ui_call(self.app.hide_window)),
            MenuItem("Quit", self.on_quit),
        )

    def _session_label(self, _item: MenuItem) -> str:
        """Live menu line with this session's cumulative transfer."""
        down_mb, up_mb = self.app.session_totals
        return session_label(down_mb, up_mb)

    def _last_speedtest_label(self, _item: MenuItem) -> str:
        """Menu line with the last speedtest, like the widget's second row."""
        return speedtest_label(self.app.speedtest_summary)

    def _auto_hide_label(self, _item: MenuItem) -> str:
        """Auto-hide label carrying the same text checkmark as the widget."""
        return auto_hide_label(get_hide_on_hover())

    def _hotkey_label(self, _item: MenuItem) -> str:
        """Hotkey action label, matching the widget's entry state."""
        return hotkey_label(self.app.hotkey_ok, self.app.hotkey_combo)

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
        """Set the live speeds line and periodically refresh session totals."""
        self._live_status = status or ""
        self._refresh_title()
        now = time.monotonic()
        if now - self._last_session_refresh >= 30.0:
            self._last_session_refresh = now
            self._refresh_menu()

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
            lambda _item: opacity_percent_label(get_opacity()),
            Menu(
                *(
                    MenuItem(
                        opacity_choice_text(level),
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

    def _on_run_speedtest(self, *_: object) -> None:
        """Trigger a manual speedtest from the tray menu."""
        self.app.ui_call(self.app.run_speedtest_now, manual=True)
