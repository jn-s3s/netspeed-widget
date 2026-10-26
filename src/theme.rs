//! Color palettes, Windows appearance overrides and probe alert colors.
//!
//! The widget and details graph share palette rules. Native menus use Windows
//! styling. High-contrast settings override the selected preset.

use std::sync::LazyLock;

use crate::canvas::Color;

/// Latency health bands; direction accents remain independent of health.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Good,
    Warn,
    Bad,
}

/// The colours used by the widget; no layout or font settings.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub surface: Color,
    pub border: Color,
    pub border_hover: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub down: Color,
    pub up: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub bad_quiet: Color,
}

/// Name of the preset used when nothing else is stored.
pub const DEFAULT_THEME: &str = "Default";
const LIGHT_DOWN: &str = "#006B8F";
const LIGHT_UP: &str = "#6B45AD";

/// Text and graph over the taskbar itself, with no card or hover border.
fn default_palette() -> Palette {
    let mut value = palette(
        "#101418", "#2a3038", "#3d4757", "#eef2f7", "#a9b4c0", "#4fc3e0", "#b39aff", false,
    );
    value.surface = Color::rgba(0, 0, 0, 0.0);
    value.border = Color::rgba(0, 0, 0, 0.0);
    value.border_hover = Color::rgba(0, 0, 0, 0.0);
    value
}

/// Builds a palette with health colors chosen for a light or dark background.
#[expect(
    clippy::too_many_arguments,
    reason = "palette presets specify each named color and the light-mode flag"
)]
fn palette(
    surface: &str,
    border: &str,
    hover: &str,
    fg: &str,
    dim: &str,
    down: &str,
    up: &str,
    light: bool,
) -> Palette {
    Palette {
        surface: Color::from_hex(surface),
        border: Color::from_hex(border),
        border_hover: Color::from_hex(hover),
        fg: Color::from_hex(fg),
        fg_dim: Color::from_hex(dim),
        down: Color::from_hex(down),
        up: Color::from_hex(up),
        good: Color::from_hex(if light { "#16804a" } else { "#43c77a" }),
        warn: Color::from_hex(if light { "#9b6000" } else { "#f0b94f" }),
        bad: Color::from_hex(if light { "#bd3438" } else { "#ff6872" }),
        // On dark taskbars, soften saturation rather than dimming small text.
        // The old endpoints differed by only ~13%, which looked steady at a glance.
        bad_quiet: Color::from_hex(if light { "#702027" } else { "#ffb3bb" }),
    }
}

/// Every built-in preset, in the order the menu lists them.
pub static THEMES: LazyLock<Vec<(&'static str, Palette)>> = LazyLock::new(|| {
    vec![
        (DEFAULT_THEME, default_palette()),
        (
            "Dark",
            palette(
                "#161b22", "#2c333d", "#3d4757", "#e6edf3", "#8b949e", "#40c4d8", "#a58bff", false,
            ),
        ),
        (
            "Ocean",
            palette(
                "#0b2431", "#245064", "#39768d", "#e1f5fa", "#9abac6", "#43c7d6", "#a894ff", false,
            ),
        ),
        (
            "Light",
            palette(
                "#f4f7fb", "#b9c8d7", "#879eb4", "#172b40", "#526779", "#087f91", "#7453bd", true,
            ),
        ),
        (
            "Rose",
            palette(
                "#2d1520", "#563044", "#80506a", "#fce4ec", "#c99aad", "#76c9e8", "#b69cff", false,
            ),
        ),
        (
            "Sakura",
            palette(
                "#fff0f5", "#e8b4c8", "#d491a8", "#4a1a2e", "#765666", "#087f91", "#6554bf", true,
            ),
        ),
        (
            "Lavender",
            palette(
                "#1a1428", "#42345d", "#655286", "#eee7fa", "#b3a4ca", "#62c9dd", "#83b6ff", false,
            ),
        ),
        (
            "Midnight",
            palette(
                "#0a0e1a", "#27314d", "#465579", "#e0e6fa", "#a0accb", "#36c6d3", "#8c9dff", false,
            ),
        ),
        (
            "Forest",
            palette(
                "#101b16", "#304d3c", "#50755b", "#e2f0e5", "#a2bca8", "#53c6d7", "#a9a0ff", false,
            ),
        ),
        (
            "Cyber",
            palette(
                "#0b0c16", "#34314f", "#5d547e", "#f1eaff", "#b3a6cf", "#00d8c0", "#aa8bff", false,
            ),
        ),
        (
            "Candy",
            palette(
                "#fff5f9", "#e7bed0", "#d796b4", "#542038", "#795a6a", "#087f91", "#6550cf", true,
            ),
        ),
        (
            "Sunset",
            palette(
                "#21120f", "#59352b", "#815243", "#ffead7", "#c9a08e", "#42c8d0", "#89aaff", false,
            ),
        ),
        (
            "Crimson",
            palette(
                "#1d1015", "#50303b", "#754858", "#f8e5eb", "#c0a0aa", "#45c8d1", "#a99aff", false,
            ),
        ),
        (
            "Monochrome",
            palette(
                "#171717", "#3b3b3b", "#626262", "#eeeeee", "#b5b5b5", "#62c7d1", "#bd9cff", false,
            ),
        ),
        (
            "Aurora",
            palette(
                "#0a1420", "#254353", "#3d6877", "#e1f4ef", "#9ebeb4", "#40d5c0", "#c19aff", false,
            ),
        ),
        (
            "Peach",
            palette(
                "#fff3e8", "#e4c2a7", "#c79675", "#50341f", "#765840", "#087f91", "#526fc2", true,
            ),
        ),
        (
            "Galaxy",
            palette(
                "#100b20", "#382755", "#59417d", "#e8defb", "#b1a0cc", "#40c6db", "#b08aff", false,
            ),
        ),
        (
            "Mint",
            palette(
                "#f0faf5", "#afd2c1", "#7faf99", "#1a3a2a", "#527765", "#087f94", "#6854bd", true,
            ),
        ),
        (
            "Honey",
            palette(
                "#211a0d", "#554321", "#80622e", "#fff0c0", "#c6ad73", "#45c7d3", "#aaa0ff", false,
            ),
        ),
        (
            "Coral Reef",
            palette(
                "#0a1820", "#28505b", "#427682", "#e0f5f1", "#9bbdb5", "#5bc8d2", "#83b9ff", false,
            ),
        ),
        (
            "Denim",
            palette(
                "#141e2a", "#354b60", "#526f89", "#e2ebf4", "#a5b8c9", "#42c7d5", "#b39aff", false,
            ),
        ),
    ]
});

/// Every preset name, in menu order.
pub fn theme_names() -> Vec<&'static str> {
    THEMES.iter().map(|(name, _)| *name).collect()
}

/// Returns a preset palette, falling back to Default for unknown names.
pub fn palette_for(name: &str) -> Palette {
    THEMES
        .iter()
        .find(|(preset, _)| *preset == name)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| palette_for(DEFAULT_THEME))
}

/// Default follows the taskbar, while high contrast overrides every preset.
pub fn resolved_palette(
    name: &str,
    appearance: crate::win::appearance::Appearance,
    background: bool,
) -> Palette {
    use crate::win::appearance::system_color;
    let mut value = palette_for(name);
    if name == DEFAULT_THEME && appearance.light {
        value = palette(
            "#F4F4F4", "#D0D0D0", "#C0C0C0", "#202124", "#515965", LIGHT_DOWN, LIGHT_UP, true,
        );
        value.surface = Color::rgba(0, 0, 0, 0.0);
        value.border = Color::rgba(0, 0, 0, 0.0);
        value.border_hover = value.border;
    }
    if name == DEFAULT_THEME && background {
        value.surface = if appearance.light {
            Color::rgba(255, 255, 255, 0.55)
        } else {
            Color::rgba(16, 20, 24, 0.55)
        };
    }
    if appearance.high_contrast {
        value.surface = system_color(appearance.background);
        value.fg = system_color(appearance.foreground);
        value.fg_dim = value.fg;
        value.down = value.fg;
        value.up = value.fg;
        value.good = value.fg;
        value.warn = value.fg;
        value.bad = value.fg;
        value.bad_quiet = value.fg;
        value.border = value.fg;
        value.border_hover = value.fg;
    }
    value
}

/// Native details controls use COLOR_WINDOW rather than the taskbar surface.
/// Resolve their graph/text contrast here so panels do not own theme policy.
pub fn native_panel_palette(
    mut value: Palette,
    background: Color,
    foreground: Color,
    high_contrast: bool,
) -> Palette {
    if background.r + background.g + background.b > 1.5 {
        value.down = Color::from_hex(LIGHT_DOWN);
        value.up = Color::from_hex(LIGHT_UP);
    }
    if high_contrast {
        value.down = foreground;
        value.up = foreground;
    }
    value.fg = foreground;
    value.fg_dim = foreground;
    value
}

/// All widget text shares one opaque alert color; graph series stay independent.
pub fn alert_color(
    state: crate::latency::DisplayState,
    palette: &Palette,
    intensity: f32,
) -> Option<Color> {
    use crate::latency::DisplayState;
    match state {
        DisplayState::Slow(_) | DisplayState::Check => Some(palette.warn),
        DisplayState::Failed => {
            let t = intensity.clamp(0.0, 1.0);
            let (quiet, bright) = (palette.bad_quiet, palette.bad);
            Some(Color {
                r: quiet.r + (bright.r - quiet.r) * t,
                g: quiet.g + (bright.g - quiet.g) * t,
                b: quiet.b + (bright.b - quiet.b) * t,
                a: 1.0,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color) -> f32 {
        let linear = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    #[test]
    fn failure_pulse_has_visible_color_separation_without_losing_contrast() {
        use crate::latency::DisplayState;
        for name in theme_names() {
            let palette = palette_for(name);
            let strong = alert_color(DisplayState::Failed, &palette, 1.0).unwrap();
            let soft = alert_color(DisplayState::Failed, &palette, 0.0).unwrap();
            let (a, b) = (luminance(strong), luminance(soft));
            assert!(
                (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 1.6,
                "{name}: pulse endpoints must not look like steady red"
            );
            let background = if palette.surface.a == 0.0 {
                Color::from_hex("#202020")
            } else {
                palette.surface
            };
            let background = luminance(background);
            for step in 0..=20 {
                let text = luminance(
                    alert_color(DisplayState::Failed, &palette, step as f32 / 20.0).unwrap(),
                );
                let contrast = (text.max(background) + 0.05) / (text.min(background) + 0.05);
                assert!(
                    contrast >= 4.5,
                    "{name}: unreadable pulse at step {step}: {contrast}"
                );
            }
        }
    }

    #[test]
    fn pulse_is_opaque_and_overrides_never_reuse_alert_colors() {
        use crate::latency::DisplayState;
        for name in theme_names() {
            let palette = palette_for(name);
            for step in 0..=20 {
                let color =
                    alert_color(DisplayState::Failed, &palette, step as f32 / 20.0).unwrap();
                assert_eq!(color.a, 1.0);
                assert!(color.r >= palette.bad_quiet.r.min(palette.bad.r));
                assert!(color.r <= palette.bad_quiet.r.max(palette.bad.r));
            }
            for state in [
                DisplayState::Waiting,
                DisplayState::Stale,
                DisplayState::Testing,
                DisplayState::Healthy(7.0),
            ] {
                assert!(alert_color(state, &palette, 0.0).is_none());
            }
        }
    }

    #[test]
    fn unknown_theme_falls_back_to_default() {
        assert_eq!(
            format!("{:?}", palette_for("nope").surface),
            format!("{:?}", palette_for(DEFAULT_THEME).surface)
        );
    }

    #[test]
    fn system_appearance_and_health_keep_their_separate_roles() {
        let light = resolved_palette(
            "Default",
            crate::win::appearance::Appearance {
                light: true,
                ..Default::default()
            },
            false,
        );
        assert!(light.fg.r < 0.2);
        assert_eq!(light.surface.a, 0.0);
        let contrast = resolved_palette(
            "Ocean",
            crate::win::appearance::Appearance {
                high_contrast: true,
                foreground: 0x00FFFFFF,
                background: 0,
                ..Default::default()
            },
            false,
        );
        assert_eq!(contrast.surface.a, 1.0);
        assert_eq!(contrast.fg.r, 1.0);
        assert_eq!(contrast.down.r, contrast.up.r);
    }

    #[test]
    fn every_preset_is_listed_once() {
        let names = theme_names();
        assert_eq!(names.len(), THEMES.len());
        assert!(names.contains(&"Ocean"));
        assert_eq!(names.iter().filter(|&&name| name == "Default").count(), 1);
        assert!(names.contains(&"Dark"));
        assert!(!names.contains(&"System"));
    }

    #[test]
    fn default_is_transparent_and_dark_keeps_its_background() {
        assert_eq!(DEFAULT_THEME, "Default");
        let default = palette_for(DEFAULT_THEME);
        assert_eq!(default.surface.a, 0.0);
        assert_eq!(default.border.a, 0.0);
        assert_eq!(default.border_hover.a, 0.0);
        assert_eq!(
            format!("{:?}", palette_for("Dark").surface),
            format!("{:?}", Color::from_hex("#161b22"))
        );
    }
}
