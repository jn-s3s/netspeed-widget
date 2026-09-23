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
        self._last_sent: int | None = None
        self._last_recv: int | None = None
        self._last_ts: float | None = None

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

        self._record_sample(counters.bytes_sent, counters.bytes_recv, time.monotonic())

        while not self._stop.wait(self.interval):
            now = time.monotonic()
            try:
                counters = psutil.net_io_counters()
            except OSError as err:
                warn(f"[SAMPLER] counter read failed: {err}")
                continue
            self._record_sample(counters.bytes_sent, counters.bytes_recv, now)

    def _record_sample(self, bytes_sent: int, bytes_recv: int, now: float) -> None:
        """Fold one counter reading into rates, session totals and history.

        The first call after start only seeds the baseline. Deltas are clamped
        at zero because an adapter reset can drop the counters below the
        previous reading; without the clamp the widget would briefly show
        negative speeds and the session totals would shrink.
        """
        if self._last_ts is None:
            self._last_sent = bytes_sent
            self._last_recv = bytes_recv
            self._last_ts = now
            return

        elapsed = max(now - self._last_ts, 1e-6)
        d_bytes = max(bytes_recv - self._last_recv, 0)
        u_bytes = max(bytes_sent - self._last_sent, 0)
        down_mbps = d_bytes * 8.0 / 1e6 / elapsed
        up_mbps = u_bytes * 8.0 / 1e6 / elapsed

        with self._lock:
            self._session_down_bytes += d_bytes
            self._session_up_bytes += u_bytes
            self._samples.append(
                NetSample(down_mbps=down_mbps, up_mbps=up_mbps, ts=time.time())
            )

        self._last_sent = bytes_sent
        self._last_recv = bytes_recv
        self._last_ts = now
