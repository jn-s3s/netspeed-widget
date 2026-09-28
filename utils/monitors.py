"""Win32 display geometry, used to keep the widget on a connected screen.

No Tk here on purpose: every function answers a question about monitors, so
the widget's rescue logic can be tested without a window or a display.
"""

import win32api
import win32con

from utils.logger import get

_log = get("monitor")

Rect = tuple[int, int, int, int]

# (0, 0) is a valid anchor, so a layout with a gap there still resolves to
# the nearest monitor rather than failing the query.
_MONITOR_FALLBACK = (100, 100)


def active_monitor_rects() -> list[Rect]:
    """Return (left, top, right, bottom) for every connected monitor.

    These are full monitor rects, taskbar included, so they suit visibility
    tests only. Anything that places the window needs `work_area` instead.
    An empty list means the display answered no query at all.
    """
    try:
        return [
            (rect[0], rect[1], rect[2], rect[3])
            for _, _, rect in win32api.EnumDisplayMonitors()
        ]
    except Exception as err:  # noqa: BLE001 - a bad display must not stop the UI
        _log.warning(f"could not enumerate monitors: {err}")
        return []


def point_on_active_monitor(
    x: int, y: int, width: int, height: int, rects: list[Rect] | None = None
) -> bool:
    """True when a window's center at (x, y) sits on any connected monitor.

    When the layout cannot be read the window is assumed visible, because
    guessing at a rect we could not read strands the widget more often than
    leaving it alone does.
    """
    if rects is None:
        rects = active_monitor_rects()
    if not rects:
        return True
    center_x = x + width // 2
    center_y = y + height // 2
    return any(
        left <= center_x < right and top <= center_y < bottom
        for left, top, right, bottom in rects
    )


def default_position(width: int, height: int, margin: int) -> tuple[int, int]:
    """Bottom-right corner of the primary monitor work area.

    The work area, not the full monitor rect, so the window clears the
    taskbar. If the display resolves nothing at all, a visible top-left
    beats a confident guess at a rect that could not be read.
    """
    try:
        monitor = win32api.MonitorFromPoint((0, 0), win32con.MONITOR_DEFAULTTONEAREST)
        _, _, right, bottom = win32api.GetMonitorInfo(monitor)["Work"]
    except Exception as err:  # noqa: BLE001 - fall back to a visible spot
        _log.warning(f"could not resolve the primary monitor work area: {err}")
        return _MONITOR_FALLBACK
    return (right - width - margin, bottom - height - margin)
