//! DPI-scaled, stable-column taskbar readouts. Historical data belongs in details.

use crate::canvas::{Canvas, Color, Rect, TextRenderer};
use crate::config::DisplayLayout;
use crate::format::RateText;
use crate::graph::TrafficGraph;
use crate::latency::{latency_label, DisplayState};
use crate::sampler::NetSample;
use crate::theme::{alert_color, Palette};
use crate::win::gdi::Font;

pub struct Fonts {
    pub value: Font,
    pub unit: Font,
    pub scale: f32,
    digit_cell: f32,
    value_right: f32,
    rate_width: f32,
    status_width: f32,
}

impl Fonts {
    pub fn for_dpi(dpi: u32, single_line: bool, height: i32) -> Option<Self> {
        let scale = dpi.max(96) as f32 / 96.0;
        let logical_height = height as f32 / scale;
        let size = if single_line {
            13.0
        } else {
            ((logical_height - 6.0) / 2.0).clamp(11.0, 13.0)
        };
        let value = Font::new("Segoe UI", (size * scale).round() as i32, 600)?;
        let digit_cell = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]
            .into_iter()
            .map(|digit| value.measure(digit))
            .fold(0.0_f32, f32::max);
        let unit = Font::new("Segoe UI", (11.0 * scale).round() as i32, 400)?;
        let number_width = ["999+", "99.9", "9.99", "<0.01", "—"]
            .into_iter()
            .map(|text| {
                text.chars()
                    .map(|c| {
                        if c.is_ascii_digit() {
                            digit_cell
                        } else {
                            value.measure(&c.to_string())
                        }
                    })
                    .sum::<f32>()
            })
            .fold(0.0_f32, f32::max);
        let value_right = value.measure("↓").max(value.measure("↑")) + 6.0 * scale + number_width;
        let unit_width = [
            "Kbps", "Mbps", "Gbps", "Tbps", "kB/s", "MB/s", "GB/s", "TB/s",
        ]
        .into_iter()
        .map(|text| unit.measure(text))
        .fold(0.0_f32, f32::max);
        let mut status_width = ["SLOW", "CHECK", "FAIL", "TEST", "—", ">1 s"]
            .into_iter()
            .map(|text| value.measure(text))
            .fold(0.0_f32, f32::max);
        // Measure every supported latency using the actual selected font and DPI.
        for ms in 0..=999 {
            status_width = status_width.max(value.measure(&format!("{ms} ms")));
        }
        Some(Self {
            value,
            unit,
            scale,
            digit_cell,
            value_right,
            rate_width: value_right + 5.0 * scale + unit_width,
            status_width,
        })
    }
}

/// Every region has fixed geometry regardless of displayed number or unit.
pub fn widget_width(layout: DisplayLayout, show_graph: bool, fonts: &Fonts) -> i32 {
    let rates = if layout == DisplayLayout::SingleLine {
        2.0
    } else {
        1.0
    };
    let graph = if layout == DisplayLayout::Balanced && show_graph {
        56.0
    } else {
        0.0
    };
    (16.0 * fonts.scale
        + rates * (fonts.rate_width + 12.0 * fonts.scale)
        + graph * fonts.scale
        + fonts.status_width)
        .ceil() as i32
}

pub fn effective_layout(preferred: DisplayLayout, height: i32, dpi: u32) -> DisplayLayout {
    if height as f32 / (dpi.max(96) as f32 / 96.0) < 34.0 {
        DisplayLayout::SingleLine
    } else {
        preferred
    }
}

pub struct WidgetView<'a> {
    pub down: &'a RateText,
    pub up: &'a RateText,
    pub status: DisplayState,
    pub failure_intensity: f32,
    pub samples: &'a [NetSample],
    pub layout: DisplayLayout,
    pub show_graph: bool,
    pub graph_fill: bool,
}

pub fn paint(
    canvas: &mut Canvas,
    fonts: &Fonts,
    view: &WidgetView<'_>,
    palette: &Palette,
    graph: &mut TrafficGraph,
) {
    let s = fonts.scale;
    let width = canvas.width() as f32;
    let height = canvas.height() as f32;
    canvas.fill_rounded_rect(Rect::new(0.0, 0.0, width, height), 0.0, palette.surface);
    canvas.stroke_rounded_rect(
        Rect::new(0.5, 0.5, width - 1.0, height - 1.0),
        0.0,
        1.0,
        palette.border,
    );
    let line = fonts.value.line_height().max(fonts.unit.line_height()) as f32;
    let single = view.layout == DisplayLayout::SingleLine;
    let alert = alert_color(view.status, palette, view.failure_intensity);
    let mut text_palette = *palette;
    if let Some(color) = alert {
        text_palette.fg = color;
        text_palette.fg_dim = color;
        text_palette.down = color;
        text_palette.up = color;
    }
    let top = (height - if single { line } else { line * 2.0 + 2.0 * s }) * 0.5;
    draw_rate(
        canvas,
        fonts,
        (8.0 * s, top),
        "↓",
        view.down,
        text_palette.down,
        &text_palette,
    );
    draw_rate(
        canvas,
        fonts,
        (
            if single {
                8.0 * s + fonts.rate_width + 12.0 * s
            } else {
                8.0 * s
            },
            if single { top } else { top + line + 2.0 * s },
        ),
        "↑",
        view.up,
        text_palette.up,
        &text_palette,
    );

    if view.layout == DisplayLayout::Balanced && view.show_graph {
        graph.draw(
            canvas,
            Rect::new(
                8.0 * s + fonts.rate_width + 12.0 * s,
                (height - 26.0 * s) * 0.5,
                44.0 * s,
                26.0 * s,
            ),
            view.samples,
            palette,
            view.graph_fill,
            s,
        );
    }
    let center = width - 8.0 * s - fonts.status_width * 0.5;
    let secondary = match (view.layout, view.status) {
        (DisplayLayout::Balanced, DisplayState::Slow(ms)) => Some(latency_label(ms)),
        _ => None,
    };
    let y = if secondary.is_some() {
        top
    } else {
        (height - line) * 0.5
    };
    let label = view.status.label();
    canvas.draw_text(
        center - fonts.value.measure(&label) * 0.5,
        y,
        &label,
        &fonts.value,
        text_palette.fg,
    );
    if let Some(text) = secondary {
        canvas.draw_text(
            center - fonts.value.measure(&text) * 0.5,
            top + line + 2.0 * s,
            &text,
            &fonts.value,
            text_palette.fg,
        );
    }
}

fn draw_rate(
    canvas: &mut Canvas,
    fonts: &Fonts,
    (x, y): (f32, f32),
    arrow: &str,
    rate: &RateText,
    accent: Color,
    palette: &Palette,
) {
    let s = fonts.scale;
    canvas.draw_text(x, y, arrow, &fonts.value, accent);
    // Give every digit its own equal-width cell, even with a proportional font.
    let cell = fonts.digit_cell;
    let right = x + fonts.value_right;
    let mut cursor = right;
    for character in rate.value.chars().rev() {
        let mut buffer = [0; 4];
        let text = character.encode_utf8(&mut buffer);
        let measured = fonts.value.measure(text);
        let advance = if character.is_ascii_digit() {
            cell
        } else {
            measured
        };
        canvas.draw_text(
            cursor - advance + (advance - measured) * 0.5,
            y,
            text,
            &fonts.value,
            palette.fg,
        );
        cursor -= advance;
    }
    let baseline_offset = (fonts.value.line_height() - fonts.unit.line_height()) as f32;
    canvas.draw_text(
        right + 5.0 * s,
        y + baseline_offset,
        rate.unit,
        &fonts.unit,
        palette.fg_dim,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_text_is_uniform_and_status_fits_at_every_supported_scale() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            for layout in [
                DisplayLayout::Compact,
                DisplayLayout::Balanced,
                DisplayLayout::SingleLine,
            ] {
                for height in [dpi as i32 / 2, dpi as i32 / 3] {
                    let fonts =
                        Fonts::for_dpi(dpi, layout == DisplayLayout::SingleLine, height).unwrap();
                    let width = widget_width(layout, false, &fonts);
                    let rate = RateText {
                        value: "<0.01".to_string(),
                        unit: "MB/s",
                    };
                    for status in [
                        DisplayState::Slow(120.0),
                        DisplayState::Check,
                        DisplayState::Failed,
                    ] {
                        for intensity in [0.0, 0.5, 1.0] {
                            let palette = crate::theme::palette_for("Default");
                            let expected = alert_color(status, &palette, intensity).unwrap();
                            let mut canvas = Canvas::new(width, height);
                            let view = WidgetView {
                                down: &rate,
                                up: &rate,
                                status,
                                failure_intensity: intensity,
                                samples: &[],
                                layout,
                                show_graph: false,
                                graph_fill: false,
                            };
                            paint(
                                &mut canvas,
                                &fonts,
                                &view,
                                &palette,
                                &mut TrafficGraph::new(),
                            );
                            assert!(fonts.value.measure(&status.label()) <= fonts.status_width);
                            // Every covered text pixel must use the same alert hue,
                            // including the small units and both direction arrows.
                            for pixel in canvas.pixels().as_chunks::<4>().0 {
                                let alpha = pixel[3] as f32;
                                for (channel, color) in
                                    pixel[..3].iter().zip([expected.r, expected.g, expected.b])
                                {
                                    assert!((*channel as f32 - color * alpha).abs() <= 2.0);
                                }
                            }
                            let status_left =
                                (width as f32 - 8.0 * fonts.scale - fonts.status_width).floor()
                                    as usize;
                            let visible_status =
                                canvas.pixels().as_chunks::<4>().0.iter().enumerate().any(
                                    |(i, pixel)| i % width as usize >= status_left && pixel[3] > 0,
                                );
                            assert!(
                                visible_status,
                                "status must be visible in {layout:?} at {dpi} DPI"
                            );
                            assert_eq!(widget_width(layout, false, &fonts), width);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn layout_scales_and_short_taskbars_use_one_line() {
        assert_eq!(
            effective_layout(DisplayLayout::Balanced, 48, 192),
            DisplayLayout::SingleLine
        );
    }

    #[test]
    fn graph_colors_and_placement_remain_independent_of_alert_state() {
        use crate::config::Placement;
        use crate::win::placement::choose_x;
        use windows::Win32::Foundation::RECT;
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            let height = dpi as i32 / 2;
            let fonts = Fonts::for_dpi(dpi, false, height).unwrap();
            let width = widget_width(DisplayLayout::Balanced, true, &fonts);
            let palette = crate::theme::palette_for("Default");
            let rate = RateText {
                value: "999+".to_string(),
                unit: "Mbps",
            };
            let samples = [
                NetSample {
                    ts: 100.0,
                    down_mbps: 10.0,
                    up_mbps: 3.0,
                },
                NetSample {
                    ts: 101.0,
                    down_mbps: 5.0,
                    up_mbps: 7.0,
                },
            ];
            let mut canvases = Vec::new();
            for status in [
                DisplayState::Healthy(7.0),
                DisplayState::Slow(120.0),
                DisplayState::Failed,
            ] {
                let mut canvas = Canvas::new(width, height);
                let view = WidgetView {
                    down: &rate,
                    up: &rate,
                    status,
                    failure_intensity: 0.0,
                    samples: &samples,
                    layout: DisplayLayout::Balanced,
                    show_graph: true,
                    graph_fill: true,
                };
                paint(
                    &mut canvas,
                    &fonts,
                    &view,
                    &palette,
                    &mut TrafficGraph::new(),
                );
                canvases.push(canvas);
            }
            let left = (8.0 * fonts.scale + fonts.rate_width + 12.0 * fonts.scale).floor() as usize;
            let right = (left as f32 + 44.0 * fonts.scale).ceil() as usize;
            for (i, pixel) in canvases[0].pixels().as_chunks::<4>().0.iter().enumerate() {
                if (left..right).contains(&(i % width as usize)) {
                    for alert in &canvases[1..] {
                        assert_eq!(pixel, &alert.pixels()[i * 4..i * 4 + 4]);
                    }
                }
            }
            // The measured width is the same width placement must reserve.
            let bar = RECT {
                left: -width,
                top: 0,
                right: 0,
                bottom: height,
            };
            assert_eq!(
                choose_x(bar, &[], width, -width, None, 0, Placement::Left),
                Some(-width)
            );
            assert_eq!(
                choose_x(bar, &[], width + 1, -width, None, 0, Placement::Left),
                None
            );
        }
    }

    #[test]
    fn transparent_default_stays_transparent() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            for layout in [
                DisplayLayout::Compact,
                DisplayLayout::Balanced,
                DisplayLayout::SingleLine,
            ] {
                let height = dpi as i32 / 2;
                let fonts =
                    Fonts::for_dpi(dpi, layout == DisplayLayout::SingleLine, height).unwrap();
                let mut canvas = Canvas::new(widget_width(layout, true, &fonts), height);
                let rate = RateText {
                    value: "999".to_string(),
                    unit: "Mbps",
                };
                let view = WidgetView {
                    down: &rate,
                    up: &rate,
                    status: DisplayState::Healthy(8.0),
                    failure_intensity: 1.0,
                    samples: &[],
                    layout,
                    show_graph: true,
                    graph_fill: false,
                };
                paint(
                    &mut canvas,
                    &fonts,
                    &view,
                    &crate::theme::palette_for("Default"),
                    &mut TrafficGraph::new(),
                );
                assert_eq!(canvas.pixels()[3], 0);
                assert!(canvas
                    .pixels()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0));
            }
        }
    }
}
