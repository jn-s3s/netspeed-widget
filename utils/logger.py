"""File logging for a packaged app that has no console.

`log.txt` under the app data directory is the only diagnostic surface a
`--noconsole` build has, so it has to survive being the only one: writes
never raise into the caller, the file rolls over at a size cap instead of
growing forever, and a failed write is mirrored to stderr where one exists.

Call `get()` once per module to bind a subsystem tag, which keeps every
line attributable without repeating the tag in each message string.
"""

import platform
import sys
from datetime import UTC, datetime
from pathlib import Path

from utils.paths import config_path

LOG_FILE: str = "log.txt"
MAX_LOG_BYTES = 2_000_000
ROTATED_FILES = 2


class Logger:
    """Writes levelled lines tagged with the subsystem that emitted them."""

    def __init__(self, subsystem: str) -> None:
        self._subsystem = subsystem.upper()

    def info(self, msg: str) -> str:
        """Record a routine event."""
        return self._write("INFO", msg)

    def warning(self, msg: str) -> str:
        """Record a recovered failure that the app worked around."""
        return self._write("WARN", msg)

    def error(self, msg: str) -> str:
        """Record a failure the caller could not recover from."""
        return self._write("ERROR", msg)

    def section(self, title: str) -> str:
        """Write a separator block that groups the lines that follow it."""
        stamp = _now_iso()
        rule = "-" * 59
        block = f"{rule}\n------  {title}\n------  {stamp}\n{rule}\n"
        return self._append(block)

    def _write(self, level: str, msg: str) -> str:
        return self._append(f"[{_now_iso()}] - [{level}] [{self._subsystem}] {msg}\n")

    def _append(self, block: str) -> str:
        """Append one block, rolling the file when it passes the size cap."""
        try:
            path = config_path(LOG_FILE)
            if _should_rotate(path, len(block)):
                _rotate(path)
            with open(path, "a", encoding="utf-8", errors="replace") as stream:
                stream.write(block)
        except Exception as err:  # noqa: BLE001 - logging must never break the app
            _report_write_failure(err, block)
        return block


_INSTANCES: dict[str, Logger] = {}


def get(subsystem: str) -> Logger:
    """Return the shared logger for a subsystem, creating it on first use."""
    key = subsystem.upper()
    if key not in _INSTANCES:
        _INSTANCES[key] = Logger(key)
    return _INSTANCES[key]


def startup(app_name: str) -> str:
    """Write the startup banner carrying the runtime and packaging mode."""
    return get("app").section(
        f"Startup - {app_name} | Runtime - python={platform.python_version()}"
        f" | exe={getattr(sys, 'frozen', False)}"
    )


def section(title: str) -> str:
    """Write a top-level section header for a lifecycle event."""
    return get("app").section(title)


def _now_iso() -> str:
    return datetime.now(UTC).isoformat()


def _should_rotate(path: Path, incoming: int) -> bool:
    """True when appending `incoming` bytes would pass the size cap."""
    try:
        size = path.stat().st_size
    except OSError:
        return False  # documented contract: no file yet, nothing to roll
    return size + incoming > MAX_LOG_BYTES


def _rotate(path: Path) -> None:
    """Shift log.txt through the numbered backups, dropping the oldest."""
    for index in range(ROTATED_FILES, 1, -1):
        older = path.with_name(f"{path.name}.{index - 1}")
        if older.exists():
            older.replace(path.with_name(f"{path.name}.{index}"))
    if path.exists():
        path.replace(path.with_name(f"{path.name}.1"))


def _report_write_failure(err: Exception, block: str) -> None:
    """Echo a dropped log line where a developer can still see it."""
    if sys.stderr is None:
        return
    try:
        sys.stderr.write(f"[ERROR] [LOG] write failed: {err} | dropped: {block}")
    except (OSError, ValueError):
        pass  # documented contract: no usable stderr in a windowed build
