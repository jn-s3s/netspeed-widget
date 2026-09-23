"""Background network throughput sampler.

Polling runs on a daemon thread so the Tk main loop never blocks on
psutil calls. The UI reads the latest immutable snapshot through a lock,
which keeps every widget update on the main thread.
"""

import threading
import time
from collections import deque
from dataclasses import dataclass

import psutil

from utils.logger import warn


@dataclass(frozen=True)
class NetSample:
    """One throughput snapshot in megabits per second."""

    down_mbps: float
    up_mbps: float
    ts: float


class NetSampler:
    """Samples system net I/O counters at a fixed interval.

    Args:
        interval: Seconds between samples.
        history: Number of recent samples kept for graphing.
    """

    def __init__(self, interval: float = 1.0, history: int = 60) -> None:
        self.interval = interval
        self._samples: deque[NetSample] = deque(maxlen=history)
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._session_down_bytes = 0
        self._session_up_bytes = 0

    def start(self) -> None:
        """Start the sampling thread once."""
        if self._thread is not None:
            return
        self._thread = threading.Thread(
            target=self._run, name="net-sampler", daemon=True
        )
        self._thread.start()

    def stop(self) -> None:
        """Signal the sampling thread to exit."""
        self._stop.set()

    @property
    def latest(self) -> NetSample | None:
        """Most recent sample, or None before the first tick."""
        with self._lock:
            return self._samples[-1] if self._samples else None

    @property
    def history(self) -> list[NetSample]:
        """Copy of the rolling sample window, oldest first."""
        with self._lock:
            return list(self._samples)

    @property
    def session_totals(self) -> tuple[float, float]:
        """Total (download, upload) in megabytes since start."""
        with self._lock:
            return (
                self._session_down_bytes / 1_000_000.0,
                self._session_up_bytes / 1_000_000.0,
            )

    def _run(self) -> None:
        try:
            counters = psutil.net_io_counters()
        except OSError as err:
            warn(f"[SAMPLER] failed to read net counters: {err}")
            return

        last_sent = counters.bytes_sent
        last_recv = counters.bytes_recv
        last_ts = time.monotonic()

        while not self._stop.wait(self.interval):
            now = time.monotonic()
            try:
                counters = psutil.net_io_counters()
            except OSError as err:
                warn(f"[SAMPLER] counter read failed: {err}")
                continue

            elapsed = max(now - last_ts, 1e-6)
            d_bytes = counters.bytes_recv - last_recv
            u_bytes = counters.bytes_sent - last_sent
            down_mbps = d_bytes * 8.0 / 1e6 / elapsed
            up_mbps = u_bytes * 8.0 / 1e6 / elapsed

            with self._lock:
                self._session_down_bytes += d_bytes
                self._session_up_bytes += u_bytes
                self._samples.append(
                    NetSample(down_mbps=down_mbps, up_mbps=up_mbps, ts=time.time())
                )

            last_sent = counters.bytes_sent
            last_recv = counters.bytes_recv
            last_ts = now
