"""Tests for fast-cli JSON parsing in the speedtest provider chain."""

from utils.speedtest import _parse_fast_result


def test_top_level_speed_keys() -> None:
    data = {"downloadSpeed": 94.3, "uploadSpeed": 48.1}
    assert _parse_fast_result(data) == (94.3, 48.1)


def test_top_level_short_keys() -> None:
    data = {"download": 10, "upload": 5}
    assert _parse_fast_result(data) == (10.0, 5.0)


def test_nested_speeds_object() -> None:
    data = {"speeds": {"download": 100.5, "upload": 20.25}}
    assert _parse_fast_result(data) == (100.5, 20.25)


def test_string_numbers_are_converted() -> None:
    data = {"downloadSpeed": "94.3", "uploadSpeed": "48.1"}
    assert _parse_fast_result(data) == (94.3, 48.1)


def test_zero_speeds_are_kept() -> None:
    data = {"downloadSpeed": 0, "uploadSpeed": 0}
    assert _parse_fast_result(data) == (0.0, 0.0)


def test_garbage_top_level_falls_through_to_nested() -> None:
    data = {
        "downloadSpeed": "n/a",
        "uploadSpeed": "n/a",
        "speeds": {"download": 10, "upload": 5},
    }
    assert _parse_fast_result(data) == (10.0, 5.0)


def test_missing_upload_fails_whole_result() -> None:
    assert _parse_fast_result({"downloadSpeed": 94.3}) == (None, None)


def test_non_dict_input_returns_none_pair() -> None:
    assert _parse_fast_result(None) == (None, None)
    assert _parse_fast_result("not json") == (None, None)
    assert _parse_fast_result({}) == (None, None)
