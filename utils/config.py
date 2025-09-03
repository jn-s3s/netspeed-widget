import os
import json
import time
from typing import Any, Dict

from utils.paths import config_path

CONFIG_FILE = "config.json"


def load_config() -> Dict[str, Any]:
    """
    Load the configuration from the JSON config file.
    """
    config_file_path = config_path(CONFIG_FILE)
    if not os.path.exists(config_file_path):
        return {}

    try:
        with open(config_file_path, "r", encoding="utf-8") as file_stream:
            return json.load(file_stream)
    except Exception:
        return {}


def save_config(config: Dict[str, Any]) -> None:
    """
    Save the given configuration dictionary to the JSON config file.
    """
    try:
        with open(config_path(CONFIG_FILE), "w", encoding="utf-8") as file_stream:
            json.dump(config, file_stream, indent=2)
    except Exception:
        # Fail silently if writing fails
        pass


def get_opacity(default: float = 0.72) -> float:
    """
    Returns current UI opacity in a safe range 0.40–1.00.
    Falls back to default if missing or invalid.
    """
    config = load_config()
    try:
        val = float(config.get("opacity", default))
        return max(0.40, min(1.00, val))
    except Exception:
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


def get_speedtest(default: Dict[str, Any] | None = None) -> Dict[str, Any] | None:
    """
    Returns the last saved speedtest dict or default.
    Dict looks like: {"down_mbps": float, "up_mbps": float, "ts": float}
    """
    try:
        config = load_config()
        speedtest = config.get("speedtest")
        if isinstance(speedtest, dict) and {"down_mbps", "up_mbps", "ts"} <= set(speedtest.keys()):
            return speedtest
        return default
    except Exception:
        return default


def set_speedtest(down_mbps: float, up_mbps: float, ts: float | None = None) -> Dict[str, Any]:
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