"""Tests for NetSampler thread start and stop."""

from unittest.mock import MagicMock, patch

from utils.sampler import NetSampler


class TestSamplerLifecycle:
    """Cover start, stop and threading behavior."""

    @patch(
        "utils.sampler.psutil.net_io_counters",
        return_value=MagicMock(bytes_sent=0, bytes_recv=0),
    )
    def test_start_spawns_thread(self, mock_counters) -> None:
        sampler = NetSampler()
        sampler.start()
        assert sampler._thread is not None
        assert sampler._thread.is_alive()
        sampler.stop()

    def test_stop_sets_event(self) -> None:
        sampler = NetSampler()
        sampler.start()
        sampler.stop()
        assert sampler._stop.is_set()
