import os
import json
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
    cfg = load_config()
    try:
        val = float(cfg.get("opacity", default))
        return max(0.40, min(1.00, val))
    except Exception:
        return default


def set_opacity(value: float) -> float:
    """
    Persists UI opacity to config and returns the clamped value.
    """
    clamped = max(0.40, min(1.00, float(value)))
    cfg = load_config()
    cfg["opacity"] = clamped
    save_config(cfg)
    return clamped