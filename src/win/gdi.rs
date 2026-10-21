//! GDI helpers: fonts, glyph masks and the layered-window blit.
//!
//! The widget paints into a software canvas and hands the finished buffer to
//! `UpdateLayeredWindow`, so the only thing GDI is used for is turning text
//! into a coverage mask. The mask is rendered with grayscale antialiasing
//! rather than ClearType, because the widget composites its own colours onto
//! a background it does not control.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::slice;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    GetTextExtentPoint32W, ReleaseDC, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA,
    AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_TOP, HDC, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

use crate::canvas::{Canvas, TextRenderer};

/// `FW_NORMAL`.
pub const WEIGHT_NORMAL: i32 = 400;
/// `FW_BOLD`.
pub const WEIGHT_BOLD: i32 = 700;

/// Paints an opaque canvas in a normal native window (details graph).
pub fn blit_opaque(hdc: HDC, canvas: &Canvas) {
    use windows::Win32::Graphics::Gdi::{SetDIBitsToDevice, DIB_RGB_COLORS};
    let mut pixels = canvas.pixels().to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: canvas.width(),
            biHeight: -canvas.height(),
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetDIBitsToDevice(
            hdc,
            0,
            0,
            canvas.width() as u32,
            canvas.height() as u32,
            0,
            0,
            0,
            canvas.height() as u32,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
        );
    }
}

/// A GDI font kept on its own memory device context.
///
/// The context stays alive for the lifetime of the font so measurements and
/// mask rendering always use the same face.
pub struct Font {
    hfont: windows::Win32::Graphics::Gdi::HFONT,
    hdc: windows::Win32::Graphics::Gdi::HDC,
    height: i32,
    masks: RefCell<HashMap<String, GlyphMask>>,
    advances: RefCell<HashMap<char, f32>>,
}

type GlyphMask = (Vec<u8>, i32, i32);

impl Font {
    pub fn handle(&self) -> windows::Win32::Graphics::Gdi::HFONT {
        self.hfont
    }
    /// The font is retained by its owning panel for the control's lifetime.
    pub fn apply_to(&self, hwnd: HWND) {
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,
                windows::Win32::Foundation::WPARAM(self.hfont.0 as usize),
                windows::Win32::Foundation::LPARAM(1),
            );
        }
    }
    /// Creates a font of the given family, pixel height and weight.
    pub fn new(family: &str, size_px: i32, weight: i32) -> Option<Self> {
        unsafe {
            let hdc = CreateCompatibleDC(HDC::default());
            let wide = to_wide(family);
            let hfont = CreateFontW(
                -size_px,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                ANTIALIASED_QUALITY.0 as u32,
                0,
                PCWSTR(wide.as_ptr()),
            );
            if hfont.is_invalid() {
                let _ = DeleteDC(hdc);
                return None;
            }
            let _ = SelectObject(hdc, HGDIOBJ(hfont.0));
            let height = size_px;
            Some(Self {
                hfont,
                hdc,
                height,
                masks: RefCell::new(HashMap::new()),
                advances: RefCell::new(HashMap::new()),
            })
        }
    }

    /// The line height the font reports for a sample of text.
    pub fn line_height(&self) -> i32 {
        unsafe {
            let wide = to_wide("0");
            let mut size = SIZE::default();
            if GetTextExtentPoint32W(self.hdc, &wide[..wide.len() - 1], &mut size).as_bool() {
                size.cy.max(self.height)
            } else {
                self.height
            }
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.hdc);
            let _ = DeleteObject(HGDIOBJ(self.hfont.0));
        }
    }
}

impl TextRenderer for Font {
    fn measure(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let mut characters = text.chars();
        let character = characters.next().filter(|_| characters.next().is_none());
        if let Some(width) = character.and_then(|key| self.advances.borrow().get(&key).copied()) {
            return width;
        }
        unsafe {
            let wide = to_wide(text);
            let mut size = SIZE::default();
            if GetTextExtentPoint32W(self.hdc, &wide[..wide.len() - 1], &mut size).as_bool() {
                let width = size.cx as f32;
                if let Some(character) = character {
                    let mut advances = self.advances.borrow_mut();
                    if advances.len() < 64 {
                        advances.insert(character, width);
                    }
                }
                width
            } else {
                0.0
            }
        }
    }

    fn mask(&self, text: &str) -> (Vec<u8>, i32, i32) {
        if text.is_empty() {
            return (Vec::new(), 0, 0);
        }
        if let Some(mask) = self.masks.borrow().get(text) {
            return mask.clone();
        }
        unsafe {
            let mut wide = to_wide(text);
            let text_len = wide.len() - 1;
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(self.hdc, &wide[..text_len], &mut size);
            // One pixel of slack on every side keeps antialiased glyph edges
            // from being clipped by the measured box.
            let width = (size.cx + 2).max(1);
            let height = (size.cy + 2).max(1);

            let mem_dc = CreateCompatibleDC(HDC::default());
            // The face has to be selected into this device context too, or
            // the text is drawn with the default system font and clipped to
            // a box that was measured for a different one.
            let _ = SelectObject(mem_dc, HGDIOBJ(self.hfont.0));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib = match CreateDIBSection(
                mem_dc,
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                HANDLE::default(),
                0,
            ) {
                Ok(dib) => dib,
                Err(_) => {
                    let _ = DeleteDC(mem_dc);
                    return (Vec::new(), 0, 0);
                }
            };
            let _ = SelectObject(mem_dc, HGDIOBJ(dib.0));

            // Black background, white glyphs: the brightest channel of each
            // pixel is then the glyph coverage.
            let pixels = slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
            for chunk in pixels.as_chunks_mut::<4>().0 {
                chunk[0] = 0;
                chunk[1] = 0;
                chunk[2] = 0;
                chunk[3] = 255;
            }
            let _ = SetBkMode(mem_dc, TRANSPARENT);
            let _ = SetTextColor(mem_dc, COLORREF(0x00FF_FFFF));
            let mut rect = windows::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            let _ = DrawTextW(
                mem_dc,
                &mut wide[..text_len],
                &mut rect,
                DRAW_TEXT_FORMAT(DT_LEFT.0 | DT_TOP.0 | DT_SINGLELINE.0 | DT_NOPREFIX.0),
            );

            let raw = slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize);
            let mut mask = vec![0u8; (width * height) as usize];
            for index in 0..(width * height) as usize {
                let blue = raw[index * 4] as u32;
                let green = raw[index * 4 + 1] as u32;
                let red = raw[index * 4 + 2] as u32;
                mask[index] = blue.max(green).max(red) as u8;
            }

            let _ = DeleteDC(mem_dc);
            let _ = DeleteObject(HGDIOBJ(dib.0));
            let result = (mask, width, height);
            let mut cache = self.masks.borrow_mut();
            if cache.len() >= 64 {
                cache.clear();
            }
            cache.insert(text.to_string(), result.clone());
            result
        }
    }
}

/// Pushes a canvas onto a layered window.
///
/// The canvas holds premultiplied RGBA; the DIB wants premultiplied BGRA.
pub fn blit_layered(hwnd: HWND, canvas: &Canvas) -> bool {
    unsafe {
        let width = canvas.width();
        let height = canvas.height();
        if width <= 0 || height <= 0 {
            return false;
        }
        let screen_dc = GetDC(HWND::default());
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = match CreateDIBSection(
            mem_dc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        ) {
            Ok(dib) => dib,
            Err(_) => {
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(HWND::default(), screen_dc);
                return false;
            }
        };
        let _ = SelectObject(mem_dc, HGDIOBJ(dib.0));

        let count = (width * height) as usize;
        let destination = slice::from_raw_parts_mut(bits as *mut u8, count * 4);
        copy_interactive_pixels(canvas.pixels(), destination);

        let source_point = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let result = UpdateLayeredWindow(
            hwnd,
            screen_dc,
            None,
            Some(&size),
            mem_dc,
            Some(&source_point),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        let _ = DeleteDC(mem_dc);
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = ReleaseDC(HWND::default(), screen_dc);
        result.is_ok()
    }
}

/// Convert premultiplied RGBA to BGRA while keeping the whole widget clickable.
/// Windows passes mouse input through zero-alpha layered pixels. The minimum
/// nonzero alpha (1/255) is visually transparent but retains the input surface,
/// including the spaces between glyphs in the Default theme.
fn copy_interactive_pixels(source: &[u8], destination: &mut [u8]) {
    assert_eq!(source.len(), destination.len());
    for (rgba, bgra) in source
        .as_chunks::<4>()
        .0
        .iter()
        .zip(destination.as_chunks_mut::<4>().0)
    {
        *bgra = [rgba[2], rgba[1], rgba[0], rgba[3].max(1)];
    }
}

/// Converts a string into a null-terminated UTF-16 buffer.
pub fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_advances_are_cached_per_font() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let font = Font::new("Segoe UI", 12, WEIGHT_NORMAL).unwrap();
        let width = font.measure("0");
        for _ in 0..100 {
            assert_eq!(font.measure("0"), width);
        }
        assert_eq!(font.advances.borrow().len(), 1);
        assert_eq!(font.advances.borrow().get(&'0'), Some(&width));
    }

    #[test]
    fn transparent_background_remains_clickable_without_dimming_content() {
        let source = [0, 0, 0, 0, 10, 20, 30, 128, 40, 50, 60, 255];
        let mut destination = [0; 12];
        copy_interactive_pixels(&source, &mut destination);
        assert_eq!(destination, [0, 0, 0, 1, 30, 20, 10, 128, 60, 50, 40, 255]);
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetGuiResources(process: HANDLE, flags: u32) -> u32;
    }

    #[test]
    fn repeated_text_rendering_releases_gdi_objects() {
        let _ui = crate::win::UI_TEST_LOCK.lock().unwrap();
        let process = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
        let before = unsafe { GetGuiResources(process, 0) };
        for _ in 0..200 {
            let font = Font::new("Segoe UI", 12, WEIGHT_NORMAL).unwrap();
            let (mask, width, height) = font.mask("123.4 Mb/s");
            assert_eq!(mask.len(), (width * height) as usize);
        }
        let after = unsafe { GetGuiResources(process, 0) };
        // Allow a little slack for other tests initializing Windows resources.
        assert!(
            after <= before + 16,
            "GDI objects grew from {before} to {after}"
        );
    }
}
