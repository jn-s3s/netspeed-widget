//! DPI-scaled, stable-column taskbar readouts. Historical data belongs in details.

use crate::canvas::{Canvas, Color, Rect, TextRenderer};
use crate::config::DisplayLayout;
use crate::format::RateText;
use crate::graph::TrafficGraph;
use crate::sampler::NetSample;
use crate::theme::{status_color, Health, Palette};
use crate::win::gdi::Font;

pub struct Fonts {
    pub value: Font,
    pub unit: Font,
    pub scale: f32,
    digit_cell: f32,
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
        Some(Self {
            value,
            unit: Font::new("Segoe UI", (11.0 * scale).round() as i32, 400)?,
            scale,
            digit_cell,
        })
    }
}

/// Every region has fixed geometry regardless of displayed number or unit.
pub fn widget_width(layout: DisplayLayout, show_graph: bool, dpi: u32) -> i32 {
    let logical = match layout {
        DisplayLayout::Compact => 128,
        DisplayLayout::Balanced => {
            if show_graph {
                224
            } else {
                168
            }
        }
        DisplayLayout::SingleLine => 288,
    };
    (logical as f32 * dpi.max(96) as f32 / 96.0).round() as i32
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
    pub ping_text: &'a str,
    pub health: Health,
    pub ping_pending: bool,
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
    let top = (height - if single { line } else { line * 2.0 + 2.0 * s }) * 0.5;
    draw_rate(
        canvas,
        fonts,
        (8.0 * s, top),
        "↓",
        view.down,
        palette.down,
        palette,
    );
    draw_rate(
        canvas,
        fonts,
        (
            if single { 126.0 * s } else { 8.0 * s },
            if single { top } else { top + line + 2.0 * s },
        ),
        "↑",
        view.up,
        palette.up,
        palette,
    );

    if view.layout == DisplayLayout::Balanced && view.show_graph {
        graph.draw(
            canvas,
            Rect::new(128.0 * s, (height - 26.0 * s) * 0.5, 44.0 * s, 26.0 * s),
            view.samples,
            palette,
            view.graph_fill,
            s,
        );
    }
    if view.layout != DisplayLayout::Compact {
        let right = width - 8.0 * s;
        let y = (height - fonts.unit.line_height() as f32) * 0.5;
        let color = if view.ping_pending {
            palette.fg_dim
        } else {
            status_color(view.health, palette)
        };
        // Shape and text distinguish failures without depending on color.
        let marker = if view.ping_pending {
            "·"
        } else {
            match view.health {
                Health::Good => "·",
                Health::Warn => "!",
                Health::Bad => "×",
            }
        };
        draw_right(canvas, right, y, view.ping_text, &fonts.unit, palette.fg);
        canvas.draw_text(right - 38.0 * s, y, marker, &fonts.unit, color);
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
    let right = x + 64.0 * s;
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

fn draw_right(canvas: &mut Canvas, right: f32, y: f32, text: &str, font: &Font, color: Color) {
    canvas.draw_text(right - font.measure(text), y, text, font, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_scales_and_short_taskbars_use_one_line() {
        assert_eq!(widget_width(DisplayLayout::Balanced, true, 96), 224);
        assert_eq!(widget_width(DisplayLayout::Compact, true, 192), 256);
        assert_eq!(
            effective_layout(DisplayLayout::Balanced, 48, 192),
            DisplayLayout::SingleLine
        );
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
                let mut canvas = Canvas::new(widget_width(layout, true, dpi), height);
                let rate = RateText {
                    value: "999".to_string(),
                    unit: "Mbps",
                };
                let view = WidgetView {
                    down: &rate,
                    up: &rate,
                    ping_text: "8 ms",
                    health: Health::Good,
                    ping_pending: false,
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
