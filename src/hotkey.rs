//! Parsing and formatting of the global hotkey combo.
//!
//! The combo is stored as a readable string such as `ctrl+shift+n` and
//! translated into the modifier flags and virtual key code that
//! `RegisterHotKey` expects. A combo must carry Ctrl or Alt, because a bare
//! or Shift-only combo would hijack normal typing in other applications.

/// `MOD_ALT`.
pub const MOD_ALT: u32 = 0x0001;
/// `MOD_CONTROL`.
pub const MOD_CONTROL: u32 = 0x0002;
/// `MOD_SHIFT`.
pub const MOD_SHIFT: u32 = 0x0004;
/// `MOD_NOREPEAT`.
pub const MOD_NOREPEAT: u32 = 0x4000;

/// Modifier names in the order they are shown.
const MOD_ORDER: [&str; 3] = ["ctrl", "shift", "alt"];

/// Extra keys the combo language accepts.
fn extra_keys() -> &'static [(&'static str, u32)] {
    &[
        ("space", 0x20),
        ("tab", 0x09),
        ("enter", 0x0D),
        ("backspace", 0x08),
        ("delete", 0x2E),
        ("insert", 0x2D),
        ("home", 0x24),
        ("end", 0x23),
        ("pageup", 0x21),
        ("pagedown", 0x22),
        ("up", 0x26),
        ("down", 0x28),
        ("left", 0x25),
        ("right", 0x27),
    ]
}

/// Virtual key code for a key name, or `None` when the key is unsupported.
pub fn key_to_vk(key: &str) -> Option<u32> {
    let mut chars = key.chars();
    if let (Some(only), true) = (chars.next(), chars.next().is_none()) {
        if only.is_ascii_alphanumeric() {
            return Some((only.to_ascii_uppercase() as u8) as u32);
        }
    }
    if let Some(number) = key.strip_prefix('f') {
        if let Ok(index) = number.parse::<u32>() {
            if (1..=12).contains(&index) {
                return Some(0x70 + index - 1);
            }
        }
    }
    extra_keys()
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, code)| *code)
}

/// Parses `ctrl+shift+n` into `(modifier flags, virtual key code)`.
///
/// Returns a message describing the problem when the combo is malformed, uses
/// an unsupported key, or lacks Ctrl or Alt.
pub fn parse_hotkey(combo: &str) -> Result<(u32, u32), String> {
    let lowered = combo.trim().to_lowercase();
    let parts: Vec<&str> = lowered.split('+').filter(|part| !part.is_empty()).collect();

    let mods: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|part| MOD_ORDER.contains(part))
        .collect();
    let keys: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|part| !MOD_ORDER.contains(part))
        .collect();

    if keys.len() != 1 {
        return Err("expected modifiers plus one key, e.g. ctrl+shift+n".to_string());
    }
    let mut seen: Vec<&str> = Vec::new();
    for mod_name in &mods {
        if seen.contains(mod_name) {
            return Err("the same modifier was given twice".to_string());
        }
        seen.push(mod_name);
    }
    if mods.is_empty() {
        return Err("expected modifiers plus one key, e.g. ctrl+shift+n".to_string());
    }
    if !mods.contains(&"ctrl") && !mods.contains(&"alt") {
        return Err("combo must include Ctrl or Alt".to_string());
    }
    let vk = key_to_vk(keys[0]).ok_or_else(|| format!("unsupported key '{}'", keys[0]))?;
    let mut flags = 0;
    for mod_name in &mods {
        flags |= match *mod_name {
            "ctrl" => MOD_CONTROL,
            "shift" => MOD_SHIFT,
            "alt" => MOD_ALT,
            _ => return Err(format!("unsupported modifier '{mod_name}'")),
        };
    }
    Ok((flags, vk))
}

/// Formats a combo for display, e.g. `ctrl+shift+n` becomes `Ctrl+Shift+N`.
pub fn format_hotkey(combo: &str) -> String {
    let lowered = combo.trim().to_lowercase();
    let parts: Vec<&str> = lowered.split('+').filter(|part| !part.is_empty()).collect();
    let mut pretty: Vec<String> = Vec::new();
    for mod_name in MOD_ORDER {
        if parts.contains(&mod_name) {
            pretty.push(capitalize(mod_name));
        }
    }
    for part in &parts {
        if !MOD_ORDER.contains(part) {
            if part.len() == 1 {
                pretty.push(part.to_uppercase());
            } else {
                pretty.push(capitalize(part));
            }
        }
    }
    pretty.join("+")
}

/// Modifier and lock keys leave an in-progress capture unchanged.
pub fn ignored_capture_key(vk: u32) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B..=0x5C | 0x90..=0x91 | 0xA0..=0xA5)
}

/// Builds a combo string, distinguishing missing modifiers from unsupported keys.
pub fn combo_from_capture(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Result<String, String> {
    if !(ctrl || alt) {
        return Err("Ctrl or Alt is required.".to_string());
    }
    let key = vk_to_name(vk).ok_or_else(|| format!("Unsupported key (VK 0x{vk:02X})."))?;
    let mut parts: Vec<String> = Vec::new();
    if ctrl {
        parts.push("ctrl".to_string());
    }
    if shift {
        parts.push("shift".to_string());
    }
    if alt {
        parts.push("alt".to_string());
    }
    parts.push(key);
    Ok(parts.join("+"))
}

/// The canonical key name for a virtual key code.
pub fn vk_to_name(vk: u32) -> Option<String> {
    if (0x30..=0x39).contains(&vk) {
        return Some(((vk as u8) as char).to_string());
    }
    if (0x41..=0x5A).contains(&vk) {
        return Some(((vk as u8) as char).to_lowercase().to_string());
    }
    if (0x70..=0x7B).contains(&vk) {
        return Some(format!("f{}", vk - 0x70 + 1));
    }
    extra_keys()
        .iter()
        .find(|(_, code)| *code == vk)
        .map(|(name, _)| (*name).to_string())
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_typical_combo() {
        let (mods, vk) = parse_hotkey("ctrl+shift+n").unwrap();
        assert_eq!(mods, MOD_CONTROL | MOD_SHIFT);
        assert_eq!(vk, 0x4E);
    }

    #[test]
    fn rejects_a_combo_without_ctrl_or_alt() {
        assert!(parse_hotkey("shift+n").is_err());
    }

    #[test]
    fn rejects_an_unsupported_key() {
        assert!(parse_hotkey("ctrl+nosuchkey").is_err());
    }

    #[test]
    fn rejects_two_keys() {
        assert!(parse_hotkey("ctrl+a+b").is_err());
    }

    #[test]
    fn formats_for_display() {
        assert_eq!(format_hotkey("ctrl+shift+n"), "Ctrl+Shift+N");
        assert_eq!(format_hotkey("alt+f1"), "Alt+F1");
    }

    #[test]
    fn capture_round_trips_through_the_combo_language() {
        let combo = combo_from_capture(0x4E, true, true, false).unwrap();
        assert_eq!(combo, "ctrl+shift+n");
        assert_eq!(parse_hotkey(&combo).unwrap().1, 0x4E);
    }

    #[test]
    fn capture_ignores_both_modifier_sides_windows_and_locks() {
        for vk in [
            0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0x90, 0x91, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5,
        ] {
            assert!(ignored_capture_key(vk), "VK 0x{vk:02X}");
        }
        assert!(!ignored_capture_key(0x4E));
        assert_eq!(
            combo_from_capture(0x4E, false, true, false).unwrap_err(),
            "Ctrl or Alt is required."
        );
        assert_eq!(
            combo_from_capture(0xBA, true, false, false).unwrap_err(),
            "Unsupported key (VK 0xBA)."
        );
    }
}
