import os
import sys

def resource_path(relative_path):
    if getattr(sys, "frozen", False):
        config_path = sys._MEIPASS
    else:
        config_path = os.path.dirname(os.path.abspath(__file__))
        config_path = os.path.abspath(os.path.join(config_path, ".."))

    return os.path.join(config_path, relative_path)