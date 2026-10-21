//! A time-based, shared-scale traffic graph. Missing intervals remain gaps.

use crate::canvas::{Canvas, Rect};
use crate::sampler::NetSample;
use crate::theme::Palette;

// The taskbar plot is only 26 logical pixels tall. A 10% fill disappears
// against translucent taskbars; 30% stays distinct without competing with lines.
const FILL_OPACITY: f32 = 0.30;

#[derive(Debug, Default)]
pub struct TrafficGraph {
    scale: f64,
    last_ts: Option<f64>,
}

impl TrafficGraph {
    pub fn new() -> Self {
        Self {
            scale: 1.0,
            last_ts: None,
        }
    }

    pub fn scale(&self) -> f64 {
        self.scale.max(1.0)
    }

    fn update_scale(&mut self, samples: &[NetSample]) {
        let Some(newest) = samples.last() else {
            return;
        };
        let peak = samples
            .iter()
            .filter(|sample| newest.ts - sample.ts <= 60.0)
            .map(|sample| sample.down_mbps.max(sample.up_mbps))
            .filter(|rate| rate.is_finite())
            .fold(1.0, f64::max);
        let elapsed = self
            .last_ts
            .map(|last| (newest.ts - last).clamp(0.0, 60.0))
            .unwrap_or(0.0);
        self.scale = peak.max(self.scale * 0.97_f64.powf(elapsed));
        self.last_ts = Some(newest.ts);
    }

    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        slot: Rect,
        samples: &[NetSample],
        palette: &Palette,
        fill: bool,
        dpi_scale: f32,
    ) {
        self.update_scale(samples);
        let now = crate::now_epoch();
        let oldest = now - 60.0;
        let to_y = |rate: f64| {
            slot.bottom()
                - ((rate.max(0.0) / self.scale()).min(1.0) as f32 * (slot.height - 2.0 * dpi_scale))
        };
        // Per-pixel maxima retain short spikes instead of sampling them away.
        let mut buckets: Vec<Option<(f64, f64, f64)>> =
            vec![None; slot.width.ceil().max(1.0) as usize + 1];
        for sample in samples {
            if sample.ts < oldest
                || sample.ts > now
                || !sample.down_mbps.is_finite()
                || !sample.up_mbps.is_finite()
            {
                continue;
            }
            let index =
                (((sample.ts - oldest) / 60.0 * slot.width as f64) as usize).min(buckets.len() - 1);
            let bucket = &mut buckets[index];
            *bucket = Some(match *bucket {
                Some((down, up, ts)) => (
                    down.max(sample.down_mbps),
                    up.max(sample.up_mbps),
                    ts.max(sample.ts),
                ),
                None => (sample.down_mbps, sample.up_mbps, sample.ts),
            });
        }
        let mut previous: Option<(f32, f64, f64, f64)> = None;
        for (index, bucket) in buckets.into_iter().enumerate() {
            let Some((down, up, ts)) = bucket else {
                continue;
            };
            let x = (slot.x + index as f32).min(slot.right());
            if let Some((last_x, last_down, last_up, last_ts)) = previous {
                if ts - last_ts <= 2.5 {
                    if fill {
                        canvas.fill_polygon(
                            &[
                                (last_x, slot.bottom()),
                                (last_x, to_y(last_down)),
                                (x, to_y(down)),
                                (x, slot.bottom()),
                            ],
                            palette.down.with_alpha(FILL_OPACITY),
                        );
                    }
                    canvas.stroke_polyline(
                        &[(last_x, to_y(last_down)), (x, to_y(down))],
                        1.25 * dpi_scale,
                        palette.down,
                    );
                    let monochrome = (palette.down.r, palette.down.g, palette.down.b)
                        == (palette.up.r, palette.up.g, palette.up.b);
                    if !monochrome || index % 4 < 2 {
                        canvas.stroke_polyline(
                            &[(last_x, to_y(last_up)), (x, to_y(up))],
                            dpi_scale,
                            palette.up,
                        );
                    }
                }
            }
            previous = Some((x, down, up, ts));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_toggle_visibly_shades_active_downloads_but_not_idle_traffic() {
        let now = crate::now_epoch();
        let samples: Vec<_> = (0..60)
            .map(|index| NetSample {
                down_mbps: 10.0,
                up_mbps: 0.0,
                ts: now - 59.0 + index as f64,
            })
            .collect();
        let palette = crate::theme::palette_for("Default");
        let slot = Rect::new(0.0, 0.0, 44.0, 26.0);
        let mut plain = Canvas::new(44, 26);
        let mut filled = Canvas::new(44, 26);
        TrafficGraph::new().draw(&mut plain, slot, &samples, &palette, false, 1.0);
        TrafficGraph::new().draw(&mut filled, slot, &samples, &palette, true, 1.0);
        let interior = ((16 * 44 + 22) * 4) as usize;
        assert_eq!(plain.pixels()[interior + 3], 0);
        assert!(
            filled.pixels()[interior + 3] >= 70,
            "fill should be perceptible at taskbar size"
        );
        let idle: Vec<_> = samples
            .iter()
            .map(|sample| NetSample {
                down_mbps: 0.0,
                ..*sample
            })
            .collect();
        let mut idle_plain = Canvas::new(44, 26);
        let mut idle_filled = Canvas::new(44, 26);
        TrafficGraph::new().draw(&mut idle_plain, slot, &idle, &palette, false, 1.0);
        TrafficGraph::new().draw(&mut idle_filled, slot, &idle, &palette, true, 1.0);
        assert_eq!(
            idle_plain.pixels(),
            idle_filled.pixels(),
            "idle must not get a fabricated filled area"
        );
    }
    #[test]
    fn repaint_does_not_decay_scale() {
        let mut graph = TrafficGraph::new();
        graph.update_scale(&[NetSample {
            down_mbps: 100.0,
            up_mbps: 0.0,
            ts: 1.0,
        }]);
        let quiet = [NetSample {
            down_mbps: 0.0,
            up_mbps: 0.0,
            ts: 62.0,
        }];
        graph.update_scale(&quiet);
        let scale = graph.scale();
        for _ in 0..100 {
            graph.update_scale(&quiet);
        }
        assert_eq!(graph.scale(), scale);
        assert!(scale < 100.0);
    }

    #[test]
    fn missing_intervals_do_not_draw_a_fake_connection() {
        let now = crate::now_epoch();
        let samples = [
            NetSample {
                down_mbps: 10.0,
                up_mbps: 1.0,
                ts: now - 40.0,
            },
            NetSample {
                down_mbps: 10.0,
                up_mbps: 1.0,
                ts: now - 10.0,
            },
        ];
        let mut canvas = Canvas::new(120, 40);
        TrafficGraph::new().draw(
            &mut canvas,
            Rect::new(0.0, 0.0, 120.0, 40.0),
            &samples,
            &crate::theme::palette_for("Default"),
            false,
            1.0,
        );
        assert!(canvas.pixels().iter().all(|byte| *byte == 0));
    }
}
