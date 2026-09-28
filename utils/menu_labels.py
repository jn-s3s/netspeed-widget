"""Shared text formats for the widget context menu and the tray menu.

Both UI surfaces render the same dynamic lines (session totals, speedtest
summary, hotkey state, auto-hide checkmark, opacity levels), so the formats
live here instead of being re-inlined in each menu builder.
"""


def session_label(down_mb: float, up_mb: float) -> str:
    """Cumulative transfer line for this session."""
    return f"Session: {down_mb:.1f} MB down, {up_mb:.1f} MB up"


def speedtest_label(summary: str) -> str:
    """Last-speedtest line, falling back to a placeholder before any run."""
    return summary or "Last speedtest: --"


def hotkey_label(ok: bool, combo: str) -> str:
    """Hotkey action label matching the registration state."""
    if ok:
        from utils.hotkeys import format_hotkey

        return f"Change hotkey ({format_hotkey(combo)})"
    return "Set hotkey (none active)"


def auto_hide_label(enabled: bool) -> str:
    """Auto-hide line carrying the same text checkmark as the widget."""
    return f"{'✓ ' if enabled else ''}Auto-hide on hover"


def opacity_percent_label(level: float) -> str:
    """Opacity cascade title showing the current percent."""
    return f"Opacity ({round(level * 100)}%)"


def opacity_choice_label(level: float, current: float) -> str:
    """One opacity choice, prefixed with a marker when it is the current one."""
    marker = "✓ " if abs(current - level) < 0.005 else ""
    return f"{marker}{int(level * 100)}%"


def opacity_choice_text(level: float) -> str:
    """One opacity choice without a marker (for menus with native checkmarks)."""
    return f"{int(level * 100)}%"
