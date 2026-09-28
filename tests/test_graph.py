"""Traffic graph scaling and geometry tests drawn onto a recorded fake canvas."""

import pytest

from conftest import FakeCanvas
from utils.graph import TrafficGraph
from utils.sampler import NetSample
from utils.theme import (
    BAD_FILL,
    DEFAULT_THEME,
    FG_DIM,
    GOOD_FILL,
    UP_COLOR,
    WARN_FILL,
    Health,
    palette_for,
)

DEFAULT_PALETTE = palette_for(DEFAULT_THEME)


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


def _segment_items(canvas: FakeCanvas) -> tuple[list[dict], list[dict], list[dict]]:
    """Per-segment items in draw order: areas, down lines, up lines.

    The graph draws every segment's polygon first, then every segment's
    download line (width 2) and upload line (width 1), so each color run is
    one entry even when `status_events` splits the window.
    """
    polygons = [item for item in canvas.items.values() if item["kind"] == "polygon"]
    down = [
        item
        for item in canvas.items.values()
        if item["kind"] == "line" and item.get("width") == 2
    ]
    up = [
        item
        for item in canvas.items.values()
        if item["kind"] == "line" and item.get("width") == 1
    ]
    return polygons, down, up


def _sample_xs(graph: TrafficGraph, count: int) -> list[float]:
    """The x coordinate of each sample, in order, for `count` samples."""
    step = graph.width / (graph.capacity - 1)
    start_x = graph.x0 + graph.width - (count - 1) * step
    return [start_x + index * step for index in range(count)]


def _down_run(graph: TrafficGraph, samples, first: int, last: int) -> list[float]:
    """Flat (x, y) coords of the download line covering samples first..last."""
    xs = _sample_xs(graph, len(samples))
    full: list[float] = []
    for index, sample in enumerate(samples):
        full.extend((xs[index], graph._to_y(sample.down_mbps)))
    return full[first * 2 : (last + 1) * 2]


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

    @pytest.mark.parametrize(
        ("health", "expected_down", "expected_fill", "expected_upload"),
        [
            (Health.GOOD, DEFAULT_PALETTE.down, GOOD_FILL, UP_COLOR),
            (Health.WARN, DEFAULT_PALETTE.warn, WARN_FILL, DEFAULT_PALETTE.warn),
            (Health.BAD, DEFAULT_PALETTE.bad, BAD_FILL, DEFAULT_PALETTE.bad),
        ],
    )
    def test_health_recolors_lines_and_area(
        self, health, expected_down, expected_fill, expected_upload
    ):
        graph = _graph()

        graph.draw(_series([(3.0, 1.0), (4.0, 2.0)]), health)

        lines = [
            item
            for item in graph.canvas.items.values()
            if item["kind"] == "line" and "width" in item
        ]
        area = next(
            item for item in graph.canvas.items.values() if item["kind"] == "polygon"
        )
        assert [line["fill"] for line in lines] == [
            expected_down,
            expected_upload,
        ]
        assert area["fill"] == expected_fill

    def test_unknown_health_raises(self):
        graph = _graph()

        # Not a Health member: the palette lookup must fail instead of
        # passing the raw value through to the canvas.
        with pytest.raises(KeyError):
            graph.draw(_series([(3.0, 1.0), (4.0, 2.0)]), FG_DIM)

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

    # ---------- status_events segmentation ----------

    def test_single_health_run_draws_one_segment_per_line(self):
        """One event color for the whole window stays a single segment."""
        graph = _graph()
        samples = _series([(3.0, 1.0), (4.0, 2.0), (5.0, 3.0), (2.0, 4.0)])

        graph.draw(samples, Health.GOOD, [(0.0, Health.GOOD)])

        polygons, down, up = _segment_items(graph.canvas)
        assert [len(polygons), len(down), len(up)] == [1, 1, 1]
        assert [line["fill"] for line in down] == [DEFAULT_PALETTE.down]
        assert [line["fill"] for line in up] == [UP_COLOR]
        assert [poly["fill"] for poly in polygons] == [GOOD_FILL]
        # The single segment must span the whole window, not a slice of it.
        assert polygons[0]["points"][0] == pytest.approx(_sample_xs(graph, 4)[0])
        assert polygons[0]["points"][-2] == pytest.approx(_sample_xs(graph, 4)[-1])

    def test_mid_series_transition_splits_into_two_segments(self):
        """A transition between samples splits the window at that boundary."""
        graph = _graph()
        samples = _series([(3.0, 1.0), (4.0, 2.0), (5.0, 3.0), (2.0, 4.0)])

        graph.draw(samples, Health.GOOD, [(0.0, Health.GOOD), (1.5, Health.BAD)])

        polygons, down, up = _segment_items(graph.canvas)
        assert [len(polygons), len(down), len(up)] == [2, 2, 2]
        assert [line["fill"] for line in down] == [
            DEFAULT_PALETTE.down,
            DEFAULT_PALETTE.bad,
        ]
        assert [line["fill"] for line in up] == [UP_COLOR, DEFAULT_PALETTE.bad]
        assert [poly["fill"] for poly in polygons] == [GOOD_FILL, BAD_FILL]
        # The 1.5 ms event sits between samples 1 and 2: sample 2 onwards
        # goes bad, so the good run covers samples 0-1 and the bad one 1-3.
        assert down[0]["points"] == pytest.approx(_down_run(graph, samples, 0, 1))
        assert down[1]["points"] == pytest.approx(_down_run(graph, samples, 1, 3))

    def test_transition_on_a_sample_timestamp_flips_at_that_sample(self):
        """An event at exactly a sample ts recolors the segment ending there.

        The sample at ts == event already reflects the new health, so the
        segment that ends on it turns, not the one after it: a '<' instead of
        '<=' in the event walk would push the boundary one sample to the
        right.
        """
        graph = _graph()
        samples = _series([(3.0, 1.0), (4.0, 2.0), (5.0, 3.0), (2.0, 4.0)])

        graph.draw(samples, Health.GOOD, [(0.0, Health.GOOD), (2.0, Health.BAD)])

        polygons, down, up = _segment_items(graph.canvas)
        assert [len(polygons), len(down), len(up)] == [2, 2, 2]
        assert [line["fill"] for line in down] == [
            DEFAULT_PALETTE.down,
            DEFAULT_PALETTE.bad,
        ]
        # The event on samples[2].ts immediately colors the segment that ends
        # at samples[2], so the good run stays on samples 0-1 and the bad run
        # starts at samples[1]: the boundary sits on samples[1], not one
        # sample later.
        assert down[0]["points"] == pytest.approx(_down_run(graph, samples, 0, 1))
        assert down[1]["points"] == pytest.approx(_down_run(graph, samples, 1, 3))
        assert down[1]["points"][0] == pytest.approx(_sample_xs(graph, 4)[1])

    def test_events_older_than_the_window_do_not_create_segments(self):
        """Only the newest pre-window event colors the start of the window.

        The app prunes every event older than the first sample except the
        baseline, so draw() must not turn each stale event into its own
        segment: the most recent pre-window transition governs the start.
        """
        graph = _graph()
        samples = _series([(3.0, 1.0), (4.0, 2.0), (5.0, 3.0), (2.0, 4.0)])

        graph.draw(
            samples,
            Health.GOOD,
            [
                (-2.0, Health.GOOD),
                (-1.0, Health.WARN),
                (1.5, Health.GOOD),
            ],
        )

        polygons, down, up = _segment_items(graph.canvas)
        assert [len(polygons), len(down), len(up)] == [2, 2, 2]
        # The stale GOOD at -2.0 is superseded by the WARN baseline, and the
        # recovery at 1.5 splits the window once more: WARN for samples 0-1,
        # GOOD for samples 1-3. No third segment for the pre-window events.
        assert [line["fill"] for line in down] == [
            DEFAULT_PALETTE.warn,
            DEFAULT_PALETTE.down,
        ]
        assert [line["fill"] for line in up] == [DEFAULT_PALETTE.warn, UP_COLOR]
        assert [poly["fill"] for poly in polygons] == [WARN_FILL, GOOD_FILL]
        assert down[0]["points"] == pytest.approx(_down_run(graph, samples, 0, 1))
        assert down[1]["points"] == pytest.approx(_down_run(graph, samples, 1, 3))
