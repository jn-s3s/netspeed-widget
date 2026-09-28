"""Tests for the TCP-connect latency probe with mocked sockets."""

from unittest.mock import MagicMock, patch

import pytest

from utils.latency import LatencyProbe


class TestLatencyProbe:
    """Cover start/stop lifecycle and TCP/ICMP measurement paths."""

    @patch("utils.latency.time.time", return_value=10.0)
    @patch("utils.latency.time.monotonic", side_effect=[0.0, 1.234])
    @patch("socket.create_connection")
    def test_tcp_success_measures_latency(
        self, mock_connect, mock_mono, mock_time
    ) -> None:
        probe = LatencyProbe()
        result = probe._measure()
        assert result.ok is True
        assert result.ms == pytest.approx(1234.0, rel=1e-3)
        assert result.ts == 10.0

    @patch("utils.latency.socket.create_connection", side_effect=OSError)
    @patch("subprocess.run", return_value=MagicMock(returncode=1))
    def test_icmp_fallback_offline(self, mock_sub, mock_sock) -> None:
        probe = LatencyProbe()
        result = probe._measure()
        assert result.ok is False
        assert result.ms is None

    @patch("utils.latency.socket.create_connection", side_effect=OSError)
    @patch("subprocess.run", return_value=MagicMock(returncode=0))
    def test_icmp_fallback_online_no_latency(self, mock_sub, mock_sock) -> None:
        probe = LatencyProbe()
        result = probe._measure()
        assert result.ok is True
        assert result.ms is None
