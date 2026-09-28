"""Tests for the sampler rate math, driven without the polling thread."""

import pytest

from utils.sampler import NetSampler


def test_first_reading_only_seeds_the_baseline() -> None:
    sampler = NetSampler()
    sampler._record_sample(1_000, 2_000, now=100.0)
    assert sampler.latest is None
    assert sampler.session_totals == (0.0, 0.0)


def test_rate_math_uses_byte_delta_over_elapsed_seconds() -> None:
    sampler = NetSampler()
    sampler._record_sample(1_000, 2_000, now=100.0)
    sampler._record_sample(2_000, 6_000, now=101.0)

    latest = sampler.latest
    assert latest is not None
    assert latest.down_mbps == pytest.approx(4_000 * 8 / 1e6)
    assert latest.up_mbps == pytest.approx(1_000 * 8 / 1e6)

    down_mb, up_mb = sampler.session_totals
    assert down_mb == pytest.approx(0.004)
    assert up_mb == pytest.approx(0.001)


def test_zero_elapsed_is_clamped_instead_of_dividing_by_zero() -> None:
    sampler = NetSampler()
    sampler._record_sample(0, 0, now=50.0)
    sampler._record_sample(8, 8, now=50.0)

    latest = sampler.latest
    assert latest is not None
    assert latest.down_mbps == pytest.approx(8 * 8 / 1e6 / 1e-6)


def test_counter_reset_yields_zero_rate_not_negative() -> None:
    sampler = NetSampler()
    sampler._record_sample(5_000, 5_000, now=10.0)
    sampler._record_sample(500, 500, now=11.0)

    latest = sampler.latest
    assert latest is not None
    assert latest.down_mbps == 0.0
    assert latest.up_mbps == 0.0
    assert sampler.session_totals == (0.0, 0.0)


def test_history_window_trims_to_maxlen() -> None:
    sampler = NetSampler(history=2)
    sampler._record_sample(0, 0, now=0.0)
    sampler._record_sample(8, 8, now=1.0)
    sampler._record_sample(24, 24, now=2.0)

    history = sampler.history
    assert len(history) == 2
    assert history[1].down_mbps == pytest.approx(2 * history[0].down_mbps)
