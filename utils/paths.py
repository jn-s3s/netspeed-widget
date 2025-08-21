import os
import sys
from typing import Final


def resource_path(relative_path: str) -> str:
    """
    Resolve an absolute filesystem path for a resource that lives alongside the app,
    supporting both normal script runs and PyInstaller-frozen executables.

    When running as a PyInstaller bundle (onefile or onedir), `sys._MEIPASS` points
    to the temporary extraction folder that contains bundled resources. Otherwise,
    we resolve the path relative to the project root (parent of the `utils` folder).
    """
    # Detect PyInstaller-frozen app
    is_frozen: Final[bool] = bool(getattr(sys, "frozen", False))

    if is_frozen:
        base_dir = getattr(sys, "_MEIPASS")  # type: ignore[attr-defined]
    else:
        # utils/paths.py -> go up to project root
        utils_dir = os.path.dirname(os.path.abspath(__file__))
        base_dir = os.path.abspath(os.path.join(utils_dir, ".."))

    return os.path.join(base_dir, relative_path)
