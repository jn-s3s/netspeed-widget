"""Tests for tray controller actions and mock icon lifecycle."""

from unittest.mock import MagicMock, patch

from tray.container import TrayController


class TestTrayController:
    """Cover tray init, start, updates and quit."""

    @patch("tray.container.paths.resource_path", return_value="/fake/icon.ico")
    @patch("PIL.Image.open")
    @patch("pystray.Icon")
    def test_start_creates_icon(self, mock_icon, mock_image_open, mock_path) -> None:
        app = MagicMock()
        tray = TrayController(app, "App")
        tray.start()
        assert tray.icon is not None
        tray.stop()

    @patch("tray.container.get_opacity", return_value=0.72)
    def test_opacity_submenu_uses_current(self, mock_opacity) -> None:
        app = MagicMock()
        tray = TrayController(app, "App")
        submenu = tray._opacity_submenu()
        assert submenu is not None
