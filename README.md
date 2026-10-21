# NetSpeed Widget

A native Windows taskbar widget that shows live download/upload speed, latency,
and a rolling traffic graph.

[![CI](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Run

Download and extract a Windows x64 release, then run `netspeed-widget.exe`.
The executable is standalone and requires no installer or additional runtime.

For source builds, install stable Rust with the Windows MSVC toolchain and the
Visual Studio C++ build tools, including the Windows SDK:

```powershell
cargo run --locked
```

## Controls

- Choose automatic placement from the context menu; dragging is not supported.
- Hover for live traffic, probe details, session totals and the last speedtest.
- Click to open or close Network details. The panel has a larger graph,
  Run speedtest, Settings and Close buttons; Tab navigates controls
  and Escape dismisses it. Clicking elsewhere dismisses the panel too.
- Start a manual speedtest with Run speedtest in the Network details panel.
- Right-click the widget or tray icon for themes, traffic graph,
  layouts, units, placement, display selection,
  hotkey and quit. Network details is also available from the tray menu
  when the widget has no room to appear.
- Ctrl+Shift+Alt+N toggles visibility. Change the shortcut from the menu.
- Click the tray icon to show or hide the widget.

Settings and the rolling log are stored in `%APPDATA%\NetSpeedWidget`.
Show/Hide controls your visibility preference; automatic placement can
temporarily hide the widget when no safe space is available. The Default theme,
listed as Follow Windows (transparent), adapts its foreground to the taskbar's
light/dark setting and has no background or border, including on hover. An optional
Subtle background improves separation. Other themes keep their colored
backgrounds. Windows high-contrast colors override all presets.

## Display

The live display uses neutral, semibold values with fixed digit cells and separate
unit columns. Cyan down arrows and lavender up arrows keep their meaning even
when a latency probe fails. Fonts, spacing and placement gaps scale with the
selected taskbar's DPI.

Choose a Layout from the context menu:

| Layout             | Contents                                     | Width at 100% scaling        |
| ------------------ | -------------------------------------------- | ---------------------------- |
| Compact            | Stacked live download and upload             | 128 px                       |
| Balanced (default) | Stacked speeds, optional graph, probe status | 224 px; 168 px without graph |
| Single line        | Both speeds and probe status in one row      | 288 px                       |

Taskbars shorter than 34 logical pixels use Single line to preserve readability.
Auto-fit crowded taskbars, enabled by default, lets automatic placement switch
Balanced to Compact before falling back to the tray. Density changes settle for
two seconds; expansion also requires extra free-space margin. Numeric updates and
hovering never change the widget's width.

Under Units, choose decimal bits (Kbps/Mbps/Gbps) or bytes (kB/s/MB/s/GB/s).
Prefixes adapt with hysteresis to avoid boundary flicker. Zero traffic is shown
as `0`; a missing or stale sample is `—`. Very small
nonzero readings use `<0.01` in the displayed unit.

The graph shows a fixed 60-second time window with a shared automatic download/
upload scale. It uses thin lines without a grid or border; Shade download area
adds a 30%-opacity download-colored fill and is disabled in high contrast.
At idle or very low download rates, there is little area to shade, so the
difference can still be small. Per-pixel peaks preserve brief bursts,
missing intervals leave gaps, and scale decay follows sample time, not repaints.
Graph height shows relative activity, not connection-capacity utilization.

Network details includes recent peaks, session totals, and the last measured
speedtest's age and provider, when available. Passive estimates are shown
separately from saved measurements. The native,
read-only details text can be focused, scrolled and copied with the keyboard.

## Taskbar integration

The widget is a non-activating child window embedded in Explorer's taskbar. It follows the
taskbar's visibility and stacking instead of repeatedly raising a floating
topmost window. It does not reserve taskbar space or change the taskbar's styles.
Only the widget's rectangle receives its clicks.

For placement, choose Prefer left side or Next to the notification area.
Both check exposed taskbar controls about once per second, including changes
from opening apps or switching icon alignment. The widget stays put while it
fits, waits for layout changes to settle before moving to another gap, and
temporarily hides when no gap fits or the layout cannot be read. The tray icon
remains available, and the widget returns automatically when space is available again.

Detection uses Windows accessibility information and visible taskbar child
windows; third-party tools may not expose their occupied area reliably.
Placement is automatic; use the context menu to choose a side and display.

The widget fills the taskbar height and uses square corners. Context menus open
at the cursor using native positioning and stay in front of the widget.

Position follows the selected taskbar, with fallback to the primary display.
Discovery retries after Explorer restarts. The tray icon is re-added if the
shell loses it. If no taskbar is available, the widget waits for one rather than
appearing as a floating window. Horizontal taskbars are the supported layout;
vertical taskbars and separate fullscreen-suppression logic are not supported.

## Network measurements

Windows IP Helper counters are sampled on a background thread once a second.
Latency uses a TCP connection to `1.1.1.1:443` every two seconds, with a single
ICMP ping fallback. Green means healthy, amber means slow or TCP-blocked, and
red means neither probe reached its target. A red status does not prove that all
internet access is unavailable. Status uses a localized marker (`!` for warnings and `×` for
failures); download/upload accents do not change. Health colors are frozen during
speedtests to avoid recording load-induced latency as an outage.

Speedtests run manually or every four hours. The app uses a built-in HTTPS
transfer test against `speed.cloudflare.com`, with no external runtime needed.
The final passive observation is labelled
as an estimate. The app does not save the estimate as a
measurement and retries sooner.
Active HTTP tests transfer up to 25 MB down and 8 MiB up; automatic tests consume
data. Results are estimates of throughput to that service, not guaranteed line
capacity.

## Build and verify

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

The standalone executable is `target\release\netspeed-widget.exe`, with its icon
and version embedded. Release builds do not open a console window.

The Windows lifecycle test launches the real application with isolated settings,
checks taskbar parenting and child-window styles, verifies that mouse movement
does not capture or reposition the widget, and checks menus, visibility controls,
settings handling, recovery and clean shutdown.
Quit any running copy before testing. The lifecycle test reports a skip when
the current Windows session has no Explorer taskbar.

## Source layout

- `src/app.rs`: Win32 controller, widget, interactions and timers.
- `src/win/`: taskbar discovery and attachment, tray, menu, dialog and GDI helpers.
- `src/win/details.rs`, `src/win/tooltip.rs`, `src/win/appearance.rs`: native
  details panel, hover information and Windows appearance detection.
- `src/canvas.rs`, `src/paint.rs`, `src/graph.rs`, `src/theme.rs`: rendering.
- `src/sampler.rs`, `src/latency.rs`, `src/speedtest.rs`: background measurements.
- `src/config.rs`, `src/paths.rs`, `src/logger.rs`: persistent state and logging.
- `src/format.rs`, `src/hotkey.rs`, `src/schedule.rs`, `src/clock.rs`: formatting,
  shortcut parsing, scheduling and shared time helpers.
- `src/lib.rs`, `src/commands.rs`: shared native command identifiers for tests.
- `tests/windows_lifecycle.rs`: real-window regression test.
- `Cargo.toml`, `Cargo.lock`, `build.rs`: dependencies, version and resources.

CI and release builds run the same Rust checks on Windows. Release tags must
match `Cargo.toml` and the newest dated changelog entry. The release workflow
packages a standalone x64 executable and creates a draft; publication remains
a maintainer action.

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution and release conventions,
[SECURITY.md](SECURITY.md) for private vulnerability reporting, and
[LICENSE](LICENSE) for the MIT license.
