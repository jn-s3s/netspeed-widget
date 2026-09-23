"""Rolling traffic plot drawn into a fixed slot of the widget canvas."""

import tkinter as tk
from collections.abc import Sequence
from dataclasses import dataclass, field

from utils.sampler import NetSample
from utils.theme import BORDER, DOWN_COLOR, DOWN_FILL, UP_COLOR

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

    def draw(self, samples: Sequence[NetSample]) -> None:
        """Redraw the slot for `samples`, oldest first. No-op if too short."""
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
        end_x = start_x + (len(samples) - 1) * step
        base = float(self.bottom)

        self.canvas.create_line(
            self.x0,
            base + BASELINE_OFFSET,
            self.x0 + self.width,
            base + BASELINE_OFFSET,
            fill=BORDER,
            tags="graph",
        )

        down_points: list[float] = []
        up_points: list[float] = []
        for index, sample in enumerate(samples):
            x = start_x + index * step
            down_points.extend((x, self._to_y(sample.down_mbps)))
            up_points.extend((x, self._to_y(sample.up_mbps)))

        self.canvas.create_polygon(
            start_x,
            base,
            *down_points,
            end_x,
            base,
            fill=DOWN_FILL,
            outline="",
            tags="graph",
        )
        self.canvas.create_line(*down_points, fill=DOWN_COLOR, width=2, tags="graph")
        self.canvas.create_line(*up_points, fill=UP_COLOR, width=1, tags="graph")

    def _to_y(self, value: float) -> float:
        """Map a Mb/s value onto the slot, scaled by the current peak."""
        height = self.bottom - self.top - AREA_INSET
        return self.bottom - (value / self._scale) * height
