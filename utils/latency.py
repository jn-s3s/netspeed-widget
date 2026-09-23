"""Latency probe based on TCP connect time.

A TCP handshake to a well-known host is a solid latency estimate and is
far cheaper than spawning ping.exe every second. A single ICMP ping is
kept as a second opinion, used only when TCP fails, to tell a real
outage apart from a blocked port.
"""

import socket
import subprocess
import threading
import time
from dataclasses import dataclass

from utils.logger import warn

PING_HOST = "fast.com"
PING_PORT = 443


@dataclass(frozen=True)
class LatencyResult:
    """One latency measurement.

    `ms` is None when TCP failed but ICMP still reached the host.
    `ok` is False when every probe failed, meaning likely offline.
    """

    ms: float | None
    ok: bool
    ts: float


class LatencyProbe:
    """Measures latency on a daemon thread.

    Args:
        host: Target host for the TCP connect.
        port: Target port. 443 passes through most firewalls.
        interval: Seconds between measurements.
        timeout: Socket timeout per attempt.
    """

    def __init__(
        self,
        host: str = PING_HOST,
        port: int = PING_PORT,
        interval: float = 2.0,
        timeout: float = 1.5,
    ) -> None:
        self.host = host
        self.port = port
        self.interval = interval
        self.timeout = timeout
        self._latest = LatencyResult(ms=None, ok=True, ts=0.0)
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        """Start the probe thread once."""
        if self._thread is not None:
            return
        self._thread = threading.Thread(
            target=self._run, name="latency-probe", daemon=True
        )
        self._thread.start()

    def stop(self) -> None:
        """Signal the probe thread to exit."""
        self._stop.set()

    @property
    def latest(self) -> LatencyResult:
        """Most recent measurement."""
        with self._lock:
            return self._latest

    def _run(self) -> None:
        while not self._stop.is_set():
            result = self._measure()
            with self._lock:
                was_ok = self._latest.ok
                self._latest = result
            if result.ok and not was_ok:
                warn("[NET] connectivity restored")
            elif not result.ok and was_ok:
                warn("[NET] connectivity lost")
            self._stop.wait(self.interval)

    def _measure(self) -> LatencyResult:
        start = time.monotonic()
        try:
            with socket.create_connection(
                (self.host, self.port), timeout=self.timeout
            ):
                ms = (time.monotonic() - start) * 1000.0
                return LatencyResult(ms=ms, ok=True, ts=time.time())
        except OSError:
            pass  # documented contract: fall through to the ICMP second opinion

        if self._icmp_ping():
            return LatencyResult(ms=None, ok=True, ts=time.time())
        return LatencyResult(ms=None, ok=False, ts=time.time())

    def _icmp_ping(self) -> bool:
        """Single ICMP echo via the OS ping tool. Fallback path only."""
        cmd = ["ping", self.host, "-n", "1", "-w", "1000"]
        try:
            result = subprocess.run(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=0x08000000,  # CREATE_NO_WINDOW
                check=False,
            )
            return result.returncode == 0
        except OSError:
            return False
