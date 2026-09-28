"""Tray menu tests: action routing, icon fallback and post-shutdown no-ops.

The real pystray Menu and MenuItem objects are used rather than mocks, so an
assertion reads what the menu actually offers instead of what a stub returned.
"""

from unittest.mock import MagicMock

import pytest
from PIL import UnidentifiedImageError
from pystray import Menu

from tray.container import TrayController
from utils import config
from utils.menu_labels import opacity_choice_text
from utils.theme import THEMES


@pytest.fixture
def app():
    tray_state = MagicMock()
    # Attribute values the dynamic menu labels read through WidgetActions.
    tray_state.session_totals = (0.0, 0.0)
    tray_state.speedtest_summary = ""
    tray_state.hotkey_ok = False
    tray_state.hotkey_combo = ""
    return tray_state


@pytest.fixture
def tray(app):
    return TrayController(app, "NetSpeed Widget")


@pytest.fixture
def no_display_icon(monkeypatch):
    """Stop pystray from touching a real notification area."""
    icon = MagicMock()
    icon.run.side_effect = lambda: None
    monkeypatch.setattr("tray.container.Icon", icon)
    return icon


class TestStartupResilience:
    def test_building_the_tray_never_reaches_through_the_app_window(
        self, app, monkeypatch
    ):
        """The old constructor called app.root.iconbitmap, which could abort startup."""
        monkeypatch.setattr("tray.container.Icon", MagicMock())

        TrayController(app, "App")

        assert not app.root.method_calls

    def test_an_unreadable_icon_falls_back_to_a_placeholder(
        self, tray, no_display_icon, monkeypatch
    ):
        def refuse(*_a, **_k):
            raise UnidentifiedImageError("truncated ico")

        monkeypatch.setattr("tray.container.Image.open", refuse)

        tray.start()

        image = no_display_icon.call_args.args[1]
        assert image.size == (16, 16)
        tray.stop()

    def test_a_missing_icon_file_also_falls_back(
        self, tray, no_display_icon, monkeypatch
    ):
        monkeypatch.setattr(
            "tray.container.icon_path",
            lambda: (_ for _ in ()).throw(FileNotFoundError()),
        )

        tray.start()

        assert no_display_icon.call_args.args[1].size == (16, 16)
        tray.stop()


class TestMenuRouting:
    def test_every_window_action_is_marshalled_to_the_tk_thread(self, tray, app):
        tray._on_reset_position()
        app.ui_call.assert_called_once_with(app.reset_position)
        app.ui_call.reset_mock()

        tray._on_toggle_hover_hide()
        app.ui_call.assert_called_once_with(app.toggle_hover_hide)
        app.ui_call.reset_mock()

        tray._on_change_hotkey()
        app.ui_call.assert_called_once_with(app.open_hotkey_dialog)
        app.ui_call.reset_mock()

        tray._on_run_speedtest()
        app.ui_call.assert_called_once_with(app.run_speedtest_now, manual=True)

    def test_opacity_is_applied_directly_because_it_marshals_itself(self, tray, app):
        tray._on_set_opacity(0.8)()

        assert app.set_opacity.call_args.args == (0.8,)

    def test_the_opacity_menu_offers_the_shared_levels(self, tray, monkeypatch):
        monkeypatch.setattr("tray.container.get_opacity", lambda: 1.0)

        submenu = tray._opacity_submenu()
        labels = [item.text for item in submenu.submenu.items]

        assert labels == [opacity_choice_text(level) for level in config.OPACITY_LEVELS]
        assert labels[0] == "100%"

    def test_exactly_one_opacity_entry_is_checked(self, tray, monkeypatch):
        """The tray reads the level through its own imported getter."""
        monkeypatch.setattr("tray.container.get_opacity", lambda: 0.8)

        submenu = tray._opacity_submenu()
        checked = [item.text for item in submenu.submenu.items if item.checked]

        assert checked == ["80%"]

    def test_theme_submenu_offers_presets_with_live_selected_state(
        self, tray, monkeypatch
    ):
        selected = ["Ocean"]
        monkeypatch.setattr("tray.container.get_theme", lambda: selected[0])

        theme_item = next(
            item for item in tray._build_menu().items if item.text == "Theme"
        )
        assert theme_item.text == "Theme"
        assert [item.text for item in theme_item.submenu.items] == list(THEMES)

        submenu = tray._theme_submenu().submenu
        assert [item.text for item in submenu.items] == list(THEMES)
        assert [item.text for item in submenu.items if item.checked] == ["Ocean"]

        selected[0] = "Light"
        assert [item.text for item in submenu.items if item.checked] == ["Light"]

    def test_theme_selection_dispatches_through_app_api(self, tray, app):
        tray._on_set_theme("Aurora")()

        app.set_theme.assert_called_once_with("Aurora")

    def test_the_speedtest_item_disables_itself_while_one_runs(self, tray):
        def speedtest_item():
            return next(
                item
                for item in tray._build_menu().items
                if item.text == "Run speedtest"
            )

        assert speedtest_item().enabled is True

        tray.start_speedtest_check()
        item = speedtest_item()

        assert item.enabled is False
        assert tray._speedtest_check is True

        tray.stop_speedtest_check()
        assert speedtest_item().enabled is True

    def test_quit_stops_the_tray_then_shuts_the_app_down(self, tray, app):
        tray._icon = MagicMock()

        tray.on_quit()

        app.ui_call.assert_called_once_with(app.shutdown)

    def test_the_menu_mirrors_the_widget_context_menu(self, tray, monkeypatch):
        """Item order, labels and separators match the widget menu exactly."""
        monkeypatch.setattr("tray.container.get_hide_on_hover", lambda: False)
        monkeypatch.setattr("tray.container.get_opacity", lambda: 1.0)

        menu = tray._build_menu()
        labels = [item.text for item in menu.items if item is not Menu.SEPARATOR]

        assert labels == [
            "Run speedtest",
            "Session: 0.0 MB down, 0.0 MB up",
            "Last speedtest: --",
            "Theme",
            "Opacity (100%)",
            "Auto-hide on hover",
            "Set hotkey (none active)",
            "Reset position",
            "Hide",
            "Quit",
        ]
        assert Menu.SEPARATOR in menu.items

    def test_hotkey_label_tracks_registration_state(self, tray, app):
        app.hotkey_ok = True
        app.hotkey_combo = "ctrl+shift+n"

        item = tray._build_menu().items[6]

        assert item.text == "Change hotkey (Ctrl+Shift+N)"

        app.hotkey_ok = False
        assert tray._build_menu().items[6].text == "Set hotkey (none active)"

    def test_auto_hide_label_carries_the_text_checkmark(self, tray, monkeypatch):
        monkeypatch.setattr("tray.container.get_hide_on_hover", lambda: True)
        assert tray._build_menu().items[5].text == "✓ Auto-hide on hover"

        monkeypatch.setattr("tray.container.get_hide_on_hover", lambda: False)
        assert tray._build_menu().items[5].text == "Auto-hide on hover"

    def test_opacity_submenu_label_shows_the_current_percent(self, tray, monkeypatch):
        monkeypatch.setattr("tray.container.get_opacity", lambda: 0.8)

        assert tray._opacity_submenu().text == "Opacity (80%)"


class TestStatusDisplay:
    def test_session_line_reads_the_app_totals(self, tray, app):
        app.session_totals = (123.4, 56.7)

        item = tray._build_menu().items[1]

        assert item.text == "Session: 123.4 MB down, 56.7 MB up"
        assert item.enabled is False

    def test_last_speedtest_line_falls_back_until_a_result_exists(self, tray, app):
        app.speedtest_summary = ""

        item = tray._build_menu().items[2]

        assert item.text == "Last speedtest: --"
        assert item.enabled is False

    def test_last_speedtest_line_shows_the_latest_summary(self, tray, app):
        app.speedtest_summary = "Last speedtest: 88.4 D | 21.7 U Mb/s"

        item = tray._build_menu().items[2]

        assert item.text == "Last speedtest: 88.4 D | 21.7 U Mb/s"

    def test_the_tooltip_leads_with_the_app_name(self, tray):
        icon = MagicMock()
        tray._icon = icon

        tray.update_live_status("D 1.0 Mb/s")

        assert icon.title.splitlines()[0] == "NetSpeed Widget"
        assert "D 1.0 Mb/s" in icon.title

    def test_refresh_is_safe_once_the_icon_is_gone(self, tray):
        icon = MagicMock()
        tray._icon = icon
        tray.stop()

        tray.update_live_status("D 2.0 Mb/s")
        tray.update_speedtest_summary("Speedtest: 1.0 D")
        tray.stop_speedtest_check()
        tray._refresh_title()
        tray._refresh_menu()

        assert tray.icon is None

    def test_stop_is_idempotent(self, tray):
        icon = MagicMock()
        tray._icon = icon

        tray.stop()
        tray.stop()

        icon.stop.assert_called_once()
