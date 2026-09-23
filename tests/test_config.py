"""Tests for persistent configuration load, save and validation."""

import os
from unittest.mock import mock_open, patch

from utils import config


class TestConfigPersistence:
    """Cover config file read/write and fallbacks."""

    @patch("utils.config.config_path", return_value="/fake/config.json")
    @patch("os.path.exists", return_value=False)
    def test_load_missing_returns_empty(self, mock_exists, mock_path) -> None:
        assert config.load_config() == {}

    @patch("utils.config.config_path", return_value="/fake/config.json")
    def test_load_reads_valid_json(self, mock_path) -> None:
        import tempfile

        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as tmp:
            tmp.write('{"opacity": 0.5}')
            tmp_path = tmp.name
        with patch("utils.config.config_path", return_value=tmp_path):
            result = config.load_config()
        assert result == {"opacity": 0.5}
        os.unlink(tmp_path)

    @patch("utils.config.config_path", return_value="/fake/config.json")
    @patch("builtins.open", new_callable=mock_open, read_data="bad json")
    def test_load_corrupt_returns_empty(self, mock_file, mock_path) -> None:
        assert config.load_config() == {}

    @patch("utils.config.config_path", return_value="/fake/config.json")
    @patch("builtins.open", new_callable=mock_open)
    def test_save_writes_json(self, mock_file, mock_path) -> None:
        config.save_config({"opacity": 0.9})
        mock_file.assert_called_once()
        handle = mock_file()
        written = "".join(call.args[0] for call in handle.write.call_args_list)
        assert "0.9" in written


class TestConfigDefaults:
    """Cover default values and clamped ranges."""

    def test_get_opacity_default(self) -> None:
        with patch.object(config, "load_config", return_value={}):
            assert config.get_opacity() == 0.72

    def test_get_opacity_clamped_high(self) -> None:
        with patch.object(config, "load_config", return_value={"opacity": 5.0}):
            assert config.get_opacity() == 1.0

    def test_get_opacity_clamped_low(self) -> None:
        with patch.object(config, "load_config", return_value={"opacity": -1.0}):
            assert config.get_opacity() == 0.4

    def test_get_position_none_when_missing(self) -> None:
        with patch.object(config, "load_config", return_value={}):
            assert config.get_position() is None

    def test_get_hide_on_hover_default_false(self) -> None:
        with patch.object(config, "load_config", return_value={}):
            assert config.get_hide_on_hover() is False

    def test_get_hotkey_default(self) -> None:
        with patch.object(config, "load_config", return_value={}):
            assert config.get_hotkey() == "ctrl+shift+alt+n"
