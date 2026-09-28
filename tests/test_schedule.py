"""Tests for when the next automatic speedtest is due."""

from utils.schedule import next_speedtest_due

INTERVAL = 4 * 60 * 60
GRACE = 20


def _record(ts: float) -> dict:
    return {"down_mbps": 50.0, "up_mbps": 10.0, "ts": ts}


class TestNextDue:
    def test_nothing_ever_measured_uses_the_startup_grace(self):
        assert next_speedtest_due(None, 1_000.0, INTERVAL, GRACE) == 1_020.0

    def test_a_recent_result_schedules_one_full_interval_after_it(self):
        due = next_speedtest_due(_record(900.0), 1_000.0, INTERVAL, GRACE)

        assert due == 900.0 + INTERVAL

    def test_a_stale_result_falls_back_to_the_grace(self):
        due = next_speedtest_due(
            _record(1_000.0 - 10 * INTERVAL), 1_000.0, 3600.0, GRACE
        )

        assert due == 1_020.0

    def test_a_due_exactly_now_is_treated_as_overdue(self):
        """The comparison is strict, so a run due this instant is not skipped."""
        due = next_speedtest_due(_record(900.0), 1_000.0, 100.0, GRACE)

        assert due == 1_020.0

    def test_the_returned_time_is_always_in_the_future(self):
        for ts in (0.0, 500.0, 999.0, 1_000.0, 1_000.5, 5_000.0):
            due = next_speedtest_due(_record(ts), 1_000.0, INTERVAL, GRACE)
            assert due > 1_000.0
