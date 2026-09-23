"""Persistent user settings held in one JSON file.

The file is shared by three threads: the Tk main thread writes position,
opacity, hotkey and hover on user gestures, the speedtest worker writes a
result when a measurement lands, and the tray reads opacity and hover while
drawing its menu. Every public getter and setter therefore runs under one
module lock, and a save writes a sibling file and renames it into place so a
concurrent reader never observes a truncated config.

A file that cannot be parsed, or that holds a value of the wrong type, is
reported and then treated as absent, so one bad edit degrades to defaults
instead of preventing startup.
"""

import json
import os
import threading
import time
from pathlib import Path
from typing import Any, TypedDict

from utils.logger import get
from utils.paths import config_path

CONFIG_FILE = "config.json"
MIN_OPACITY = 0.40
MAX_OPACITY = 1.00
DEFAULT_OPACITY = 0.72
DEFAULT_HOTKEY = "ctrl+shift+alt+n"

# The choices both the widget menu and the tray submenu offer, so the two
# lists cannot drift apart.
OPACITY_LEVELS: tuple[float, ...] = (1.0, 0.9, 0.8, 0.7, 0.6, 0.5)

_REQUIRED_SPEEDTEST_KEYS = ("down_mbps", "up_mbps", "ts")

_log = get("config")
_lock = threading.RLock()


class SpeedtestRecord(TypedDict):
    """One persisted speedtest measurement with its completion time."""

    down_mbps: float
    up_mbps: float
    ts: float


def clamp_opacity(value: float) -> float:
    """Return `value` inside the supported opacity range.

    Raises TypeError or ValueError when `value` is not numeric, so callers
    that must not die decide what the fallback is.
    """
    return max(MIN_OPACITY, min(MAX_OPACITY, float(value)))


def load_config() -> dict[str, Any]:
    """Read the whole config, reporting and discarding an unusable file."""
    return _read()[0]


def _read() -> tuple[dict[str, Any], bool]:
    """Return the config and whether it was actually readable.

    The distinction matters for a writer. A file that is merely missing can be
    rebuilt from defaults, but a file that could not be opened because it is
    locked, which Windows can report mid-rename, must not be overwritten from
    an empty read or the settings in it would be lost.
    """
    with _lock:
        path = None
        try:
            path = config_path(CONFIG_FILE)
            with open(path, encoding="utf-8") as stream:
                data = json.load(stream)
        except FileNotFoundError:
            return {}, True
        except OSError as err:
            _log.error(f"config at {path} could not be read: {err}")
            return {}, False
        except ValueError as err:
            _log.warning(
                f"discarding unreadable config at {path}: {err};"
                " the next save rebuilds it from defaults"
            )
            return {}, True
        if not isinstance(data, dict):
            _log.warning(f"config at {path} is not a JSON object; ignoring it")
            return {}, True
        return data, True


def save_config(config: dict[str, Any]) -> None:
    """Write `config` in one step, leaving the old file intact on failure.

    Resolution of the target path is inside the handler too: a profile
    directory that cannot be read is a save failure to report, not an
    exception to hand back to a menu callback on the Tk thread.
    """
    with _lock:
        staging = None
        try:
            path = config_path(CONFIG_FILE)
            staging = path.with_name(f"{path.name}.tmp")
            staging.parent.mkdir(parents=True, exist_ok=True)
            with open(staging, "w", encoding="utf-8") as stream:
                json.dump(config, stream, indent=2)
            os.replace(staging, path)
        except (OSError, TypeError, ValueError) as err:
            target = staging if staging is not None else CONFIG_FILE
            _log.error(f"failed to save config via {target}: {err}")
            _discard(staging)


def _discard(path: Path | None) -> None:
    if path is None:
        return
    try:
        if path.exists():
            path.unlink()
    except OSError:
        pass  # documented contract: a leftover temp file costs nothing


def _mutate(key: str, value: Any) -> None:
    """Store one key, holding the lock across the read and the write.

    A config that could not be read is left alone rather than rebuilt, so a
    momentary lock cannot delete the settings it was holding.
    """
    with _lock:
        current, readable = _read()
        if not readable:
            _log.error(f"skipping save of '{key}': the config could not be read")
            return
        current[key] = value
        save_config(current)


def get_opacity(default: float = DEFAULT_OPACITY) -> float:
    """Current window opacity, clamped to the supported range."""
    with _lock:
        try:
            return clamp_opacity(load_config().get("opacity", default))
        except (TypeError, ValueError):
            return default


def set_opacity(value: float) -> float:
    """Persist window opacity and return the clamped value actually stored."""
    with _lock:
        clamped = clamp_opacity(value)
        _mutate("opacity", clamped)
        return clamped


def get_position() -> tuple[int, int] | None:
    """Saved window position, or None when it was never stored or is unusable."""
    with _lock:
        position = load_config().get("position")
    if not isinstance(position, dict):
        return None
    try:
        return int(position["x"]), int(position["y"])
    except (KeyError, TypeError, ValueError):
        return None


def set_position(x: int, y: int) -> None:
    """Persist the window position."""
    _mutate("position", {"x": int(x), "y": int(y)})


def get_hide_on_hover(default: bool = False) -> bool:
    """Whether the widget hides when the cursor enters it."""
    with _lock:
        value = load_config().get("hide_on_hover", default)
    return bool(value) if isinstance(value, bool) else default


def set_hide_on_hover(value: bool) -> None:
    """Persist the auto-hide on hover preference."""
    _mutate("hide_on_hover", bool(value))


def get_speedtest(default: SpeedtestRecord | None = None) -> SpeedtestRecord | None:
    """Last persisted speedtest, or `default` when absent or malformed.

    Values are coerced to float here because the scheduling maths that
    reads `ts` cannot recover from a type that a hand-edited file invented.
    """
    with _lock:
        stored = load_config().get("speedtest")
    if not isinstance(stored, dict):
        return default
    try:
        return SpeedtestRecord(
            down_mbps=float(stored["down_mbps"]),
            up_mbps=float(stored["up_mbps"]),
            ts=float(stored["ts"]),
        )
    except (KeyError, TypeError, ValueError) as err:
        _log.warning(f"ignoring malformed speedtest entry {stored}: {err}")
        return default


def set_speedtest(
    down_mbps: float, up_mbps: float, ts: float | None = None
) -> SpeedtestRecord:
    """Persist a compact speedtest snapshot and return what was stored."""
    record = SpeedtestRecord(
        down_mbps=round(float(down_mbps), 2),
        up_mbps=round(float(up_mbps), 2),
        ts=float(ts if ts is not None else time.time()),
    )
    _mutate("speedtest", record)
    return record


def get_hotkey(default: str = DEFAULT_HOTKEY) -> str:
    """Global show/hide hotkey combo, or `default` when unusable."""
    with _lock:
        value = load_config().get("hotkey", default)
    return value if isinstance(value, str) and value else default


def set_hotkey(combo: str) -> None:
    """Persist the global show/hide hotkey combo."""
    _mutate("hotkey", combo)
