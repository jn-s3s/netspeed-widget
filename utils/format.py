"""Small display formatting helpers shared by the widget and the tray."""


def format_speed(mbps: float) -> str:
    """Format a speed with precision matched to its magnitude."""
    if mbps >= 100.0:
        return f"{mbps:.0f}"
    if mbps >= 10.0:
        return f"{mbps:.1f}"
    return f"{mbps:.2f}"
