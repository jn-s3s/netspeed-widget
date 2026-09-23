"""Tests for timestamped log writing."""

from unittest.mock import mock_open, patch

from utils import logger


class TestLoggerWrite:
    """Cover log file append behavior."""

    @patch("builtins.open", new_callable=mock_open)
    @patch("utils.logger.config_path", return_value="/fake/log.txt")
    def test_info_appends_line(self, mock_path, mock_file) -> None:
        logger.info("hello")
        handle = mock_file()
        written = "".join(call.args[0] for call in handle.write.call_args_list)
        assert "hello" in written
        assert "[INFO]" in written

    @patch("builtins.open", new_callable=mock_open)
    @patch("utils.logger.config_path", return_value="/fake/log.txt")
    def test_startup_writes_title_block(self, mock_path, mock_file) -> None:
        logger.startup("App v1")
        handle = mock_file()
        written = "".join(call.args[0] for call in handle.write.call_args_list)
        assert "Startup" in written
        assert "------" in written
