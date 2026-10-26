//! Cached DirectWrite grayscale labels for the software canvas.
//!
//! All coordinates and font sizes are physical pixels. Layout uses an identity
//! transform and one pixel per DIP: callers scale logical font sizes exactly once.
//! Masks are rasterized at integer layout origins, never resized. Factory2's
//! explicit GRAYSCALE mode produces ALIASED_1x1 textures with antialiased coverage
//! (the texture name describes storage, not the selected antialiasing mode).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Once;

use windows::core::{implement, Error, IUnknown, Result, PCWSTR};
use windows::Win32::Foundation::{BOOL, E_NOTIMPL, RECT};
use windows::Win32::Graphics::DirectWrite::*;

use crate::canvas::{CoverageMask, TextRenderer};
use crate::win::gdi::{to_wide, Font};

const CACHE_LIMIT: usize = 64;
static FAILURE_LOG: Once = Once::new();

#[derive(Clone)]
struct Entry {
    advance: f32,
    mask: CoverageMask,
}

struct DirectFont {
    factory: IDWriteFactory2,
    format: IDWriteTextFormat,
}

impl DirectFont {
    fn new(family: &str, size: f32, weight: i32) -> Result<Self> {
        let family = to_wide(family);
        let locale = to_wide("en-us");
        // SAFETY: DirectWrite returns owned COM interfaces; string buffers remain
        // alive for these synchronous calls. No COM apartment is required by DWrite.
        unsafe {
            let factory: IDWriteFactory2 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = factory.CreateTextFormat(
                PCWSTR(family.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT(weight),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                PCWSTR(locale.as_ptr()),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(Self { factory, format })
        }
    }

    fn layout(&self, text: &str) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: The API copies the supplied text and retains the format.
        unsafe {
            self.factory
                .CreateTextLayout(&wide, &self.format, 16384.0, 4096.0)
        }
    }

    fn line_metrics(&self) -> Result<(f32, f32)> {
        let layout = self.layout("Ag")?;
        let mut lines = [DWRITE_LINE_METRICS::default()];
        let mut count = 0;
        // SAFETY: One unwrapped line and writable output storage.
        unsafe {
            layout.GetLineMetrics(Some(&mut lines), &mut count)?;
        }
        Ok((lines[0].height, lines[0].baseline))
    }

    fn render(&self, text: &str) -> Result<Entry> {
        self.render_at(text, 0.0, 0.0)
    }

    fn render_at(&self, text: &str, x: f32, y: f32) -> Result<Entry> {
        let layout = self.layout(text)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        let runs = Rc::new(RefCell::new(Vec::new()));
        let renderer: IDWriteTextRenderer = RunCollector {
            factory: self.factory.clone(),
            runs: Rc::clone(&runs),
        }
        .into();
        // SAFETY: Layout invokes the renderer synchronously. Each callback keeps
        // only an owned analysis interface and bounds, not borrowed glyph pointers.
        unsafe {
            layout.GetMetrics(&mut metrics)?;
            layout.Draw(None, &renderer, x, y)?;
        }
        let runs = runs.borrow();
        let bounds = runs
            .iter()
            .map(|(_, rect)| *rect)
            .reduce(|b, r| RECT {
                left: b.left.min(r.left),
                top: b.top.min(r.top),
                right: b.right.max(r.right),
                bottom: b.bottom.max(r.bottom),
            })
            .unwrap_or_default();
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let len = (width as usize)
            .checked_mul(height as usize)
            .filter(|&len| len <= 16 * 1024 * 1024)
            .ok_or_else(|| Error::from_hresult(windows::Win32::Foundation::E_OUTOFMEMORY))?;
        let mut pixels = vec![0u8; len];
        for (analysis, rect) in runs.iter() {
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            let mut coverage = vec![0u8; (w * h) as usize];
            // SAFETY: Bounds came from this analysis and the buffer has exactly
            // one byte per pixel for the explicitly grayscale analysis.
            unsafe {
                analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, rect, &mut coverage)?;
            }
            for y in 0..h {
                for x in 0..w {
                    let dest = ((y + rect.top - bounds.top) * width + x + rect.left - bounds.left)
                        as usize;
                    let alpha = coverage[(y * w + x) as usize] as u32;
                    // Source-over coverage handles overlapping shaped/fallback runs.
                    pixels[dest] =
                        (alpha + (pixels[dest] as u32 * (255 - alpha) + 127) / 255) as u8;
                }
            }
        }
        Ok(Entry {
            advance: metrics.widthIncludingTrailingWhitespace,
            mask: CoverageMask {
                pixels: pixels.into(),
                width,
                height,
                offset_x: bounds.left,
                offset_y: bounds.top,
            },
        })
    }
}

/// An immutable face/size/weight/rendering-scale identity with a 64-entry LRU.
/// Color is not part of the cache: the canvas tints shared grayscale bytes.
pub struct LabelFont {
    direct: Option<DirectFont>,
    fallback: Font,
    height: f32,
    baseline: f32,
    cache: RefCell<VecDeque<(String, Entry)>>,
}

fn log_failure(error: &Error) {
    FAILURE_LOG.call_once(|| {
        crate::logger::warning(
            "text",
            &format!("DirectWrite unavailable; using GDI: {error}"),
        )
    });
}

impl LabelFont {
    /// Uses DirectWrite when available, otherwise a measurement-matched GDI face.
    pub fn new(family: &str, size_px: f32, weight: i32) -> Option<Self> {
        let fallback = Font::new(family, size_px.round() as i32, weight)?;
        let result = DirectFont::new(family, size_px, weight)
            .and_then(|font| font.line_metrics().map(|metrics| (font, metrics)));
        let (direct, height, baseline) = match result {
            Ok((font, (height, baseline))) => (Some(font), height, baseline),
            Err(error) => {
                log_failure(&error);
                (None, fallback.line_height() as f32, fallback.baseline())
            }
        };
        Some(Self {
            direct,
            fallback,
            height,
            baseline,
            cache: RefCell::new(VecDeque::new()),
        })
    }

    /// Physical line height, retaining fractional metrics until layout alignment.
    pub fn line_height(&self) -> f32 {
        self.height
    }

    /// Baseline below the physical layout origin.
    pub fn baseline(&self) -> f32 {
        self.baseline
    }

    fn entry(&self, text: &str) -> Entry {
        let mut cache = self.cache.borrow_mut();
        if let Some(index) = cache.iter().position(|(key, _)| key == text) {
            let hit = cache.remove(index).expect("cache index was just found");
            let entry = hit.1.clone();
            cache.push_back(hit);
            return entry;
        }
        let entry = match self.direct.as_ref().map(|font| font.render(text)) {
            Some(Ok(entry)) => entry,
            result => {
                if let Some(Err(error)) = result {
                    log_failure(&error);
                }
                // Both measurement and pixels fall back together for this string.
                Entry {
                    advance: self.fallback.measure(text),
                    mask: self.fallback.mask(text),
                }
            }
        };
        if cache.len() == CACHE_LIMIT {
            cache.pop_front();
        }
        cache.push_back((text.into(), entry.clone()));
        entry
    }

    /// Fits a single line with an ellipsis; complete rates remain in the native body.
    pub fn fit(&self, text: &str, width: f32) -> String {
        let fits = |text: &str| {
            let entry = self.entry(text);
            entry
                .advance
                .max((entry.mask.offset_x + entry.mask.width) as f32)
                <= width
        };
        if fits(text) {
            return text.into();
        }
        let mut value = text.to_owned();
        while value.pop().is_some() {
            let candidate = format!("{value}…");
            if fits(&candidate) {
                return candidate;
            }
        }
        String::new()
    }
}

impl TextRenderer for LabelFont {
    fn measure(&self, text: &str) -> f32 {
        self.entry(text).advance
    }
    fn mask(&self, text: &str) -> CoverageMask {
        self.entry(text).mask
    }
}

type Runs = Rc<RefCell<Vec<(IDWriteGlyphRunAnalysis, RECT)>>>;

#[implement(IDWriteTextRenderer)]
struct RunCollector {
    factory: IDWriteFactory2,
    runs: Runs,
}

impl IDWritePixelSnapping_Impl for RunCollector_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> Result<BOOL> {
        Ok(true.into())
    }
    fn GetCurrentTransform(&self, _: *const c_void, transform: *mut DWRITE_MATRIX) -> Result<()> {
        // SAFETY: DirectWrite supplies a writable output pointer during Draw.
        unsafe {
            *transform = DWRITE_MATRIX {
                m11: 1.0,
                m22: 1.0,
                ..Default::default()
            };
        }
        Ok(())
    }
    fn GetPixelsPerDip(&self, _: *const c_void) -> Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for RunCollector_Impl {
    fn DrawGlyphRun(
        &self,
        _: *const c_void,
        x: f32,
        y: f32,
        measuring: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        _: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _: Option<&IUnknown>,
    ) -> Result<()> {
        // SAFETY: The layout owns run data through this callback, including font
        // fallback, RTL direction and glyph offsets. Analysis copies what it needs.
        unsafe {
            let analysis = self.factory.CreateGlyphRunAnalysis(
                run,
                None,
                DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                measuring,
                DWRITE_GRID_FIT_MODE_ENABLED,
                DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                x,
                y,
            )?;
            let bounds = analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1)?;
            if bounds.right > bounds.left && bounds.bottom > bounds.top {
                self.runs.borrow_mut().push((analysis, bounds));
            }
        }
        Ok(())
    }
    // Labels never request decorations or inline objects. Reject these rather
    // than silently dropping requested content; the font wrapper then uses GDI.
    fn DrawUnderline(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_UNDERLINE,
        _: Option<&IUnknown>,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn DrawStrikethrough(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: Option<&IUnknown>,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn DrawInlineObject(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: Option<&IDWriteInlineObject>,
        _: BOOL,
        _: BOOL,
        _: Option<&IUnknown>,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const STRINGS: &[&str] = &[
        "1.28 MB/s",
        "112 kB/s",
        "Traffic · last 60 seconds",
        "Shared auto scale: 75.4 Mbps",
        "<0.01",
        "999+",
        "—",
        "↑ ↓",
        "Agj()",
        "שלום",
        "中文",
    ];

    #[test]
    fn directwrite_shapes_grayscale_and_shares_bounded_cache() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            let font = LabelFont::new("Segoe UI", 13.0 * dpi as f32 / 96.0, 400).unwrap();
            assert!(font.direct.is_some());
            assert!(font.baseline() > 0.0 && font.baseline() < font.line_height());
            for text in STRINGS {
                // Call the backend directly: this test must not pass through fallback.
                let direct = font.direct.as_ref().unwrap().render(text).unwrap();
                let mask = font.mask(text);
                assert!(mask.pixels.iter().any(|&v| v > 0), "{text} / {dpi}");
                if *text == "1.28 MB/s" {
                    assert!(
                        mask.pixels.iter().any(|&v| v > 0 && v < 255),
                        "{text} / {dpi}"
                    );
                }
                assert_eq!(mask.pixels.len(), (mask.width * mask.height) as usize);
                assert_eq!(mask.pixels, direct.mask.pixels);
                assert!(Rc::ptr_eq(&mask.pixels, &font.mask(text).pixels));
                assert!(font.measure(text) > 0.0);
                assert!(font.measure(&font.fit(text, 50.0)) <= 50.0);
            }
            let stable = font.mask("Traffic · last 60 seconds");
            for i in 0..200 {
                font.mask(&format!("{i} Mbps"));
                assert!(Rc::ptr_eq(
                    &stable.pixels,
                    &font.mask("Traffic · last 60 seconds").pixels
                ));
                assert!(font.cache.borrow().len() <= CACHE_LIMIT);
            }
            let mut fallback = LabelFont::new("Segoe UI", 13.0, 400).unwrap();
            fallback.direct = None;
            fallback.cache.borrow_mut().clear();
            assert_eq!(
                fallback.measure("1.28 MB/s"),
                fallback.fallback.measure("1.28 MB/s")
            );
            assert!(fallback.mask("1.28 MB/s").pixels.iter().any(|&v| v > 0));
        }
    }

    #[test]
    fn dpi_metrics_and_masks_are_rebuilt() {
        let small = LabelFont::new("Segoe UI", 13.0, 400).unwrap();
        let large = LabelFont::new("Segoe UI", 26.0, 400).unwrap();
        assert!((large.line_height() / small.line_height() - 2.0).abs() < 0.01);
        assert!((large.measure("1.28") / small.measure("1.28") - 2.0).abs() < 0.01);
        assert!(large.mask("1.28").height > small.mask("1.28").height);
    }

    #[test]
    fn run_bounds_preserve_origins_and_all_visible_pixels() {
        let font = DirectFont::new("Segoe UI", 16.0, 600).unwrap();
        for text in STRINGS {
            let origin = font.render(text).unwrap().mask;
            let shifted = font.render_at(text, -31.0, 27.0).unwrap().mask;
            assert_eq!(
                origin.pixels, shifted.pixels,
                "integer translation must not clip {text}"
            );
            assert_eq!(origin.offset_x - 31, shifted.offset_x);
            assert_eq!(origin.offset_y + 27, shifted.offset_y);
            // Analysis bounds are the actual ink bounds, not an advance-sized box.
            assert!(origin
                .pixels
                .chunks(origin.width as usize)
                .any(|row| row[0] > 0));
            assert!(origin
                .pixels
                .chunks(origin.width as usize)
                .any(|row| row[row.len() - 1] > 0));
        }
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetGuiResources(process: windows::Win32::Foundation::HANDLE, flags: u32) -> u32;
    }

    #[test]
    fn repeated_fonts_release_handles_and_cached_storage() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        // Warm process-wide DirectWrite/font fallback resources first.
        let warm = LabelFont::new("Segoe UI", 16.0, 600).unwrap();
        for text in STRINGS {
            warm.mask(text);
        }
        drop(warm);
        let process = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
        let before = unsafe { GetGuiResources(process, 0) };
        for _ in 0..200 {
            let font = LabelFont::new("Segoe UI", 16.0, 600).unwrap();
            let mask = font.mask("↓ 1.28 MB/s    ↑ 112 kB/s");
            let weak = Rc::downgrade(&mask.pixels);
            drop(mask);
            drop(font);
            assert!(
                weak.upgrade().is_none(),
                "closed fonts must release cache bytes"
            );
        }
        let after = unsafe { GetGuiResources(process, 0) };
        eprintln!("DirectWrite font lifecycle: GDI handles {before} -> {after}");
        assert!(after <= before + 4, "GDI handles {before} -> {after}");
    }

    #[test]
    #[ignore = "release text-path microbenchmark"]
    fn text_rendering_benchmark() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        for dpi in [96, 120, 144, 192] {
            for direct in [false, true] {
                let start = Instant::now();
                let font: Box<dyn TextRenderer> = if direct {
                    Box::new(LabelFont::new("Segoe UI", 16.0 * dpi as f32 / 96.0, 600).unwrap())
                } else {
                    Box::new(Font::new("Segoe UI", 14 * dpi / 96, 600).unwrap())
                };
                let init = start.elapsed();
                let start = Instant::now();
                std::hint::black_box(font.mask("↓ 1.28 MB/s    ↑ 112 kB/s"));
                let first = start.elapsed();
                let start = Instant::now();
                for _ in 0..10000 {
                    std::hint::black_box(font.mask("↓ 1.28 MB/s    ↑ 112 kB/s"));
                }
                let warm = start.elapsed() / 10000;
                let start = Instant::now();
                for i in 0..200 {
                    std::hint::black_box(font.mask(&format!("↓ {i}.28 MB/s    ↑ 112 kB/s")));
                }
                eprintln!("{dpi} DPI direct={direct}: init {init:?}, first mask {first:?}, cache hit {warm:?}, changing mask {:?}", start.elapsed() / 200);
            }
        }
    }
}
