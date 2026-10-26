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
        let mut points = Vec::with_capacity(buckets.len());
        for (index, bucket) in buckets.into_iter().enumerate() {
            let Some((down, up, ts)) = bucket else {
                continue;
            };
            let x = (slot.x + index as f32).min(slot.right());
            points.push((index, (x, to_y(down)), (x, to_y(up)), ts));
        }
        let monochrome = (palette.down.r, palette.down.g, palette.down.b)
            == (palette.up.r, palette.up.g, palette.up.b);
        let mut start = 0;
        for end in 1..=points.len() {
            if end < points.len() && points[end].3 - points[end - 1].3 <= 2.5 {
                continue;
            }
            let run = &points[start..end];
            start = end;
            if run.len() < 2 {
                continue;
            }
            // Composite each connected path once. Separately blending round caps
            // at every sample makes their partially covered joins visibly heavier.
            let down: Vec<_> = run.iter().map(|point| point.1).collect();
            let up: Vec<_> = run.iter().map(|point| point.2).collect();
            if fill {
                let mut area = Vec::with_capacity(down.len() + 2);
                area.push((down[0].0, slot.bottom()));
                area.extend_from_slice(&down);
                area.push((down[down.len() - 1].0, slot.bottom()));
                canvas.fill_polygon(&area, palette.down.with_alpha(FILL_OPACITY));
            }
            canvas.stroke_polyline(&down, 1.25 * dpi_scale, palette.down);
            if monochrome {
                let mut dash_start = 0;
                for (end, point) in run.iter().enumerate().skip(1) {
                    if point.0 % 4 >= 2 {
                        canvas.stroke_polyline(&up[dash_start..end], dpi_scale, palette.up);
                        dash_start = end;
                    }
                }
                canvas.stroke_polyline(&up[dash_start..], dpi_scale, palette.up);
            } else {
                canvas.stroke_polyline(&up, dpi_scale, palette.up);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady_samples() -> Vec<NetSample> {
        let now = crate::now_epoch();
        (0..60)
            .map(|index| NetSample {
                ts: now - 59.0 + index as f64,
                down_mbps: 10.0,
                up_mbps: 0.0,
            })
            .collect()
    }

    #[test]
    fn steady_line_has_uniform_coverage_including_sample_joins() {
        let mut palette = crate::theme::palette_for("Default");
        palette.down.a = 0.5;
        let mut canvas = Canvas::new(44, 26);
        TrafficGraph::new().draw(
            &mut canvas,
            Rect::new(0.0, 0.0, 44.0, 26.0),
            &steady_samples(),
            &palette,
            false,
            1.0,
        );
        let alpha = |x: usize| canvas.pixels()[(44 + x) * 4 + 3];
        assert!(alpha(4) > 0);
        for x in 4..40 {
            assert_eq!(alpha(x), alpha(4), "uneven coverage at column {x}");
        }
    }

    #[test]
    fn monochrome_upload_keeps_visible_dash_gaps() {
        let mut palette = crate::theme::palette_for("Default");
        palette.up = palette.down;
        let mut canvas = Canvas::new(120, 26);
        TrafficGraph::new().draw(
            &mut canvas,
            Rect::new(0.0, 0.0, 120.0, 26.0),
            &steady_samples(),
            &palette,
            false,
            1.0,
        );
        let row = &canvas.pixels()[25 * 120 * 4..26 * 120 * 4];
        assert!(row.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        assert!(row.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
    }

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
