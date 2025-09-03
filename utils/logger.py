import platform
import sys
from datetime import datetime, timezone

from utils.paths import config_path

LOG_FILE: str = "log.txt"


def save_log(message: str, has_time: bool = True, is_title: bool = False) -> str:
    """
    Append a line or a title block to the log file.

    Log lives at %APPDATA%\\NetSpeedWidget.
    """
    try:
        timestamp = _now_iso()
        if is_title:
            formatted = (
                "-----------------------------------------------------------\n"
                f"------  {message}\n"
                f"------  {timestamp}\n"
                "-----------------------------------------------------------\n"
            )
        elif has_time:
            formatted = f"[{timestamp}] - {message}\n"
        else:
            formatted = f"{message}\n"

        with open(config_path(LOG_FILE), "a", encoding="utf-8", errors="replace") as f:
                f.write(formatted)
        return message
    except Exception:
        # Logging must never break the app.
        return message


def startup(app_name: str) -> None:
    """
    Log a single startup banner for visibility.
    """
    save_log(f"Startup - {app_name} | Runtime - python={platform.python_version()} | exe={getattr(sys, 'frozen', False)}", is_title=True)


def section(title: str) -> None:
    """
    Convenience helper to write a section header.
    """
    save_log(title, is_title=True)


def info(msg: str) -> None:
    """
    Info-level message. Simple prefix to help scanning.
    """
    save_log(f"[INFO] {msg}", has_time=True)


def warn(msg: str) -> None:
    """
    Warning-level message.
    """
    save_log(f"[WARN] {msg}", has_time=True)


def _now_iso() -> str:
    """
    Current UTC time in ISO 8601.
    """
    return datetime.now(timezone.utc).isoformat()
