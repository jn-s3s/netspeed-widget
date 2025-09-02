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
