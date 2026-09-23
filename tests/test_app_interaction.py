"""Tests for core NetSpeedWidget interactions without a real Tk window."""

from unittest.mock import MagicMock, patch

from app import NetSpeedWidget


class TestAppInteractions:
    """Cover drag, hover and opacity actions."""

    @patch("app.NetSpeedWidget._build_menu", return_value=MagicMock())
    @patch("app.tkfont.Font", return_value=MagicMock())
    @patch("app.tk.Tk")
    @patch("app.GlobalHotkey")
    @patch("app.NetSampler")
    @patch("app.LatencyProbe")
    def test_init_sets_defaults(
        self, mock_probe, mock_sampler, mock_hotkey, mock_tk, mock_font, mock_menu
    ) -> None:
        mock_hotkey.return_value.apply_combo.return_value = (True, None)
        root = mock_tk.return_value
        widget = NetSpeedWidget(root)
        assert widget._run is True
        widget.shutdown()

    @patch("app.NetSpeedWidget._build_menu", return_value=MagicMock())
    @patch("app.tkfont.Font", return_value=MagicMock())
    @patch("app.tk.Tk")
    @patch("app.GlobalHotkey")
    @patch("app.NetSampler")
    @patch("app.LatencyProbe")
    def test_toggle_window_changes_state(
        self, mock_probe, mock_sampler, mock_hotkey, mock_tk, mock_font, mock_menu
    ) -> None:
        mock_hotkey.return_value.apply_combo.return_value = (True, None)
        root = mock_tk.return_value
        root.state.side_effect = ["normal", "normal"]
        widget = NetSpeedWidget(root)
        widget.toggle_window()
        root.withdraw.assert_called()
        widget.shutdown()
