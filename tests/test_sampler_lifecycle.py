"""Sampler thread tests: start once, stop cleanly, and cover the run loop.

The earlier lifecycle test started a real thread and never joined it, so the
suite leaked a worker per run and never exercised `_run` at all.
"""

import time

import pytest

from utils import sampler as sm
from utils.sampler import NetSampler


class _Snapshot:
    """Stands in for the psutil counter object."""

    def __init__(self, sent: int, recv: int):
        self.bytes_sent = sent
        self.bytes_recv = recv


def _wait_until(predicate, timeout: float = 3.0) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.01)
    return False


@pytest.fixture
def feed(monkeypatch):
    """Install a scripted psutil.net_io_counters that replays `readings`."""

    def _install(readings):
        script = list(readings)

        def fake():
            if script:
                item = script.pop(0)
            else:
                item = readings[-1]
            if isinstance(item, Exception):
                raise item
            return item

        monkeypatch.setattr(sm.psutil, "net_io_counters", fake)

    return _install


class TestThreadLifecycle:
    def test_start_is_idempotent(self, feed):
        feed([_Snapshot(0, 0)])
        sampler = NetSampler(interval=0.05)

        sampler.start()
        thread = sampler._thread
        sampler.start()

        assert sampler._thread is thread
        sampler.stop()
        thread.join(timeout=3)

    def test_stop_ends_the_worker_thread(self, feed):
        feed([_Snapshot(0, 0), _Snapshot(1_000_000, 1_000_000)])
        sampler = NetSampler(interval=0.02)
        sampler.start()
        thread = sampler._thread
        assert _wait_until(lambda: sampler.latest is not None)

        sampler.stop()
        thread.join(timeout=3)

        assert not thread.is_alive()

    def test_a_counter_that_always_fails_ends_the_thread_without_raising(self, feed):
        feed([OSError("no adapters")])
        sampler = NetSampler(interval=0.02)

        sampler.start()
        thread = sampler._thread
        assert _wait_until(lambda: not thread.is_alive())

        assert sampler.latest is None

    def test_a_transient_failure_does_not_stop_sampling(self, feed):
        feed(
            [
                _Snapshot(0, 0),
                OSError("transient"),
                _Snapshot(5_000_000, 1_000_000),
                _Snapshot(9_000_000, 2_000_000),
            ]
        )
        sampler = NetSampler(interval=0.02)

        sampler.start()
        sampled = _wait_until(lambda: sampler.latest is not None)
        alive = sampler._thread.is_alive()
        sampler.stop()
        sampler._thread.join(timeout=3)

        assert sampled
        assert alive


class TestRunLoop:
    def test_progress_is_recorded_after_the_seeding_reading(self, feed):
        feed([_Snapshot(0, 0), _Snapshot(1_250_000, 625_000)])
        sampler = NetSampler(interval=0.02)

        sampler.start()
        got_sample = _wait_until(lambda: len(sampler.history) >= 1)
        sampler.stop()
        sampler._thread.join(timeout=3)

        assert got_sample

    def test_session_totals_accumulate_the_transferred_bytes(self, feed):
        feed([_Snapshot(0, 0), _Snapshot(625_000, 1_250_000)])
        sampler = NetSampler(interval=0.02)

        sampler.start()
        finished = _wait_until(lambda: sampler.session_totals[0] > 0)
        sampler.stop()
        sampler._thread.join(timeout=3)
        down, up = sampler.session_totals

        assert finished
        assert down == pytest.approx(1.25, rel=0.5)
        assert up == pytest.approx(0.625, rel=0.5)
        assert sampler.latest.down_mbps > 0

    def test_history_is_bounded_by_the_configured_window(self, feed):
        feed([_Snapshot(index * 10, index * 10) for index in range(1, 200)])
        sampler = NetSampler(interval=0.001, history=5)

        sampler.start()
        time.sleep(0.2)
        sampler.stop()
        sampler._thread.join(timeout=3)

        assert len(sampler.history) <= 5
