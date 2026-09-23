import json
import os
import time
from typing import Any

from utils.logger import warn
from utils.paths import config_path

CONFIG_FILE = "config.json"


def load_config() -> dict[str, Any]:
    """
    Load the configuration from the JSON config file.
    """
    config_file_path = config_path(CONFIG_FILE)
    if not os.path.exists(config_file_path):
        return {}

    try:
        with open(config_file_path, "r", encoding="utf-8") as file_stream:
            return json.load(file_stream)
    except (OSError, ValueError):
        # Unreadable or corrupt config falls back to defaults.
        return {}


def save_config(config: dict[str, Any]) -> None:
    """
    Save the given configuration dictionary to the JSON config file.
    """
    try:
        with open(config_path(CONFIG_FILE), "w", encoding="utf-8") as file_stream:
            json.dump(config, file_stream, indent=2)
    except (OSError, TypeError, ValueError) as err:
        warn(f"[CONFIG] Failed to save config: {err}")


def get_opacity(default: float = 0.72) -> float:
    """
    Returns current UI opacity in a safe range 0.40–1.00.
    Falls back to default if missing or invalid.
    """
    config = load_config()
    try:
        val = float(config.get("opacity", default))
        return max(0.40, min(1.00, val))
    except (TypeError, ValueError):
        return default


def set_opacity(value: float) -> float:
    """
    Persists UI opacity to config and returns the clamped value.
    """
    clamped = max(0.40, min(1.00, float(value)))
    config = load_config()
    config["opacity"] = clamped
    save_config(config)
    return clamped


def get_position() -> tuple[int, int] | None:
    """Returns the saved window position, or None if never dragged."""
    try:
        position = load_config().get("position")
        if isinstance(position, dict) and {"x", "y"} <= set(position.keys()):
            return int(position["x"]), int(position["y"])
        return None
    except (TypeError, ValueError):
        return None


def set_position(x: int, y: int) -> None:
    """Persists the window position."""
    config = load_config()
    config["position"] = {"x": int(x), "y": int(y)}
    save_config(config)


def get_hide_on_hover(default: bool = False) -> bool:
    """Returns whether the widget hides when the cursor enters it."""
    try:
        return bool(load_config().get("hide_on_hover", default))
    except (TypeError, ValueError):
        return default


def set_hide_on_hover(value: bool) -> None:
    """Persists the auto-hide on hover preference."""
    config = load_config()
    config["hide_on_hover"] = bool(value)
    save_config(config)


def get_speedtest(default: dict[str, Any] | None = None) -> dict[str, Any] | None:
    """
    Returns the last saved speedtest dict or default.
    Dict looks like: {"down_mbps": float, "up_mbps": float, "ts": float}
    """
    try:
        config = load_config()
        speedtest = config.get("speedtest")
        if isinstance(speedtest, dict) and {"down_mbps", "up_mbps", "ts"} <= set(
            speedtest.keys()
        ):
            return speedtest
        return default
    except (TypeError, ValueError):
        return default


def set_speedtest(
    down_mbps: float, up_mbps: float, ts: float | None = None
) -> dict[str, Any]:
    """
    Saves a compact speedtest snapshot. Returns the saved dict.
    """
    payload = {
        "down_mbps": round(float(down_mbps), 2),
        "up_mbps": round(float(up_mbps), 2),
        "ts": float(ts if ts is not None else time.time()),
    }
    config = load_config()
    config["speedtest"] = payload
    save_config(config)
    return payload


def get_hotkey(default: str = "ctrl+shift+alt+n") -> str:
    """Returns the global show/hide hotkey combo string."""
    try:
        value = load_config().get("hotkey", default)
        return value if isinstance(value, str) and value else default
    except (TypeError, ValueError):
        return default


def set_hotkey(combo: str) -> None:
    """Persists the global show/hide hotkey combo string."""
    config = load_config()
    config["hotkey"] = combo
    save_config(config)
