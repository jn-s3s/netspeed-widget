"""Traffic graph scaling and geometry tests drawn onto a recorded fake canvas."""

import pytest

from conftest import FakeCanvas
from utils.graph import TrafficGraph
from utils.sampler import NetSample


def _graph() -> TrafficGraph:
    return TrafficGraph(
        canvas=FakeCanvas(), x0=190, width=142, top=6, bottom=46, capacity=60
    )


def _series(pairs):
    return [NetSample(down, up, float(index)) for index, (down, up) in enumerate(pairs)]


def _data_lines(canvas: FakeCanvas) -> list[list[float]]:
    """The down and up polylines, in draw order, excluding the baseline.

    The baseline is the only line drawn without a width, which is what tells
    the three apart without depending on how many points each has.
    """
    lines = [
        item
        for item in canvas.items.values()
        if item["kind"] == "line" and "width" in item
    ]
    return [item["points"] for item in lines]


class TestEmptyAndShort:
    def test_a_short_window_draws_nothing_but_clears_the_slot(self):
        graph = _graph()

        graph.draw(_series([(5.0, 5.0)]))

        assert graph.canvas.deleted_tags == ["graph"]
        assert graph.canvas.items == {}

    def test_no_samples_at_all_is_safe(self):
        graph = _graph()

        graph.draw([])

        assert graph.canvas.items == {}

    def test_two_samples_start_anchored_to_the_right_edge(self):
        graph = _graph()

        graph.draw(_series([(1.0, 1.0), (2.0, 2.0)]))

        step = graph.width / (graph.capacity - 1)
        down_line = _data_lines(graph.canvas)[0]
        assert down_line[0] == pytest.approx(graph.x0 + graph.width - step)
        assert down_line[-2] == pytest.approx(graph.x0 + graph.width)


class TestScaling:
    def test_a_spike_becomes_the_scale_at_once(self):
        graph = _graph()

        graph.draw(_series([(10.0, 0.0), (500.0, 0.0)]))

        assert graph._scale == 500.0

    def test_a_quieter_window_relaxes_the_scale_gradually(self):
        graph = _graph()
        graph.draw(_series([(400.0, 0.0), (400.0, 0.0)]))
        before = graph._scale

        graph.draw(_series([(1.0, 1.0), (1.0, 1.0)]))

        assert graph._scale == pytest.approx(before * 0.97)

    def test_the_scale_never_drops_below_the_visible_peak(self):
        graph = _graph()
        graph.draw(_series([(100.0, 0.0), (100.0, 0.0)]))

        for _ in range(40):
            graph.draw(_series([(0.5, 0.5), (0.5, 0.5)]))

        assert graph._scale >= 0.5

    def test_silence_cannot_divide_by_zero(self):
        graph = _graph()
        graph.draw(_series([(0.0, 0.0), (0.0, 0.0)]))

        graph.draw(_series([(0.0, 0.0), (0.0, 0.0)]))

        assert graph._scale >= 1.0

    def test_zero_sits_on_the_baseline_and_the_peak_on_the_top_inset(self):
        graph = _graph()
        graph.draw(_series([(0.0, 0.0), (250.0, 0.0)]))

        assert graph._to_y(0.0) == graph.bottom
        assert graph._to_y(graph._scale) == pytest.approx(graph.top + 2)


class TestDrawing:
    def test_every_redraw_clears_the_previous_items(self):
        graph = _graph()
        samples = _series([(3.0, 1.0), (4.0, 2.0)])

        graph.draw(samples)
        graph.draw(samples)

        assert graph.canvas.deleted_tags == ["graph", "graph"]

    def test_all_items_stay_inside_the_slot(self):
        graph = _graph()

        graph.draw(_series([(1_000.0, 900.0), (2.0, 3.0), (800.0, 10.0)]))

        for item in graph.canvas.items.values():
            points = item.get("points")
            if not points:
                continue
            xs = points[0::2]
            ys = points[1::2]
            assert all(graph.x0 - 1 <= x <= graph.x0 + graph.width + 1 for x in xs)
            assert all(graph.top - 1 <= y <= graph.bottom + 1 for y in ys)
