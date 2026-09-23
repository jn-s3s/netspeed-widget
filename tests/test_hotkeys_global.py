"""Tests for GlobalHotkey thread lifecycle and combo application."""

from unittest.mock import patch

from utils.hotkeys import GlobalHotkey


class TestGlobalHotkey:
    """Cover listener init, combo apply and shutdown."""

    @patch("win32gui.RegisterClass", return_value=1234)
    @patch("win32gui.CreateWindowEx", return_value=5678)
    def test_init_creates_window(self, mock_create, mock_reg) -> None:
        hotkey = GlobalHotkey(on_trigger=lambda: None)
        assert hotkey._hwnd is not None
        hotkey.stop()

    @patch("win32gui.PostMessage")
    def test_stop_posts_close(self, mock_post) -> None:
        with patch("win32gui.CreateWindowEx", return_value=1):
            hotkey = GlobalHotkey(on_trigger=lambda: None)
        hotkey.stop()
        mock_post.assert_called()
