"""Tests for the speedtest provider chain and fallback behavior."""

from unittest.mock import MagicMock, patch

from utils.speedtest import (
    SpeedtestResult,
    _measure_fast_cli,
    _measure_passive_estimate,
    _measure_python_speedtest,
    measure_speed,
)


class TestProviderChain:
    """Cover individual providers and the ordered chain."""

    @patch(
        "utils.speedtest._run_fast_cli",
        return_value={"speeds": {"download": 10, "upload": 5}},
    )
    def test_fast_cli_success(self, mock_run) -> None:
        result = _measure_fast_cli()
        assert result == SpeedtestResult(down_mbps=10.0, up_mbps=5.0)

    @patch("utils.speedtest._run_fast_cli", return_value=None)
    @patch("builtins.__import__", side_effect=ImportError)
    def test_python_speedtest_missing_import(self, mock_imp, mock_fast) -> None:
        result = _measure_python_speedtest()
        assert result is None

    @patch(
        "psutil.net_io_counters",
        side_effect=[
            MagicMock(bytes_sent=0, bytes_recv=0),
            MagicMock(bytes_sent=100, bytes_recv=200),
        ],
    )
    @patch("utils.speedtest.time.sleep")
    @patch("utils.speedtest.time.time", side_effect=[100, 110])
    def test_passive_estimate_computes_rate(
        self, mock_time, mock_sleep, mock_counters
    ) -> None:
        result = _measure_passive_estimate()
        assert result is not None
        assert result.down_mbps > 0

    @patch("utils.speedtest._measure_fast_cli", return_value=SpeedtestResult(50, 20))
    def test_measure_speed_uses_first_success(self, mock_fast) -> None:
        result = measure_speed()
        assert result == SpeedtestResult(50, 20)

    @patch("utils.speedtest._measure_fast_cli", return_value=None)
    @patch("utils.speedtest._measure_python_speedtest", return_value=None)
    @patch(
        "utils.speedtest._measure_passive_estimate", return_value=SpeedtestResult(1, 1)
    )
    def test_measure_speed_fallback_to_passive(
        self, mock_passive, mock_python, mock_fast
    ) -> None:
        result = measure_speed()
        assert result == SpeedtestResult(1, 1)
