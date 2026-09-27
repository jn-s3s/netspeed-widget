"""The widget palette, theme registry and the shape it is drawn on.

Held in one module because the pill, its menus and the capture dialog all
paint with the same colors, and a theme change has to land in one place.
Also holds the theme registry and status health color mapping.
"""

import tkinter as tk
from dataclasses import dataclass
from enum import StrEnum
from typing import Any


class Health(StrEnum):
    """Latency health bands that tint the status dot, rows and graph."""

    GOOD = "good"
    WARN = "warn"
    BAD = "bad"


SURFACE = "#161b22"
BORDER = "#2c333d"
BORDER_HOVER = "#3d4757"
FG = "#e6edf3"
FG_DIM = "#8b949e"
DOWN_COLOR = "#40c4d8"
UP_COLOR = "#a58bff"
GOOD_FILL = "#12321c"
WARN_FILL = "#3b2a0d"
BAD_FILL = "#40171b"
TRANSPARENT = "#ff00ff"
FONT_FAMILY = "Segoe UI"


@dataclass(frozen=True)
class Palette:
    """Colors used by the widget and its menus; no layout or font settings."""

    surface: str
    border: str
    border_hover: str
    fg: str
    fg_dim: str
    down: str
    up: str
    good_fill: str
    warn_fill: str
    bad_fill: str
    good: str
    warn: str
    bad: str


def _palette(
    surface: str,
    border: str,
    hover: str,
    fg: str,
    dim: str,
    down: str,
    up: str,
    light: bool = False,
) -> Palette:
    """Build a theme while keeping health colors semantically consistent."""
    return Palette(
        surface=surface,
        border=border,
        border_hover=hover,
        fg=fg,
        fg_dim=dim,
        down=down,
        up=up,
        good_fill="#e0f2e8" if light else "#12321c",
        warn_fill="#fff0d1" if light else "#3b2a0d",
        bad_fill="#fbe4e2" if light else "#40171b",
        good="#16804a" if light else "#43c77a",
        warn="#9b6000" if light else "#f0b94f",
        bad="#bd3438" if light else "#ff6872",
    )


DEFAULT_THEME = "Default"
THEMES: dict[str, Palette] = {
    DEFAULT_THEME: _palette(
        SURFACE, BORDER, BORDER_HOVER, FG, FG_DIM, DOWN_COLOR, UP_COLOR
    ),
    "Ocean": _palette(
        "#0b2431", "#245064", "#39768d", "#e1f5fa", "#9abac6", "#43c7d6", "#a894ff"
    ),
    "Light": _palette(
        "#f4f7fb",
        "#b9c8d7",
        "#879eb4",
        "#172b40",
        "#526779",
        "#087f91",
        "#7453bd",
        True,
    ),
    "Rose": _palette(
        "#2d1520", "#563044", "#80506a", "#fce4ec", "#c99aad", "#76c9e8", "#b69cff"
    ),
    "Sakura": _palette(
        "#fff0f5",
        "#e8b4c8",
        "#d491a8",
        "#4a1a2e",
        "#765666",
        "#087f91",
        "#6554bf",
        True,
    ),
    "Lavender": _palette(
        "#1a1428", "#42345d", "#655286", "#eee7fa", "#b3a4ca", "#62c9dd", "#83b6ff"
    ),
    "Midnight": _palette(
        "#0a0e1a", "#27314d", "#465579", "#e0e6fa", "#a0accb", "#36c6d3", "#8c9dff"
    ),
    "Forest": _palette(
        "#101b16", "#304d3c", "#50755b", "#e2f0e5", "#a2bca8", "#53c6d7", "#a9a0ff"
    ),
    "Cyber": _palette(
        "#0b0c16", "#34314f", "#5d547e", "#f1eaff", "#b3a6cf", "#00d8c0", "#aa8bff"
    ),
    "Candy": _palette(
        "#fff5f9",
        "#e7bed0",
        "#d796b4",
        "#542038",
        "#795a6a",
        "#087f91",
        "#6550cf",
        True,
    ),
    "Sunset": _palette(
        "#21120f", "#59352b", "#815243", "#ffead7", "#c9a08e", "#42c8d0", "#89aaff"
    ),
    "Crimson": _palette(
        "#1d1015", "#50303b", "#754858", "#f8e5eb", "#c0a0aa", "#45c8d1", "#a99aff"
    ),
    "Monochrome": _palette(
        "#171717", "#3b3b3b", "#626262", "#eeeeee", "#b5b5b5", "#62c7d1", "#bd9cff"
    ),
    "Aurora": _palette(
        "#0a1420", "#254353", "#3d6877", "#e1f4ef", "#9ebeb4", "#40d5c0", "#c19aff"
    ),
    "Peach": _palette(
        "#fff3e8",
        "#e4c2a7",
        "#c79675",
        "#50341f",
        "#765840",
        "#087f91",
        "#526fc2",
        True,
    ),
    "Galaxy": _palette(
        "#100b20", "#382755", "#59417d", "#e8defb", "#b1a0cc", "#40c6db", "#b08aff"
    ),
    "Mint": _palette(
        "#f0faf5",
        "#afd2c1",
        "#7faf99",
        "#1a3a2a",
        "#527765",
        "#087f94",
        "#6854bd",
        True,
    ),
    "Honey": _palette(
        "#211a0d", "#554321", "#80622e", "#fff0c0", "#c6ad73", "#45c7d3", "#aaa0ff"
    ),
    "Coral Reef": _palette(
        "#0a1820", "#28505b", "#427682", "#e0f5f1", "#9bbdb5", "#5bc8d2", "#83b9ff"
    ),
    "Denim": _palette(
        "#141e2a", "#354b60", "#526f89", "#e2ebf4", "#a5b8c9", "#42c7d5", "#b39aff"
    ),
}


def palette_for(name: str) -> Palette:
    """Return a preset palette, falling back to Default for unknown names."""
    return THEMES.get(name, THEMES[DEFAULT_THEME])


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


def status_upload_color(health: Health, palette: Palette) -> str:
    """Return the upload color for a health status, keeping healthy blue."""
    if health is Health.GOOD:
        return palette.up
    return status_color_for(health, palette)


def status_download_color(health: Health, palette: Palette) -> str:
    """Return the download color for a health status, keeping healthy green."""
    if health is Health.GOOD:
        return palette.down
    return status_color_for(health, palette)


def status_fill_color(health: Health, palette: Palette) -> str:
    """Return the area fill for a health status."""
    match health:
        case Health.GOOD:
            return palette.good_fill
        case Health.WARN:
            return palette.warn_fill
        case Health.BAD:
            return palette.bad_fill
    raise KeyError(health)


def status_color_for(health: Health, palette: Palette) -> str:
    """Map a recorded health status to its palette color."""
    match health:
        case Health.GOOD:
            return palette.good
        case Health.WARN:
            return palette.warn
        case Health.BAD:
            return palette.bad
    raise KeyError(health)
