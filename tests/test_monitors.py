"""Monitor geometry tests, with the Win32 calls stubbed.

These are the helpers that decide whether the widget is stranded off a
detached screen, so they are tested without a display or a Tk window.
"""

from unittest.mock import MagicMock

import pytest

from utils import monitors
from utils.logger import Logger


@pytest.fixture
def spy(monkeypatch):
    mock = MagicMock(spec=Logger)
    monkeypatch.setattr(monitors, "_log", mock)
    return mock


class TestActiveMonitorRects:
    def test_rects_are_returned_as_tuples(self, monkeypatch):
        monkeypatch.setattr(
            monitors.win32api,
            "EnumDisplayMonitors",
            lambda: [
                (None, None, (0, 0, 1920, 1080)),
                (None, None, (1920, 0, 3840, 1080)),
            ],
        )

        assert monitors.active_monitor_rects() == [
            (0, 0, 1920, 1080),
            (1920, 0, 3840, 1080),
        ]

    def test_a_display_that_refuses_to_answer_yields_no_rects(self, monkeypatch, spy):
        def refuse():
            raise OSError("no display attached")

        monkeypatch.setattr(monitors.win32api, "EnumDisplayMonitors", refuse)

        assert monitors.active_monitor_rects() == []
        spy.warning.assert_called_once()


PRIMARY = [(0, 0, 1920, 1080)]
SECOND_SCREEN = [(0, 0, 1920, 1080), (1920, 0, 3840, 1080)]
NO_RECTS: list[tuple[int, int, int, int]] = []


class TestVisibility:
    @pytest.mark.parametrize(
        ("x", "y", "expected"),
        [
            (0, 0, True),
            (1700, 1000, True),
            (3000, 900, False),
            (-400, 100, False),
        ],
    )
    def test_window_center_decides_visibility(self, x, y, expected):
        inside = monitors.point_on_active_monitor(x, y, 340, 52, rects=PRIMARY)

        assert inside is expected

    def test_the_right_and_bottom_edges_are_exclusive(self):
        """Win32 rects are half-open, so a center on the edge is outside."""
        inside = monitors.point_on_active_monitor(1749, 10, 340, 52, rects=PRIMARY)
        on_edge_x = monitors.point_on_active_monitor(1750, 10, 340, 52, rects=PRIMARY)
        on_edge_y = monitors.point_on_active_monitor(10, 1054, 340, 52, rects=PRIMARY)

        assert inside is True
        assert on_edge_x is False
        assert on_edge_y is False

    def test_a_second_monitor_rescues_an_off_primary_position(self):
        assert monitors.point_on_active_monitor(2500, 400, 340, 52, rects=SECOND_SCREEN)

    def test_an_unreadable_layout_is_treated_as_visible(self, monkeypatch):
        monkeypatch.setattr(monitors, "active_monitor_rects", lambda: NO_RECTS)

        assert monitors.point_on_active_monitor(9999, 9999, 340, 52) is True


class TestDefaultPosition:
    def test_position_is_the_work_area_corner_offset_by_the_margin(self, monkeypatch):
        monitor = object()
        monkeypatch.setattr(
            monitors.win32api,
            "MonitorFromPoint",
            lambda _point, _flag: monitor,
        )
        monkeypatch.setattr(
            monitors.win32api,
            "GetMonitorInfo",
            lambda _m: {"Monitor": (0, 0, 1920, 1080), "Work": (0, 0, 1920, 1040)},
        )

        assert monitors.default_position(340, 52, 12) == (1568, 976)

    def test_an_unresolvable_display_falls_back_to_a_visible_spot(
        self, monkeypatch, spy
    ):
        def refuse(*_a):
            raise OSError("no monitors at all")

        monkeypatch.setattr(monitors.win32api, "MonitorFromPoint", refuse)

        assert monitors.default_position(340, 52, 12) == (100, 100)
        spy.warning.assert_called_once()
