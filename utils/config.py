import os, json
from utils import paths

CONFIG_FILE = "config.json"

def load_config():
    config_file_path = paths.resource_path(CONFIG_FILE)
    if not os.path.exists(config_file_path):
        return {}
    try:
        with open(config_file_path, "r") as f:
            return json.load(f)
    except Exception:
        return {}

def save_config(config):
    config_file_path = paths.resource_path(CONFIG_FILE)
    try:
        with open(config_file_path, "w") as f:
            json.dump(config, f, indent=2)
    except Exception:
        pass