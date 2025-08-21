import os
import json
from typing import Any, Dict
from utils import paths

CONFIG_FILE = "config.json"


def load_config() -> Dict[str, Any]:
    """
    Load the configuration from the JSON config file.
    """
    config_file_path = paths.resource_path(CONFIG_FILE)
    if not os.path.exists(config_file_path):
        return {}
    try:
        with open(config_file_path, "r", encoding="utf-8") as file:
            return json.load(file)
    except Exception:
        return {}

def save_config(config: Dict[str, Any]) -> None:
    """
    Save the given configuration dictionary to the JSON config file.
    """
    config_file_path = paths.resource_path(CONFIG_FILE)
    try:
        with open(config_file_path, "w", encoding="utf-8") as file:
            json.dump(config, file, indent=2)
    except Exception:
        # Fail silently if writing fails
        pass