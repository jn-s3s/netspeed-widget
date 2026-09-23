"""Provider chain tests: ordering, fallback limits and result provenance.

The ordering assertions are the point of this file. Every provider returns a
plausible SpeedtestResult, so a chain that is merely "some order" passes with
the passive estimate first, which would present ten seconds of incidental
traffic as the user's line rate.
"""

from unittest.mock import MagicMock

import pytest

from utils import speedtest as sp
from utils.speedtest import (
    SOURCE_FAST,
    SOURCE_PASSIVE,
    SOURCE_SPEEDTEST,
    SpeedtestResult,
    _parse_fast_result,
    measure_speed,
)


def _counter_pair(sent_start, recv_start, sent_end, recv_end):
    return [
        MagicMock(bytes_sent=sent_start, bytes_recv=recv_start),
        MagicMock(bytes_sent=sent_end, bytes_recv=recv_end),
    ]


@pytest.fixture
def quiet_clock(monkeypatch):
    """Make the 10 second passive window instant and deterministic."""
    ticks = iter([0.0, 10.0, 20.0, 30.0])
    monkeypatch.setattr(sp.time, "monotonic", lambda: next(ticks))
    monkeypatch.setattr(sp.time, "sleep", lambda _seconds: None)


class TestChainOrder:
    def test_providers_are_tried_fast_then_speedtest_cli_then_passive(
        self, monkeypatch
    ):
        order = []

        def recorder(tag, outcome):
            def provider():
                order.append(tag)
                return outcome

            return provider

        monkeypatch.setattr(
            sp,
            "_measure_fast_cli",
            recorder("fast", SpeedtestResult(1, 1, SOURCE_FAST)),
        )
        monkeypatch.setattr(sp, "_measure_python_speedtest", recorder("cli", None))
        monkeypatch.setattr(sp, "_measure_passive_estimate", recorder("passive", None))

        assert measure_speed().source == SOURCE_FAST
        assert order == ["fast"]

    def test_all_three_are_reached_before_giving_up(self, monkeypatch):
        seen = []

        def recorder(tag):
            def provider():
                seen.append(tag)

            return provider

        monkeypatch.setattr(sp, "_measure_fast_cli", recorder("fast"))
        monkeypatch.setattr(sp, "_measure_python_speedtest", recorder("cli"))
        monkeypatch.setattr(sp, "_measure_passive_estimate", recorder("passive"))

        with pytest.raises(RuntimeError) as exc:
            measure_speed()

        assert seen == ["fast", "cli", "passive"]
        message = str(exc.value)
        assert SOURCE_FAST in message
        assert SOURCE_SPEEDTEST in message
        assert SOURCE_PASSIVE in message

    def test_a_raising_provider_falls_through_to_the_next(self, monkeypatch):
        def boom():
            raise ValueError("backend exploded")

        monkeypatch.setattr(sp, "_measure_fast_cli", boom)
        monkeypatch.setattr(
            sp,
            "_measure_python_speedtest",
            lambda: SpeedtestResult(30.0, 5.0, SOURCE_SPEEDTEST),
        )
        monkeypatch.setattr(
            sp, "_measure_passive_estimate", lambda: pytest.fail("should not run")
        )

        result = measure_speed()

        assert result == SpeedtestResult(30.0, 5.0, SOURCE_SPEEDTEST)

    def test_a_provider_returning_none_is_not_a_result(self, monkeypatch):
        monkeypatch.setattr(sp, "_measure_fast_cli", lambda: None)
        monkeypatch.setattr(sp, "_measure_python_speedtest", lambda: None)
        monkeypatch.setattr(
            sp,
            "_measure_passive_estimate",
            lambda: SpeedtestResult(2.0, 1.0, SOURCE_PASSIVE),
        )

        result = measure_speed()

        assert result.source == SOURCE_PASSIVE
        assert result.measured is False


class TestPassiveEstimate:
    def test_a_window_with_real_traffic_yields_an_estimate(
        self, monkeypatch, quiet_clock
    ):
        counters = _counter_pair(1_000, 2_000, 10_000_000 + 1_000, 2_000)
        monkeypatch.setattr(sp.psutil, "net_io_counters", lambda: counters.pop(0))

        result = sp._measure_passive_estimate()

        assert result is not None
        assert result.source == SOURCE_PASSIVE
        assert result.measured is False
        assert result.up_mbps == pytest.approx(8.0)

    def test_an_idle_window_is_reported_as_no_result(self, monkeypatch, quiet_clock):
        counters = _counter_pair(500, 500, 560, 540)
        monkeypatch.setattr(sp.psutil, "net_io_counters", lambda: counters.pop(0))

        assert sp._measure_passive_estimate() is None

    def test_a_counter_reset_cannot_produce_a_negative_speed(
        self, monkeypatch, quiet_clock
    ):
        """An adapter reset drops a counter below the previous reading.

        The affected direction has to clamp to zero rather than report the
        negative difference the raw subtraction would give.
        """
        counters = _counter_pair(0, 10**9, 20_000_000, 5_000_000)
        monkeypatch.setattr(sp.psutil, "net_io_counters", lambda: counters.pop(0))

        result = sp._measure_passive_estimate()

        assert result is not None
        assert result.down_mbps == 0.0
        assert result.up_mbps == pytest.approx(16.0)


class TestFastCliBackend:
    def test_bundled_node_is_preferred_over_path(self, monkeypatch):
        monkeypatch.setattr(
            sp, "_run_node_bundle_fast", lambda **_kw: {"download": 9.0, "upload": 1.0}
        )
        monkeypatch.setattr(
            sp, "_run_path_fast", lambda **_kw: pytest.fail("PATH used first")
        )

        assert sp._run_fast_cli() == {"download": 9.0, "upload": 1.0}

    def test_missing_bundle_files_return_without_running_anything(self, monkeypatch):
        missing = MagicMock(is_file=lambda: False)
        monkeypatch.setattr(sp, "bundled_node_path", lambda: missing)
        monkeypatch.setattr(sp, "fast_cli_entry_path", lambda: missing)
        spy = MagicMock()
        monkeypatch.setattr(sp.subprocess, "run", spy)

        assert sp._run_node_bundle_fast() is None
        spy.assert_not_called()


class TestResultShape:
    def test_measured_flag_separates_measurements_from_estimates(self):
        assert SpeedtestResult(1, 1, SOURCE_FAST).measured is True
        assert SpeedtestResult(1, 1, SOURCE_SPEEDTEST).measured is True
        assert SpeedtestResult(1, 1, SOURCE_PASSIVE).measured is False

    def test_parse_accepts_the_shapes_fast_cli_emits(self):
        assert _parse_fast_result({"downloadSpeed": 4.0, "uploadSpeed": 2.0}) == (
            4.0,
            2.0,
        )
        assert _parse_fast_result({"speeds": {"download": 4, "upload": 2}}) == (
            4.0,
            2.0,
        )

    def test_a_missing_upload_discards_the_whole_result(self):
        assert _parse_fast_result({"downloadSpeed": 4.0}) == (None, None)
