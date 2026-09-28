"""Scheduling for the periodic speedtest, kept free of Tk and of the clock."""

from utils.config import SpeedtestRecord


def next_speedtest_due(
    saved: SpeedtestRecord | None,
    now: float,
    interval_sec: float,
    startup_grace_sec: float,
) -> float:
    """Return the epoch second the next automatic run is due.

    A saved result schedules one full interval after it. If that moment has
    already passed, or nothing was ever measured, the startup grace applies
    so a fresh launch does not go straight onto the network.

    Args:
        saved: The persisted speedtest record, or None when absent.
        now: Current epoch seconds, injected so the caller decides.
        interval_sec: Seconds between automatic runs.
        startup_grace_sec: Delay used when nothing is scheduled ahead.

    Returns:
        An epoch timestamp for the next run.
    """
    if saved is not None:
        due = saved["ts"] + interval_sec
        if due > now:
            return due
    return now + startup_grace_sec
