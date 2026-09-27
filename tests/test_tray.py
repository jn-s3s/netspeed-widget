"""Tray menu tests: action routing, icon fallback and post-shutdown no-ops.

The real pystray Menu and MenuItem objects are used rather than mocks, so an
assertion reads what the menu actually offers instead of what a stub returned.
"""

from unittest.mock import MagicMock

import pytest
from PIL import UnidentifiedImageError

from tray.container import TrayController, _opacity_label
from utils import config
from utils.theme import THEMES


@pytest.fixture
def app():
    return MagicMock()


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

        tray._on_check_speedtest()
        app.ui_call.assert_called_once_with(app.run_speedtest_now, manual=True)

    def test_opacity_is_applied_directly_because_it_marshals_itself(self, tray, app):
        tray._on_set_opacity(0.8)()

        assert app.set_opacity.call_args.args == (0.8,)

    def test_the_opacity_menu_offers_the_shared_levels(self, tray, monkeypatch):
        monkeypatch.setattr("tray.container.get_opacity", lambda: 1.0)

        submenu = tray._opacity_submenu()
        labels = [item.text for item in submenu.submenu.items]

        assert labels == [_opacity_label(level) for level in config.OPACITY_LEVELS]
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

        theme_item = tray._build_menu().items[2]
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
        item = tray._build_menu().items[1]
        assert item.enabled is True

        tray.start_speedtest_check()
        item = tray._build_menu().items[1]

        assert item.enabled is False
        assert tray._speedtest_check is True

        tray.stop_speedtest_check()
        assert tray._build_menu().items[1].enabled is True

    def test_quit_stops_the_tray_then_shuts_the_app_down(self, tray, app):
        tray._icon = MagicMock()

        tray.on_quit()

        app.ui_call.assert_called_once_with(app.shutdown)


class TestStatusDisplay:
    def test_status_line_is_empty_until_something_arrives(self, tray):
        assert tray._menu_status_text() == "No data yet"

    def test_live_speeds_and_summary_are_joined(self, tray):
        tray.update_live_status("D 1.0 Mb/s | U 0.2 Mb/s | 30 ms")
        tray.update_speedtest_summary("Speedtest: 90.0 D | 20.0 U")

        assert tray._menu_status_text() == (
            "Speedtest: 90.0 D | 20.0 U | D 1.0 Mb/s | U 0.2 Mb/s | 30 ms"
        )

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
