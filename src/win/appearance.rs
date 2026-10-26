//! System taskbar colors, including the Windows high-contrast override.

use crate::canvas::Color;
use crate::win::gdi::to_wide;
use windows::core::PCWSTR;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::Gdi::{GetSysColor, COLOR_HIGHLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Appearance {
    pub light: bool,
    pub high_contrast: bool,
    pub animations: bool,
    pub foreground: u32,
    pub background: u32,
    pub accent: u32,
}

impl Appearance {
    pub fn current() -> Self {
        let mut light = 0_u32;
        let mut size = std::mem::size_of_val(&light) as u32;
        let key = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let name = to_wide("SystemUsesLightTheme");
        let mut contrast = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        let mut animations = BOOL(0);
        unsafe {
            // A failed preference query conservatively disables animation.
            let _ = SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut animations as *mut BOOL).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            let _ = RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut light as *mut u32).cast()),
                Some(&mut size),
            );
            let _ = SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            Self {
                light: light != 0,
                high_contrast: (contrast.dwFlags & HCF_HIGHCONTRASTON).0 != 0,
                animations: animations.as_bool(),
                foreground: GetSysColor(COLOR_WINDOWTEXT),
                background: GetSysColor(COLOR_WINDOW),
                accent: GetSysColor(COLOR_HIGHLIGHT),
            }
        }
    }
}

pub fn system_color(value: u32) -> Color {
    Color::rgb(value as u8, (value >> 8) as u8, (value >> 16) as u8)
}
