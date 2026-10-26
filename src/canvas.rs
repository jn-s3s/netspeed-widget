//! A small software rasterizer for the taskbar widget.
//!
//! The layered widget needs per-pixel alpha that ordinary GDI drawing does not
//! preserve. Shapes use analytic antialiasing and text is blended from glyph
//! masks into a premultiplied RGBA buffer with `u8` channels. The GDI helper
//! converts it to premultiplied BGRA for `UpdateLayeredWindow`.
//!
//! Premultiplied channels allow source-over compositing without unpremultiplying
//! the destination for each pixel.

use std::rc::Rc;

/// A straight (non-premultiplied) colour with an alpha channel.
#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Builds an opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// Builds a colour with an explicit alpha in `0..=1`.
    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a,
        }
    }

    /// Parses a `#rrggbb` string, allowing surrounding whitespace and an omitted `#`.
    /// Invalid length or non-ASCII input yields opaque black; invalid channel pairs yield zero.
    pub fn from_hex(hex: &str) -> Self {
        let hex = hex.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.is_ascii() {
            return Color::rgb(0, 0, 0);
        }
        let parse =
            |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).unwrap_or(0);
        Color::rgb(parse(0..2), parse(2..4), parse(4..6))
    }

    /// Returns the same colour with a different alpha.
    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }
}

/// A cached grayscale coverage mask, including its offset from the layout origin.
#[derive(Clone, Default)]
pub struct CoverageMask {
    pub pixels: Rc<[u8]>,
    pub width: i32,
    pub height: i32,
    pub offset_x: i32,
    pub offset_y: i32,
}

/// Draws text into coverage masks using backend-matched measurements.
pub trait TextRenderer {
    /// Width of the text in pixels.
    fn measure(&self, text: &str) -> f32;

    /// Coverage mask of the text: one byte per pixel, plus its size.
    ///
    /// The mask is the glyph shape flattened into an alpha channel, so the
    /// widget can paint it in any colour.
    fn mask(&self, text: &str) -> CoverageMask;
}

/// An RGBA drawing surface.
pub struct Canvas {
    width: i32,
    height: i32,
    /// Premultiplied RGBA, four bytes per pixel, row major.
    pixels: Vec<u8>,
}

impl Canvas {
    /// Creates a transparent canvas, clamping each dimension to at least one pixel.
    pub fn new(width: i32, height: i32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let len = (width as usize) * (height as usize) * 4;
        Self {
            width,
            height,
            pixels: vec![0; len],
        }
    }

    /// Canvas width in pixels.
    pub fn width(&self) -> i32 {
        self.width
    }

    /// Canvas height in pixels.
    pub fn height(&self) -> i32 {
        self.height
    }

    /// The premultiplied RGBA bytes, ready to be converted for GDI.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Composites one colour over one pixel with the given coverage.
    pub fn blend(&mut self, x: i32, y: i32, color: Color, coverage: f32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height || coverage <= 0.0 {
            return;
        }
        let alpha = (color.a * coverage).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let index = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let inv = 1.0 - alpha;
        let pixels = &mut self.pixels;
        pixels[index] = to_u8(color.r * alpha + pixels[index] as f32 / 255.0 * inv);
        pixels[index + 1] = to_u8(color.g * alpha + pixels[index + 1] as f32 / 255.0 * inv);
        pixels[index + 2] = to_u8(color.b * alpha + pixels[index + 2] as f32 / 255.0 * inv);
        pixels[index + 3] = to_u8(alpha + pixels[index + 3] as f32 / 255.0 * inv);
    }

    /// Fills a rounded rectangle with analytic edge antialiasing.
    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.paint_rounded_rect(rect, radius, 0.0, color);
    }

    /// Strokes the outline of a rounded rectangle.
    pub fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        self.paint_rounded_rect(rect, radius, width, color);
    }

    /// Fills or strokes a rounded rectangle.
    ///
    /// The signed distance to the shape determines edge coverage for both fill and outline.
    fn paint_rounded_rect(&mut self, rect: Rect, radius: f32, stroke: f32, color: Color) {
        if color.a <= 0.0 {
            return;
        }
        let radius = radius.max(0.0).min(rect.width * 0.5).min(rect.height * 0.5);
        let cx = rect.x + rect.width * 0.5;
        let cy = rect.y + rect.height * 0.5;
        let half_w = rect.width * 0.5;
        let half_h = rect.height * 0.5;

        let x0 = rect.x.floor() as i32;
        let y0 = rect.y.floor() as i32;
        let x1 = (rect.x + rect.width).ceil() as i32;
        let y1 = (rect.y + rect.height).ceil() as i32;

        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let dist = sd_rounded_box(px, py, cx, cy, half_w, half_h, radius);
                let coverage = if stroke <= 0.0 {
                    clamp01(0.5 - dist)
                } else {
                    // The band between the outer edge and the inner edge.
                    let outer = clamp01(0.5 - dist);
                    let inner = clamp01(0.5 - (dist + stroke));
                    (outer - inner).clamp(0.0, 1.0)
                };
                self.blend(x, y, color, coverage);
            }
        }
    }

    /// Fills a closed polygon with antialiased edges.
    pub fn fill_polygon(&mut self, points: &[(f32, f32)], color: Color) {
        if points.len() < 3 {
            return;
        }
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for (_, y) in points {
            min_y = min_y.min(*y);
            max_y = max_y.max(*y);
        }
        let y_start = min_y.floor() as i32;
        let y_end = max_y.ceil() as i32;
        let sub_samples = 4;
        let mut row = vec![0.0f32; self.width as usize];
        let mut crossings = Vec::with_capacity(points.len());

        for y in y_start..y_end {
            row.fill(0.0);
            let mut touched = false;
            for sub in 0..sub_samples {
                let sy = y as f32 + (sub as f32 + 0.5) / sub_samples as f32;
                crossings.clear();
                for index in 0..points.len() {
                    let (x0, y0) = points[index];
                    let (x1, y1) = points[(index + 1) % points.len()];
                    if (y0 <= sy) == (y1 <= sy) {
                        continue;
                    }
                    let t = (sy - y0) / (y1 - y0);
                    crossings.push(x0 + t * (x1 - x0));
                }
                crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let mut span = 0;
                while span + 1 < crossings.len() {
                    let (sx0, sx1) = (crossings[span], crossings[span + 1]);
                    span += 2;
                    if sx1 <= 0.0 || sx0 >= self.width as f32 {
                        continue;
                    }
                    touched = true;
                    add_span_coverage(&mut row, sx0, sx1, 1.0 / sub_samples as f32);
                }
            }
            if !touched {
                continue;
            }
            for (x, coverage) in row.iter().enumerate() {
                if *coverage > 0.0 {
                    self.blend(x as i32, y, color, *coverage);
                }
            }
        }
    }

    /// Strokes a polyline with round joins and antialiased edges.
    pub fn stroke_polyline(&mut self, points: &[(f32, f32)], thickness: f32, color: Color) {
        if points.len() < 2 {
            return;
        }
        let half = (thickness * 0.5).max(0.5);
        let margin = half + 1.0;
        let left = (points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) - margin)
            .floor()
            .max(0.0) as i32;
        let top = (points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) - margin)
            .floor()
            .max(0.0) as i32;
        let right = (points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) + margin)
            .ceil()
            .min(self.width as f32) as i32;
        let bottom = (points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) + margin)
            .ceil()
            .min(self.height as f32) as i32;
        if right <= left || bottom <= top {
            return;
        }
        let stride = (right - left) as usize;
        let mut coverage = vec![0.0f32; stride * (bottom - top) as usize];
        for index in 0..points.len() - 1 {
            let a = points[index];
            let b = points[index + 1];
            let (min_x, max_x) = (a.0.min(b.0), a.0.max(b.0));
            let (min_y, max_y) = (a.1.min(b.1), a.1.max(b.1));
            let x0 = (min_x - half - 1.0).floor() as i32;
            let x1 = (max_x + half + 1.0).ceil() as i32;
            let y0 = (min_y - half - 1.0).floor() as i32;
            let y1 = (max_y + half + 1.0).ceil() as i32;
            for y in y0.max(0)..y1.min(self.height) {
                for x in x0.max(0)..x1.min(self.width) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let dist = distance_to_segment(px, py, a, b);
                    let value = clamp01(half + 0.5 - dist);
                    let slot = &mut coverage[(y - top) as usize * stride + (x - left) as usize];
                    if value > *slot {
                        *slot = value;
                    }
                }
            }
        }
        for y in top..bottom {
            for x in left..right {
                let value = coverage[(y - top) as usize * stride + (x - left) as usize];
                if value > 0.0 {
                    self.blend(x, y, color, value);
                }
            }
        }
    }

    /// Draws text with its top-left corner at (`x`, `y`).
    ///
    /// Returns the width of the drawn text in pixels.
    pub fn draw_text<R: TextRenderer>(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        renderer: &R,
        color: Color,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.draw_mask(x, y, &renderer.mask(text), color);
        renderer.measure(text)
    }

    /// Blends a reusable mask without copying its coverage bytes.
    pub fn draw_mask(&mut self, x: f32, y: f32, mask: &CoverageMask, color: Color) {
        let left = x.round() as i32 + mask.offset_x;
        let top = y.round() as i32 + mask.offset_y;
        for my in 0..mask.height {
            for mx in 0..mask.width {
                let coverage = mask.pixels[(my * mask.width + mx) as usize] as f32 / 255.0;
                if coverage > 0.0 {
                    self.blend(left + mx, top + my, color, coverage);
                }
            }
        }
    }
}

/// An axis-aligned rectangle in pixels.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Builds a rectangle from its edges.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// Signed distance to a rounded box centred on (`cx`, `cy`).
fn sd_rounded_box(px: f32, py: f32, cx: f32, cy: f32, half_w: f32, half_h: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (half_w - r);
    let qy = (py - cy).abs() - (half_h - r);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

/// Distance from a point to the closest point on a segment.
fn distance_to_segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (ax, ay) = a;
    let (bx, by) = b;
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq <= f32::EPSILON {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0);
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    (px - cx).hypot(py - cy)
}

/// Adds a horizontal span to a coverage row with analytic edge coverage.
fn add_span_coverage(row: &mut [f32], start: f32, end: f32, weight: f32) {
    let start = start.max(0.0);
    let end = end.min(row.len() as f32);
    if end <= start {
        return;
    }
    let first = start.floor() as usize;
    let last = (end.ceil() as usize).min(row.len());
    for (index, slot) in row.iter_mut().enumerate().take(last).skip(first) {
        let left = index as f32;
        let right = left + 1.0;
        let covered = end.min(right) - start.max(left);
        if covered > 0.0 {
            *slot += covered * weight;
        }
    }
}

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_padding_keeps_the_layout_origin_and_clips_at_canvas_edges() {
        let mut canvas = Canvas::new(2, 2);
        let mask = CoverageMask {
            pixels: vec![0, 0, 0, 0, 0, 255, 128, 0, 0, 64, 32, 0, 0, 0, 0, 0].into(),
            width: 4,
            height: 4,
            offset_x: -1,
            offset_y: -1,
        };
        canvas.draw_mask(0.0, 0.0, &mask, Color::rgb(255, 255, 255));
        assert_eq!(
            canvas
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[3])
                .collect::<Vec<_>>(),
            [255, 128, 64, 32]
        );
    }

    #[test]
    fn connected_stroke_does_not_darken_shared_joins() {
        let points = [(2.5, 3.5), (6.5, 3.5), (10.5, 3.5)];
        let mut canvas = Canvas::new(14, 8);
        canvas.stroke_polyline(&points, 1.0, Color::rgba(255, 255, 255, 0.5));
        let center = |x: usize| canvas.pixels()[(3 * 14 + x) * 4 + 3];
        assert_eq!(center(4), center(6));
        assert_eq!(center(6), 128);
    }

    #[test]
    fn hex_colors_reject_non_ascii_without_panicking() {
        for text in ["€€", "a€bc", "", "12345", "zzzzzz"] {
            let color = Color::from_hex(text);
            assert_eq!((color.r, color.g, color.b, color.a), (0.0, 0.0, 0.0, 1.0));
        }
        let color = Color::from_hex("#ff8000");
        assert_eq!((color.r, color.g, color.b), (1.0, 128.0 / 255.0, 0.0));
    }

    #[test]
    fn bounded_stroke_matches_full_canvas_coverage_at_clipped_edges() {
        for points in [
            vec![(-10.0, 2.0), (12.0, 8.0), (40.0, 20.0)],
            vec![(4.0, 5.0), (9.0, 2.0), (9.0, 14.0)],
            vec![(100.0, 100.0), (110.0, 110.0)],
        ] {
            let mut bounded = Canvas::new(24, 16);
            let mut reference = Canvas::new(24, 16);
            let color = Color::rgba(20, 180, 220, 0.7);
            bounded.stroke_polyline(&points, 1.5, color);
            for y in 0..16 {
                for x in 0..24 {
                    let coverage = points
                        .windows(2)
                        .map(|pair| {
                            clamp01(
                                1.25 - distance_to_segment(
                                    x as f32 + 0.5,
                                    y as f32 + 0.5,
                                    pair[0],
                                    pair[1],
                                ),
                            )
                        })
                        .fold(0.0, f32::max);
                    reference.blend(x, y, color, coverage);
                }
            }
            assert_eq!(bounded.pixels(), reference.pixels());
        }
    }

    struct FakeText;

    impl TextRenderer for FakeText {
        fn measure(&self, _text: &str) -> f32 {
            10.0
        }

        fn mask(&self, _text: &str) -> CoverageMask {
            CoverageMask {
                pixels: vec![255; 4].into(),
                width: 2,
                height: 2,
                offset_x: 0,
                offset_y: 0,
            }
        }
    }

    #[test]
    fn rounded_rect_is_opaque_in_the_middle() {
        let mut canvas = Canvas::new(20, 20);
        canvas.fill_rounded_rect(Rect::new(2.0, 2.0, 16.0, 16.0), 4.0, Color::rgb(255, 0, 0));
        let index = (10 * 20 + 10) * 4;
        assert_eq!(canvas.pixels()[index + 3], 255);
        assert_eq!(canvas.pixels()[index], 255);
    }

    #[test]
    fn rounded_rect_corners_stay_transparent() {
        let mut canvas = Canvas::new(20, 20);
        canvas.fill_rounded_rect(Rect::new(2.0, 2.0, 16.0, 16.0), 6.0, Color::rgb(255, 0, 0));
        let index = 2 * 20 * 4 + 2 * 4;
        assert_eq!(canvas.pixels()[index + 3], 0);
    }

    #[test]
    fn polygon_covers_its_interior() {
        let mut canvas = Canvas::new(10, 10);
        canvas.fill_polygon(
            &[(1.0, 1.0), (8.0, 1.0), (8.0, 8.0), (1.0, 8.0)],
            Color::rgb(0, 255, 0),
        );
        let index = (5 * 10 + 5) * 4;
        assert_eq!(canvas.pixels()[index + 3], 255);
    }

    #[test]
    fn text_blend_uses_the_mask() {
        let mut canvas = Canvas::new(4, 4);
        canvas.draw_text(0.0, 0.0, "ab", &FakeText, Color::rgb(0, 0, 255));
        assert_eq!(canvas.pixels()[3], 255);
        assert_eq!(canvas.pixels()[2], 255);
    }
}
