"""The widget palette, font family and the shape it is drawn on.

Held in one module because the pill, its menus and the capture dialog all
paint with the same colors, and a theme change has to land in one place.
"""

import tkinter as tk
from typing import Any

SURFACE = "#161b22"
BORDER = "#2c333d"
BORDER_HOVER = "#3d4757"
FG = "#e6edf3"
FG_DIM = "#8b949e"
DOWN_COLOR = "#3fb950"
UP_COLOR = "#58a6ff"
GOOD_FILL = "#12321c"
WARN_FILL = "#3b2a0d"
BAD_FILL = "#40171b"
GOOD_COLOR = "#3fb950"
WARN_COLOR = "#d29922"
BAD_COLOR = "#f85149"
TRANSPARENT = "#ff00ff"
FONT_FAMILY = "Segoe UI"


def rounded_rect(
    canvas: tk.Canvas, x1: int, y1: int, x2: int, y2: int, radius: int, **kw: Any
) -> int:
    """Draw a rounded rectangle as a smoothed polygon. Returns item id."""
    points = [
        x1 + radius,
        y1,
        x2 - radius,
        y1,
        x2,
        y1,
        x2,
        y1 + radius,
        x2,
        y2 - radius,
        x2,
        y2,
        x2 - radius,
        y2,
        x1 + radius,
        y2,
        x1,
        y2,
        x1,
        y2 - radius,
        x1,
        y1 + radius,
        x1,
        y1,
    ]
    return canvas.create_polygon(points, smooth=True, **kw)


_STATUS_FILLS = {
    GOOD_COLOR: GOOD_FILL,
    WARN_COLOR: WARN_FILL,
    BAD_COLOR: BAD_FILL,
}


def status_upload_color(status_color: str) -> str:
    """Return the upload color for a health status, keeping healthy blue."""
    return UP_COLOR if status_color == GOOD_COLOR else status_color


def status_download_color(status_color: str) -> str:
    """Return the download color for a health status, keeping healthy green."""
    return DOWN_COLOR if status_color == GOOD_COLOR else status_color


def status_fill_color(status_color: str) -> str:
    """Return the area fill for a health status, passing unknown colors through."""
    return _STATUS_FILLS.get(status_color, status_color)
