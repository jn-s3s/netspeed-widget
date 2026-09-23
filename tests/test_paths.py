"""Tests for resource and config path resolution."""

import os
from unittest.mock import patch

from utils import paths


class TestPaths:
    """Cover frozen/non-frozen resource resolution."""

    def test_config_path_uses_appdata(self) -> None:
        with patch.dict(os.environ, {"APPDATA": "/fake/appdata"}):
            result = paths.config_path("test.json")
            assert result == os.path.join(
                "/fake/appdata", "NetSpeedWidget", "test.json"
            )

    @patch("sys.frozen", True, create=True)
    @patch("sys._MEIPASS", "/fake/meipass", create=True)
    def test_resource_path_frozen(self) -> None:
        result = paths.resource_path("icon.ico")
        assert result == os.path.join("/fake/meipass", "icon.ico")

    @patch("sys.frozen", False, create=True)
    def test_resource_path_non_frozen(self) -> None:
        result = paths.resource_path("icon.ico")
        assert "icon.ico" in result
