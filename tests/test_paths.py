"""Path resolution tests for bundled resources and the user state folder."""

from pathlib import Path
from unittest.mock import patch

from utils import paths


class TestAppData:
    """The per-user state folder, which config and logs both live in."""

    def test_config_path_honours_appdata_and_returns_a_path(self, isolated_appdata):
        result = paths.config_path("settings.json")

        assert isinstance(result, Path)
        assert result == isolated_appdata / "NetSpeedWidget" / "settings.json"

    def test_config_path_defaults_to_the_config_file(self, isolated_appdata):
        assert paths.config_path().name == "config.json"

    def test_the_directory_is_created_on_first_use(self, isolated_appdata):
        target = isolated_appdata / "NetSpeedWidget"
        assert not target.exists()

        paths.config_path()

        assert target.is_dir()

    def test_missing_appdata_falls_back_to_the_user_profile(self, monkeypatch):
        monkeypatch.delenv("APPDATA", raising=False)
        monkeypatch.setattr(Path, "home", lambda: Path("C:/fake/user"))

        result = paths.config_path("config.json")

        assert result.parent == Path("C:/fake/user/AppData/Roaming/NetSpeedWidget")
        assert result.name == "config.json"

    def test_an_unusable_directory_is_still_returned(self, monkeypatch):
        def refuse(*_a, **_k):
            raise OSError("read-only profile")

        monkeypatch.setattr(Path, "mkdir", refuse)

        assert paths.config_path().name == "config.json"


class TestResources:
    """Read-only files that ship beside the app."""

    @patch("sys.frozen", True, create=True)
    @patch("sys._MEIPASS", "/fake/meipass", create=True)
    def test_frozen_build_resolves_inside_the_bundle(self):
        assert paths.resource_path("icon.ico") == Path("/fake/meipass/icon.ico")

    @patch("sys.frozen", False, create=True)
    def test_source_run_resolves_against_the_repo_root(self):
        assert paths.resource_path("icon.ico") == Path(__file__).resolve().parents[
            1
        ] / ("icon.ico")

    @patch("sys.frozen", False, create=True)
    def test_icon_path_matches_the_tray_and_window_source(self):
        assert paths.icon_path() == paths.resource_path(paths.ICON_FILE)
        assert paths.icon_path().name == "icon.ico"

    @patch("sys.frozen", False, create=True)
    def test_speedtest_bundle_layout(self):
        """The names build.py packages must be the ones speedtest.py reads."""
        node = paths.bundled_node_path()
        cli = paths.fast_cli_entry_path()

        assert node.parts[-3:] == ("third_party", "node", "node.exe")
        assert cli.parts[-6:] == (
            "third_party",
            "fast-bundle",
            "node_modules",
            "fast-cli",
            "distribution",
            "cli.js",
        )
        assert paths.fast_bundle_dir() in cli.parents

    @patch("sys.frozen", False, create=True)
    def test_os_path_usage_is_not_reintroduced(self):
        """Callers chain Paths, so these helpers must keep returning one."""
        assert isinstance(paths.resource_path("x"), Path)
        assert isinstance(paths.config_path("x"), Path)


class TestWorkingTree:
    """Guard the ignore rules that keep generated state out of git."""

    def test_vendor_tree_is_ignored_but_the_lockfile_is_not(self):
        root = Path(__file__).resolve().parents[1]
        rules = {
            line.strip()
            for line in (root / ".gitignore").read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.strip().startswith("#")
        }

        assert "third_party/fast-bundle/node_modules/" in rules
        assert "third_party/fast-bundle/" not in rules
        assert ".venv/" in rules
        assert ".pytest_cache/" in rules
