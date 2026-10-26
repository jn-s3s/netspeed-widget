//! Number formatting shared by the widget and the tray.

use crate::config::SpeedUnits;

/// A fixed number slot and a separate unit slot keep changing values steady.
#[derive(Clone, Debug)]
pub struct RateText {
    pub value: String,
    pub unit: &'static str,
}

/// Stateful unit selection with a 10% release margin to avoid boundary flicker.
pub struct RateFormatter {
    tier: usize,
}

impl Default for RateFormatter {
    fn default() -> Self {
        Self { tier: 1 }
    }
}

impl RateFormatter {
    pub fn format(&mut self, mbps: Option<f64>, units: SpeedUnits) -> RateText {
        let labels = match units {
            SpeedUnits::Bits => ["Kbps", "Mbps", "Gbps", "Tbps"],
            SpeedUnits::Bytes => ["kB/s", "MB/s", "GB/s", "TB/s"],
        };
        let Some(mbps) = mbps.filter(|value| value.is_finite() && *value >= 0.0) else {
            return RateText {
                value: "—".to_string(),
                unit: labels[self.tier],
            };
        };
        let rate = if units == SpeedUnits::Bytes {
            mbps / 8.0
        } else {
            mbps
        } * 1000.0;
        if rate == 0.0 {
            self.tier = 1;
        }
        while self.tier < 3 && rate / 1000_f64.powi(self.tier as i32) >= 999.5 {
            self.tier += 1;
        }
        while rate > 0.0 && self.tier > 0 && rate / 1000_f64.powi(self.tier as i32) < 0.9 {
            self.tier -= 1;
        }
        let value = rate / 1000_f64.powi(self.tier as i32);
        let text = if value == 0.0 {
            "0".to_string()
        } else if self.tier == 3 && value >= 999.5 {
            "999+".to_string()
        } else if value < 0.005 {
            "<0.01".to_string()
        } else {
            format_speed(value)
        };
        RateText {
            value: text,
            unit: labels[self.tier],
        }
    }
}

pub fn transfer_label(mb: f64) -> String {
    if (mb * 10.0).round() >= 10_000.0 {
        format!("{:.2} GB", mb / 1000.0)
    } else {
        format!("{mb:.1} MB")
    }
}

pub fn age_label(ts: f64, now: f64) -> String {
    let seconds = (now - ts).max(0.0) as u64;
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86399 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86400),
    }
}

/// Formats a speed with precision matched to its magnitude.
pub fn format_speed(mbps: f64) -> String {
    if !mbps.is_finite() {
        return "0.00".to_string();
    }
    if mbps >= 99.95 {
        format!("{mbps:.0}")
    } else if mbps >= 9.995 {
        format!("{mbps:.1}")
    } else {
        format!("{mbps:.2}")
    }
}

/// Last-speedtest line, falling back to a placeholder before any run.
pub fn speedtest_label(summary: &str) -> String {
    if summary.trim().is_empty() {
        "Last speedtest: --".to_string()
    } else {
        summary.to_string()
    }
}

/// Hotkey action label matching the registration state.
pub fn hotkey_label(ok: bool, combo: &str) -> String {
    if ok {
        format!(
            "Change visibility shortcut… ({})",
            crate::hotkey::format_hotkey(combo)
        )
    } else {
        "Change visibility shortcut… (none active)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_and_transfer_boundaries() {
        for (age, expected) in [
            (0.0, "just now"),
            (59.0, "just now"),
            (60.0, "1 min ago"),
            (3599.0, "59 min ago"),
            (3600.0, "1 h ago"),
            (86399.0, "23 h ago"),
            (86400.0, "1 days ago"),
        ] {
            assert_eq!(age_label(0.0, age), expected);
        }
        assert_eq!(age_label(10.0, 0.0), "just now");
        assert_eq!(transfer_label(999.94), "999.9 MB");
        assert_eq!(transfer_label(999.96), "1.00 GB");
        assert_eq!(transfer_label(1000.0), "1.00 GB");
    }

    #[test]
    fn speed_precision_follows_magnitude() {
        assert_eq!(format_speed(0.5), "0.50");
        assert_eq!(format_speed(12.34), "12.3");
        assert_eq!(format_speed(123.4), "123");
    }

    #[test]
    fn rate_units_are_explicit_and_hysteretic() {
        let mut formatter = RateFormatter::default();
        assert_eq!(formatter.format(Some(999.6), SpeedUnits::Bits).unit, "Gbps");
        assert_eq!(formatter.format(Some(980.0), SpeedUnits::Bits).unit, "Gbps");
        assert_eq!(formatter.format(Some(890.0), SpeedUnits::Bits).unit, "Mbps");
        assert_eq!(formatter.format(Some(8.0), SpeedUnits::Bytes).value, "1.00");
        assert_eq!(formatter.format(None, SpeedUnits::Bits).value, "—");
        assert_eq!(formatter.format(Some(0.0), SpeedUnits::Bits).value, "0");
        assert_eq!(
            formatter.format(Some(0.001), SpeedUnits::Bits).value,
            "1.00"
        );
        assert_eq!(formatter.format(Some(0.001), SpeedUnits::Bits).unit, "Kbps");
        assert_eq!(
            formatter.format(Some(0.000001), SpeedUnits::Bits).value,
            "<0.01"
        );
    }
}
