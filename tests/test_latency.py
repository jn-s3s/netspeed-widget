"""Tests for the TCP-connect latency probe with mocked sockets."""

import ipaddress
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

import utils.latency
from utils.latency import PING_HOST, PING_PORT, LatencyProbe


class TestLatencyProbe:
    """Cover start/stop lifecycle and TCP/ICMP measurement paths."""

    @patch("utils.latency.time.time", return_value=10.0)
    @patch("utils.latency.time.perf_counter", side_effect=[0.0, 1.234])
    @patch("socket.create_connection")
    def test_tcp_success_measures_latency(
        self, mock_connect, mock_perf, mock_time
    ) -> None:
        probe = LatencyProbe()
        result = probe._measure()
        assert result.ok is True
        assert result.ms == pytest.approx(1234.0, rel=1e-3)
        assert result.ts == 10.0

    def test_timing_avoids_the_coarse_windows_clock(self) -> None:
        """A sub-tick handshake must not collapse to zero.

        Windows backs time.monotonic with GetTickCount64, which only advances
        every 15.6 ms. Timing a 3 ms connect on that clock reads back as 0 or
        15.6, so the probe has to use perf_counter (QPC) instead.
        """
        source = Path(utils.latency.__file__).read_text(encoding="utf-8")

        assert "time.perf_counter()" in source
        assert "time.monotonic()" not in source

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


class TestProbeTarget:
    """The target has to report the link, not one CDN's load balancer."""

    def test_default_target_is_an_anycast_address_not_a_hostname(self) -> None:
        """A hostname would fold DNS and CDN routing into the reading.

        fast.com is a speedtest host and sits far from many users, so its
        round trip measures their distance to Netflix's edge rather than their
        own connection quality. An anycast IP is answered by the nearest
        endpoint, which is what makes the number mean "my link".
        """
        assert ipaddress.ip_address(PING_HOST)
        assert PING_PORT == 443

    def test_probe_defaults_to_the_configured_target(self) -> None:
        assert LatencyProbe().host == PING_HOST
        assert LatencyProbe().port == PING_PORT
