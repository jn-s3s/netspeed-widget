//! User settings stored in a cached JSON file.
//!
//! Changes update the cache and attempt to save through a sibling file that is
//! renamed into place. Save failures are logged and leave the old file intact.
//!
//! Missing or mistyped fields use defaults without discarding valid fields.
//! An unreadable file or invalid JSON is logged and uses display defaults.
//! Before accepting a change after a load failure, the cache retries the read;
//! it refuses the change if the file still cannot be loaded.

use std::fs;
use std::sync::{Mutex, OnceLock};

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};

use crate::logger;
use crate::paths::config_path;
use crate::theme::{theme_names, DEFAULT_THEME};

/// Name of the settings file inside the app data directory.
pub const CONFIG_FILE: &str = "config.json";
/// Hotkey used when nothing is stored.
pub const DEFAULT_HOTKEY: &str = "ctrl+shift+alt+n";

/// Where the widget sits inside the taskbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Placement {
    /// Just left of the notification area.
    #[default]
    #[serde(alias = "manual")]
    Tray,
    /// Prefer an available gap on the left side of the taskbar.
    Left,
}

/// Preferred taskbar density. Auto-fit may temporarily use Compact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayLayout {
    Compact,
    #[default]
    Balanced,
    SingleLine,
}

/// Throughput units; byte units are decimal, not binary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedUnits {
    #[default]
    Bits,
    Bytes,
}

/// One persisted speedtest measurement with its completion time.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SpeedtestRecord {
    pub down_mbps: f64,
    pub up_mbps: f64,
    pub ts: f64,
}

/// Settings whose fields accept missing or mistyped values by using defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, deserialize_with = "tolerant")]
    pub layout: DisplayLayout,
    #[serde(default, deserialize_with = "tolerant")]
    pub units: SpeedUnits,
    #[serde(default = "enabled", deserialize_with = "tolerant_enabled")]
    pub auto_fit: bool,
    #[serde(default, deserialize_with = "tolerant")]
    pub subtle_background: bool,
    #[serde(default, deserialize_with = "tolerant")]
    pub graph_fill: bool,
    #[serde(default = "enabled", deserialize_with = "tolerant_enabled")]
    pub animate_errors: bool,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub speedtest_source: Option<String>,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub speedtest: Option<SpeedtestRecord>,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub hotkey: Option<String>,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub theme: Option<String>,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub placement: Option<Placement>,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub show_graph: Option<bool>,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub widget_visible: Option<bool>,
    #[serde(
        default,
        deserialize_with = "tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub taskbar_monitor: Option<String>,
}

fn tolerant<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

fn tolerant_enabled<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or(true))
}

/// A failed read may be used for display defaults, never as a save baseline.
struct CachedConfig {
    config: Config,
    trusted: bool,
}

impl std::ops::Deref for CachedConfig {
    type Target = Config;
    fn deref(&self) -> &Config {
        &self.config
    }
}

fn enabled() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            layout: DisplayLayout::Balanced,
            units: SpeedUnits::Bits,
            auto_fit: true,
            subtle_background: false,
            graph_fill: false,
            animate_errors: true,
            speedtest_source: None,
            speedtest: None,
            hotkey: None,
            theme: None,
            placement: None,
            show_graph: None,
            widget_visible: None,
            taskbar_monitor: None,
        }
    }
}

/// Display preferences are read together to avoid inconsistent snapshots.
pub struct DisplayPreferences {
    pub layout: DisplayLayout,
    pub units: SpeedUnits,
    pub auto_fit: bool,
    pub subtle_background: bool,
    pub graph_fill: bool,
    pub animate_errors: bool,
}

pub fn display_preferences() -> DisplayPreferences {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    DisplayPreferences {
        layout: guard.layout,
        units: guard.units,
        auto_fit: guard.auto_fit,
        subtle_background: guard.subtle_background,
        graph_fill: guard.graph_fill,
        animate_errors: guard.animate_errors,
    }
}

pub enum DisplayPreference {
    Layout(DisplayLayout),
    Units(SpeedUnits),
    AutoFit(bool),
    Background(bool),
    Fill(bool),
    AnimateErrors(bool),
}

pub fn set_display_preference(preference: DisplayPreference) {
    mutate(|config| apply_display_preference(config, preference));
}

fn apply_display_preference(config: &mut Config, preference: DisplayPreference) {
    match preference {
        DisplayPreference::Layout(value) => config.layout = value,
        DisplayPreference::Units(value) => config.units = value,
        DisplayPreference::AutoFit(value) => config.auto_fit = value,
        DisplayPreference::Background(value) => config.subtle_background = value,
        DisplayPreference::Fill(value) => config.graph_fill = value,
        DisplayPreference::AnimateErrors(value) => config.animate_errors = value,
    }
}

pub fn speedtest_source() -> Option<String> {
    cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .speedtest_source
        .clone()
}

pub fn set_speedtest_source(source: &str) {
    mutate(|config| config.speedtest_source = Some(source.to_string()));
}

/// The cached settings, loaded on first use.
static CONFIG: OnceLock<Mutex<CachedConfig>> = OnceLock::new();

fn cache() -> &'static Mutex<CachedConfig> {
    CONFIG.get_or_init(|| {
        Mutex::new(match load(&config_path(CONFIG_FILE)) {
            Ok(config) => CachedConfig {
                config,
                trusted: true,
            },
            Err(_) => CachedConfig {
                config: Config::default(),
                trusted: false,
            },
        })
    })
}

/// Loads settings, using defaults for a missing file and logging other failures.
///
/// # Errors
/// Returns an error if the file cannot be read or parsed as settings JSON.
fn load(path: &std::path::Path) -> Result<Config, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(err) => {
            logger::error(
                "config",
                &format!("config at {} could not be read: {err}", path.display()),
            );
            return Err(err.to_string());
        }
    };
    match serde_json::from_str::<Config>(&text) {
        Ok(config) => Ok(config),
        Err(err) => {
            logger::warning(
                "config",
                &format!(
                    "invalid config at {}: {err}; saving is disabled until it can be read",
                    path.display()
                ),
            );
            Err(err.to_string())
        }
    }
}

/// Writes the settings in one step, leaving the old file intact on failure.
fn save(config: &Config, path: &std::path::Path) {
    let staging = path.with_extension("json.tmp");
    let text = match serde_json::to_string_pretty(config) {
        Ok(text) => text,
        Err(err) => {
            logger::error("config", &format!("could not serialise the config: {err}"));
            return;
        }
    };
    if let Err(err) = fs::write(&staging, text) {
        logger::error(
            "config",
            &format!("failed to save config via {}: {err}", staging.display()),
        );
        remove_staging_file(&staging);
        return;
    }
    if let Err(err) = fs::rename(&staging, path) {
        logger::error(
            "config",
            &format!("failed to replace config at {}: {err}", path.display()),
        );
        remove_staging_file(&staging);
    }
}

fn remove_staging_file(path: &std::path::Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        // A failed staging write need not have created a file.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => logger::warning(
            "config",
            &format!("could not remove staging file {}: {err}", path.display()),
        ),
    }
}

/// Applies and saves a change only after establishing a readable settings baseline.
fn mutate(apply: impl FnOnce(&mut Config)) {
    let mut guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    mutate_cached(&mut guard, &config_path(CONFIG_FILE), apply);
}

fn mutate_cached(
    guard: &mut CachedConfig,
    path: &std::path::Path,
    apply: impl FnOnce(&mut Config),
) {
    if !guard.trusted {
        let Ok(config) = load(path) else {
            logger::warning(
                "config",
                "settings change not saved: no trustworthy settings baseline",
            );
            return;
        };
        guard.config = config;
        guard.trusted = true;
    }
    apply(&mut guard.config);
    save(&guard.config, path);
}

/// The saved preset name, or Default when missing or invalid.
pub fn theme() -> String {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    match guard.theme.as_deref() {
        Some(name) if theme_names().contains(&name) => name.to_string(),
        _ => DEFAULT_THEME.to_string(),
    }
}

/// Saves a known theme preset. Unknown names are ignored.
pub fn set_theme(name: &str) {
    if !theme_names().contains(&name) {
        return;
    }
    mutate(|config| config.theme = Some(name.to_string()));
}

/// Returns the saved show/hide shortcut, or the default if missing or blank.
/// Parsing and registration are handled by the caller.
pub fn hotkey() -> String {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    match guard.hotkey.as_deref() {
        Some(combo) if !combo.trim().is_empty() => combo.to_string(),
        _ => DEFAULT_HOTKEY.to_string(),
    }
}

/// Persists the global show/hide hotkey combo.
pub fn set_hotkey(combo: &str) {
    mutate(|config| config.hotkey = Some(combo.to_string()));
}

/// Returns the cached speedtest record, if present.
pub fn speedtest() -> Option<SpeedtestRecord> {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    guard.speedtest
}

/// Rounds rates to two decimals, timestamps the result and attempts to save it.
/// Returns the record even if saving fails.
pub fn set_speedtest(down_mbps: f64, up_mbps: f64) -> SpeedtestRecord {
    let record = SpeedtestRecord {
        down_mbps: (down_mbps * 100.0).round() / 100.0,
        up_mbps: (up_mbps * 100.0).round() / 100.0,
        ts: crate::now_epoch(),
    };
    mutate(|config| config.speedtest = Some(record));
    record
}

/// Where the widget sits inside the taskbar.
pub fn placement() -> Placement {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    guard.placement.unwrap_or_default()
}

/// Persists the taskbar placement.
pub fn set_placement(placement: Placement) {
    mutate(|config| config.placement = Some(placement));
}

/// Returns the graph visibility preference; the effective layout may hide it.
pub fn show_graph() -> bool {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    guard.show_graph.unwrap_or(true)
}

/// Persists whether the traffic graph is drawn.
pub fn set_show_graph(value: bool) {
    mutate(|config| config.show_graph = Some(value));
}

/// Returns the visibility preference; lack of taskbar space may hide the widget.
pub fn widget_visible() -> bool {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    guard.widget_visible.unwrap_or(true)
}

/// Persists whether the taskbar widget is visible.
pub fn set_widget_visible(value: bool) {
    mutate(|config| config.widget_visible = Some(value));
}

/// The display device the widget is pinned to, if one was chosen.
pub fn taskbar_monitor() -> Option<String> {
    let guard = cache().lock().unwrap_or_else(|err| err.into_inner());
    guard.taskbar_monitor.clone()
}

/// Persists the display device the widget is pinned to.
pub fn set_taskbar_monitor(device: Option<String>) {
    mutate(|config| config.taskbar_monitor = device);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_profile_is_not_replaced_and_is_reloaded_before_mutation() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory =
            std::env::temp_dir().join(format!("netspeed-config-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.json");
        fs::write(&path, r#"{"theme":"Ocean","show_graph":true,"layout":"compact","units":"bytes","auto_fit":false}"#).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(load(&path).is_err());
        let mut cache = CachedConfig {
            config: Config::default(),
            trusted: false,
        };
        mutate_cached(&mut cache, &path, |config| config.show_graph = Some(false));
        assert!(!cache.trusted);
        assert!(!path.with_extension("json.tmp").exists());
        drop(lock);
        assert_eq!(load(&path).unwrap().show_graph, Some(true));
        mutate_cached(&mut cache, &path, |config| config.show_graph = Some(false));
        let restored = load(&path).unwrap();
        assert_eq!(restored.theme.as_deref(), Some("Ocean"));
        assert_eq!(restored.show_graph, Some(false));
        // An AppState initialized from defaults must only save the clicked
        // preference after recovery, not its other default display values.
        mutate_cached(&mut cache, &path, |config| {
            apply_display_preference(config, DisplayPreference::Fill(true))
        });
        let restored = load(&path).unwrap();
        assert_eq!(restored.layout, DisplayLayout::Compact);
        assert_eq!(restored.units, SpeedUnits::Bytes);
        assert!(!restored.auto_fit);
        assert!(restored.graph_fill);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn invalid_fields_do_not_discard_valid_siblings_or_invent_measurements() {
        let config: Config = serde_json::from_str(r#"{"theme":"Ocean","speedtest":{"down_mbps":3},"units":42,"auto_fit":"bad","show_graph":false}"#).unwrap();
        assert_eq!(config.theme.as_deref(), Some("Ocean"));
        assert!(config.speedtest.is_none());
        assert_eq!(config.units, SpeedUnits::Bits);
        assert!(config.auto_fit);
        assert_eq!(config.show_graph, Some(false));
    }

    #[test]
    fn removed_settings_are_ignored_and_not_saved_again() {
        let config: Config = serde_json::from_str(
            r#"{"opacity":0.4,"hide_on_hover":true,"auto_hide":true,"theme":"System","show_graph":false}"#,
        )
        .unwrap();
        assert_eq!(config.theme.as_deref(), Some("System"));
        assert_eq!(config.show_graph, Some(false));
        let saved = serde_json::to_value(config).unwrap();
        assert!(saved.get("opacity").is_none());
        assert!(saved.get("auto_hide").is_none());
        assert!(saved.get("hide_on_hover").is_none());
    }

    #[test]
    fn unknown_theme_is_not_accepted() {
        let names = theme_names();
        assert!(!names.contains(&"Not A Theme"));
    }

    #[test]
    fn old_configs_receive_display_defaults_and_new_options_round_trip() {
        let old: Config = serde_json::from_str(r#"{"theme":"Ocean"}"#).unwrap();
        assert_eq!(old.layout, DisplayLayout::Balanced);
        assert!(old.auto_fit);
        assert!(old.animate_errors);
        assert_eq!(old.units, SpeedUnits::Bits);
        let config = Config {
            layout: DisplayLayout::Compact,
            units: SpeedUnits::Bytes,
            auto_fit: false,
            graph_fill: true,
            animate_errors: false,
            subtle_background: true,
            ..Config::default()
        };
        let restored: Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(restored.layout, DisplayLayout::Compact);
        assert_eq!(restored.units, SpeedUnits::Bytes);
        assert!(!restored.auto_fit);
        assert!(restored.graph_fill && restored.subtle_background);
        assert!(!restored.animate_errors);
    }

    #[test]
    fn automatic_placement_settings_stay_compatible_and_manual_migrates() {
        for (name, placement) in [
            ("left", Placement::Left),
            ("tray", Placement::Tray),
            ("manual", Placement::Tray),
        ] {
            let config: Config = serde_json::from_value(serde_json::json!({
                "placement": name,
                "tray_offset": 123,
                "theme": "Ocean",
                "show_graph": false,
                "units": "bytes"
            }))
            .unwrap();
            assert_eq!(config.placement, Some(placement));
            let saved = serde_json::to_value(config).unwrap();
            assert_eq!(
                saved["placement"],
                if name == "manual" { "tray" } else { name }
            );
            assert!(saved.get("tray_offset").is_none());
            assert_eq!(saved["theme"], "Ocean");
            assert_eq!(saved["show_graph"], false);
            assert_eq!(saved["units"], "bytes");
        }
    }
}
