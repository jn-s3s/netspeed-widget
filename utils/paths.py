"""Location helpers for bundled resources and per-user state files.

Two roots matter. Read-only files that ship beside the app resolve through
`resource_path`, user settings and logs resolve through `config_path`. Both
return `pathlib.Path` so callers chain with `/` instead of rebuilding paths
by hand, which is what the speedtest bundle path and the config writer need.
"""

import os
import sys
from pathlib import Path

APP_DIR_NAME = "NetSpeedWidget"
ICON_FILE = "icon.ico"

# The bundled speedtest backend. build.py writes this layout and
# utils/speedtest.py reads it, so the names live here once.
THIRD_PARTY = "third_party"
NODE_DIR = "node"
NODE_BINARY = "node.exe"
FAST_BUNDLE_DIR = "fast-bundle"
FAST_CLI_ENTRY = ("node_modules", "fast-cli", "distribution", "cli.js")


def resource_path(relative: str) -> Path:
    """Absolute path to a resource that ships alongside the application.

    A PyInstaller bundle, onefile or onedir, unpacks resources into
    `sys._MEIPASS`. A normal script run resolves them against the project
    root, the parent of this package.
    """
    if getattr(sys, "frozen", False):
        base_dir = Path(sys._MEIPASS)  # type: ignore[attr-defined]
    else:
        base_dir = Path(__file__).resolve().parent.parent
    return base_dir / relative


def config_path(filename: str = "config.json") -> Path:
    """Absolute path to a user state file inside the app data directory."""
    return _appdata_dir() / filename


def _appdata_dir() -> Path:
    """Return the NetSpeedWidget folder under APPDATA, creating it if needed.

    A directory that cannot be created is still returned, because the
    config and log writers surface that failure where the damage would
    actually land.
    """
    base = os.environ.get("APPDATA")
    if not base:
        base = Path.home() / "AppData" / "Roaming"
    target = Path(base) / APP_DIR_NAME
    try:
        target.mkdir(parents=True, exist_ok=True)
    except OSError:
        pass  # documented contract: callers surface an unusable directory
    return target


def icon_path() -> Path:
    """The application icon, shared by the window chrome and the tray."""
    return resource_path(ICON_FILE)


def bundled_node_path() -> Path:
    """The node.exe shipped inside the application bundle."""
    return resource_path(THIRD_PARTY) / NODE_DIR / NODE_BINARY


def fast_bundle_dir() -> Path:
    """Working directory the vendored fast-cli runs from."""
    return resource_path(THIRD_PARTY) / FAST_BUNDLE_DIR


def fast_cli_entry_path() -> Path:
    """The fast-cli entry script inside the vendored npm bundle."""
    return fast_bundle_dir().joinpath(*FAST_CLI_ENTRY)
