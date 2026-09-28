"""Rolling traffic plot drawn into a fixed slot of the widget canvas."""

import tkinter as tk
from collections.abc import Sequence
from dataclasses import dataclass, field

from utils.sampler import NetSample
from utils.theme import (
    DEFAULT_THEME,
    Health,
    Palette,
    palette_for,
    status_download_color,
    status_fill_color,
    status_upload_color,
)

BASELINE_OFFSET = 0.5
SCALE_DECAY = 0.97
FLOOR_MBPS = 1.0
AREA_INSET = 2


@dataclass
class TrafficGraph:
    """Draws samples as a filled down area plus two lines, scaled to a peak.

    The scale relaxes gradually rather than snapping to the visible window,
    so a spike holds the axis steady for a few seconds instead of rescaling
    every frame. A closed slot has no axis worth drawing, so the peak floor
    keeps the divisor away from zero.

    Args:
        canvas: Canvas the items are created on.
        x0: Left edge of the slot.
        width: Slot width in pixels.
        top: Slot top edge in pixels.
        bottom: Slot bottom edge, which is also the zero baseline.
        capacity: Sample count a full window holds, which fixes the x step.
    """

    canvas: tk.Canvas
    x0: int
    width: int
    top: int
    bottom: int
    capacity: int
    _scale: float = field(default=1.0, init=False)
    palette: Palette = field(default_factory=lambda: palette_for(DEFAULT_THEME))

    def draw(
        self,
        samples: Sequence[NetSample],
        health: Health = Health.GOOD,
        status_events: Sequence[tuple[float, Health]] = (),
    ) -> None:
        """Redraw the slot, retaining the health color of each past segment.

        `health` only colors the window when `status_events` is empty; once
        events are supplied they alone decide each segment, so a caller cannot
        pass both and expect the fallback to be honored.
        """
        self.canvas.delete("graph")
        if len(samples) < 2:
            return

        peak = max(
            max(sample.down_mbps for sample in samples),
            max(sample.up_mbps for sample in samples),
            FLOOR_MBPS,
        )
        if peak > self._scale:
            self._scale = peak
        else:
            self._scale = max(peak, self._scale * SCALE_DECAY)

        step = self.width / (self.capacity - 1)
        start_x = self.x0 + self.width - (len(samples) - 1) * step
        base = float(self.bottom)

        self.canvas.create_line(
            self.x0,
            base + BASELINE_OFFSET,
            self.x0 + self.width,
            base + BASELINE_OFFSET,
            fill=self.palette.border,
            tags="graph",
        )

        down_points: list[float] = []
        up_points: list[float] = []
        for index, sample in enumerate(samples):
            x = start_x + index * step
            down_points.extend((x, self._to_y(sample.down_mbps)))
            up_points.extend((x, self._to_y(sample.up_mbps)))

        segment_colors: list[Health] = []
        event_index = 0
        for sample in samples[1:]:
            while (
                event_index + 1 < len(status_events)
                and status_events[event_index + 1][0] <= sample.ts
            ):
                event_index += 1
            segment_colors.append(
                status_events[event_index][1] if status_events else health
            )

        runs: list[tuple[int, int, Health]] = []
        run_start = 0
        for index in range(1, len(segment_colors)):
            if segment_colors[index] != segment_colors[run_start]:
                runs.append((run_start, index, segment_colors[run_start]))
                run_start = index
        runs.append((run_start, len(segment_colors), segment_colors[run_start]))

        for first, last, run_health in runs:
            points = down_points[first * 2 : (last + 1) * 2]
            self.canvas.create_polygon(
                points[0],
                base,
                *points,
                points[-2],
                base,
                fill=status_fill_color(run_health, self.palette),
                outline="",
                tags="graph",
            )
        for first, last, run_health in runs:
            self.canvas.create_line(
                *down_points[first * 2 : (last + 1) * 2],
                fill=status_download_color(run_health, self.palette),
                width=2,
                tags="graph",
            )
        for first, last, run_health in runs:
            self.canvas.create_line(
                *up_points[first * 2 : (last + 1) * 2],
                fill=status_upload_color(run_health, self.palette),
                width=1,
                tags="graph",
            )

    def _to_y(self, value: float) -> float:
        """Map a Mb/s value onto the slot, scaled by the current peak."""
        height = self.bottom - self.top - AREA_INSET
        return self.bottom - (value / self._scale) * height
