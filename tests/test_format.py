"""Tests for speed display formatting."""

import pytest

from utils.format import format_speed


@pytest.mark.parametrize(
    ("mbps", "expected"),
    [
        (0.0, "0.00"),
        (0.5, "0.50"),
        (9.99, "9.99"),
        (10.0, "10.0"),
        (99.9, "99.9"),
        (100.0, "100"),
        (1000.0, "1000"),
    ],
)
def test_format_speed_picks_precision_by_magnitude(mbps: float, expected: str) -> None:
    assert format_speed(mbps) == expected
