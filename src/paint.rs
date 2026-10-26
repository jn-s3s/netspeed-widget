//! DPI-scaled, stable-column taskbar readouts. Historical data belongs in details.

use crate::canvas::{Canvas, Color, CoverageMask, Rect, TextRenderer};
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
    down_arrow: CoverageMask,
    up_arrow: CoverageMask,
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
        let arrow_width = (7.0 * scale).ceil();
        let value_right = arrow_width + 6.0 * scale + number_width;
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
            down_arrow: arrow_mask(scale, value.line_height(), false),
            up_arrow: arrow_mask(scale, value.line_height(), true),
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

/// Rasterize once per DPI/font change, not once per paint. A single silhouette
/// avoids darker overlap between the arrowhead and shaft on translucent themes.
fn arrow_mask(scale: f32, line_height: i32, up: bool) -> CoverageMask {
    let width = (7.0 * scale).ceil() as i32 + 2;
    let height = line_height + 2;
    let mut canvas = Canvas::new(width, height);
    let stem = scale.round().max(1.0);
    let cx = (width as f32 * 0.5 - stem * 0.5).round() + stem * 0.5;
    let head = (3.5 * scale).min(cx - 1.0).min(width as f32 - 1.0 - cx);
    let top = 1.0 + ((line_height as f32 - 10.0 * scale) * 0.5).round();
    let bottom = top + 10.0 * scale;
    let half = stem * 0.5;
    let mut points = [
        (cx - half, top),
        (cx + half, top),
        (cx + half, bottom - 3.0 * scale),
        (cx + 2.5 * scale, bottom - 5.0 * scale),
        (cx + head, bottom - 4.0 * scale),
        (cx, bottom),
        (cx - head, bottom - 4.0 * scale),
        (cx - 2.5 * scale, bottom - 5.0 * scale),
        (cx - half, bottom - 3.0 * scale),
    ];
    if up {
        for (_, y) in &mut points {
            *y = top + bottom - *y;
        }
    }
    canvas.fill_polygon(&points, Color::rgb(255, 255, 255));
    CoverageMask {
        pixels: canvas
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3])
            .collect(),
        width,
        height,
        offset_x: -1,
        offset_y: -1,
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
    // Clip both horizontal borders so the surface fills the taskbar's usable height.
    canvas.stroke_rounded_rect(
        Rect::new(0.0, -2.0, width, height + 4.0),
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
        &fonts.down_arrow,
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
        &fonts.up_arrow,
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
    arrow: &CoverageMask,
    rate: &RateText,
    accent: Color,
    palette: &Palette,
) {
    let s = fonts.scale;
    canvas.draw_mask(x, y, arrow, accent);
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
    fn vector_arrows_have_smooth_edges_and_fit_their_reserved_column() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            let fonts = Fonts::for_dpi(dpi, false, dpi as i32 / 2).unwrap();
            for mask in [&fonts.down_arrow, &fonts.up_arrow] {
                assert!(mask.pixels.contains(&255));
                assert!(mask.pixels.iter().any(|&p| p > 0 && p < 255));
                assert_eq!(mask.pixels.len(), (mask.width * mask.height) as usize);
                for row in mask.pixels.chunks_exact(mask.width as usize) {
                    assert_eq!(row[0], 0);
                    assert_eq!(row[row.len() - 1], 0);
                }
                assert_eq!(mask.height, fonts.value.line_height() + 2);
            }
        }
    }

    /// Manual microbenchmark: excludes window presentation and network sampling.
    #[test]
    #[ignore = "manual release-build rendering benchmark"]
    fn rendering_benchmark() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            let height = dpi as i32 / 2;
            let fonts = Fonts::for_dpi(dpi, false, height).unwrap();
            let rate = RateText {
                value: "9.99".into(),
                unit: "Mbps",
            };
            let now = crate::now_epoch();
            let samples: Vec<_> = (0..60)
                .map(|i| NetSample {
                    ts: now - (59 - i) as f64,
                    down_mbps: (i % 11) as f64,
                    up_mbps: (i % 7) as f64,
                })
                .collect();
            for active in [false, true] {
                let view = WidgetView {
                    down: &rate,
                    up: &rate,
                    status: DisplayState::Healthy(8.0),
                    failure_intensity: 0.0,
                    samples: if active { &samples } else { &[] },
                    layout: DisplayLayout::Balanced,
                    show_graph: true,
                    graph_fill: true,
                };
                let palette = crate::theme::palette_for("Default");
                let width = widget_width(view.layout, true, &fonts);
                let mut graph = TrafficGraph::new();
                let mut render = || {
                    let mut canvas = Canvas::new(width, height);
                    paint(&mut canvas, &fonts, &view, &palette, &mut graph);
                    std::hint::black_box(canvas);
                };
                render(); // Warm font masks before measuring steady-state work.
                let mut timings = Vec::new();
                for _ in 0..5 {
                    let start = std::time::Instant::now();
                    for _ in 0..500 {
                        render();
                    }
                    timings.push(start.elapsed().as_secs_f64() * 1_000_000.0 / 500.0);
                }
                timings.sort_by(f64::total_cmp);
                println!(
                    "{dpi} DPI, active={active}: {:.2} us/paint (median)",
                    timings[2]
                );
            }
        }
    }

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
    fn background_fills_top_and_bottom_without_horizontal_borders() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            let height = dpi as i32 / 2 - 1;
            let fonts = Fonts::for_dpi(dpi, false, height).unwrap();
            let width = widget_width(DisplayLayout::Balanced, false, &fonts);
            let mut canvas = Canvas::new(width, height);
            let palette = crate::theme::palette_for("Dark");
            let rate = RateText {
                value: "0".to_string(),
                unit: "B/s",
            };
            paint(
                &mut canvas,
                &fonts,
                &WidgetView {
                    down: &rate,
                    up: &rate,
                    status: DisplayState::Healthy(8.0),
                    failure_intensity: 0.0,
                    samples: &[],
                    layout: DisplayLayout::Balanced,
                    show_graph: false,
                    graph_fill: false,
                },
                &palette,
                &mut TrafficGraph::new(),
            );
            let mut expected = Canvas::new(width, 1);
            expected.fill_rounded_rect(
                Rect::new(0.0, 0.0, width as f32, 1.0),
                0.0,
                palette.surface,
            );
            // Exclude the side borders and their antialiased inner edges.
            for row in [0, height - 1] {
                let start = (row * width + 2) as usize * 4;
                let end = ((row + 1) * width - 2) as usize * 4;
                assert_eq!(
                    &canvas.pixels()[start..end],
                    &expected.pixels()[8..(width as usize - 2) * 4],
                    "edge row {row} must contain only the surface at {dpi} DPI"
                );
                for x in [0, width - 1] {
                    let edge = ((row * width + x) * 4) as usize;
                    let mut border = Canvas::new(1, 1);
                    border.blend(0, 0, palette.surface, 1.0);
                    border.blend(0, 0, palette.border, 1.0);
                    assert_eq!(&canvas.pixels()[edge..edge + 4], border.pixels());
                }
            }
        }
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
