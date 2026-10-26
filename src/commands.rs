//! Stable native menu/control identifiers, shared with lifecycle tests.

/// Starts a speedtest with the built-in HTTPS test and passive fallback.
pub const CMD_RUN_SPEEDTEST: u32 = 1001;
/// Toggles the taskbar traffic graph.
pub const CMD_TOGGLE_GRAPH: u32 = 1002;
/// Places the widget beside the notification area.
pub const CMD_PLACEMENT_TRAY: u32 = 1003;
/// Places the widget in an available left-side taskbar gap.
pub const CMD_PLACEMENT_LEFT: u32 = 1004;
/// Opens the global hotkey capture dialog.
pub const CMD_CHANGE_HOTKEY: u32 = 1005;
/// Toggles widget visibility without removing the tray icon.
pub const CMD_TOGGLE_VISIBILITY: u32 = 1007;
/// Shuts down the application.
pub const CMD_QUIT: u32 = 1009;
/// Opens the live traffic details panel.
pub const CMD_DETAILS: u32 = 1012;
/// Opens the settings menu.
pub const CMD_SETTINGS: u32 = 1013;
/// Toggles per-user Windows sign-in startup for the current executable.
pub const CMD_TOGGLE_AUTOSTART: u32 = 1014;
/// Selects the compact taskbar layout.
pub const CMD_LAYOUT_COMPACT: u32 = 1100;
/// Selects the balanced taskbar layout.
pub const CMD_LAYOUT_BALANCED: u32 = 1101;
/// Selects the single-line taskbar layout.
pub const CMD_LAYOUT_SINGLE_LINE: u32 = 1102;
/// Displays throughput in bits per second.
pub const CMD_UNITS_BITS: u32 = 1110;
/// Displays throughput in decimal bytes per second.
pub const CMD_UNITS_BYTES: u32 = 1111;
/// Toggles automatic layout fitting.
pub const CMD_AUTO_FIT: u32 = 1112;
/// Toggles the subtle background preference for the Default theme.
pub const CMD_SUBTLE_BACKGROUND: u32 = 1113;
/// Toggles the traffic graph fill.
pub const CMD_GRAPH_FILL: u32 = 1114;
/// Toggles the failed-probe text pulse.
pub const CMD_ANIMATE_ERRORS: u32 = 1115;
/// First command ID reserved for theme presets, indexed by preset order.
pub const CMD_THEME_BASE: u32 = 2000;
/// First command ID reserved for taskbar monitors, indexed by menu order.
pub const CMD_TASKBAR_BASE: u32 = 4000;
/// Native control ID of the details panel's read-only text body.
pub const DETAILS_BODY_ID: i32 = 10;
