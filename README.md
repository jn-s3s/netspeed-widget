# NetSpeed Widget

A native Windows taskbar widget that shows live download/upload speed, latency,
and a rolling traffic graph.

[![CI](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml)
[![Release](https://github.com/jn-s3s/netspeed-widget/actions/workflows/release.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

![NetSpeed Widget showing live speeds and a traffic graph in the Windows taskbar](screenshots/taskbar-widget.svg)

## Run

Download and run the Windows x64 `.exe` release. It needs no installer or
additional runtime. Releases also include a ZIP with the executable, README
and license.

For source builds, install Rust through rustup and the Visual Studio C++ build
tools, including the Windows SDK. Run this from the repository folder; rustup
uses the MSVC toolchain pinned in `rust-toolchain.toml`:

```powershell
cargo run --locked
```

## Controls

- Choose automatic placement under **Taskbar** in the context menu. The widget cannot be dragged.
- Hover for live traffic, probe details, session totals and the last speedtest.
- Click to open or close Network details. The panel has a larger graph,
  Run speedtest, Settings and Close buttons; Tab navigates controls
  and Escape dismisses it. Clicking elsewhere dismisses the panel too.
- Start a manual speedtest with Run speedtest in the Network details panel.
- Right-click the widget or tray icon to open **Network details**, **Show widget** or **Quit**.
  **Appearance** contains layout, units, theme, background, graph and visual effects.
  **Taskbar** contains placement and, when multiple taskbars are available, **Display**.
  **Preferences** contains startup and the visibility shortcut. The Network details panel's
  **Settings** button opens this menu even when the widget has no room to appear.
- Ctrl+Shift+Alt+N toggles visibility. Change it under **Preferences → Change visibility shortcut…**.
- Click the tray icon to show or hide the widget.

Settings are stored in `%APPDATA%\NetSpeedWidget\config.json`. The log is
`log.txt` in the same folder; it rotates before a write would exceed 2 MB and
keeps two backups. Settings are cached, so quit the app before editing the file.
Missing or mistyped fields use defaults. If the whole file is unreadable or
invalid JSON, the app uses defaults and refuses to save changes until it can
read the file again.

Show/Hide controls your visibility preference. Automatic placement can hide the
widget temporarily when no safe space is available. The Default theme appears
as Follow Windows (transparent). It adapts its foreground to the taskbar's
light or dark setting and has no background or border, even on hover. Turn on
**Appearance → Subtle background** for more separation. The setting is disabled
for other themes, but its saved value is kept. Windows high-contrast colors
override every theme.

## Start with Windows

Enable **Preferences → Start with Windows** in the widget or tray context menu to launch at
sign-in for your Windows account. It is off by default and needs no administrator
access, service or scheduled task. Startup uses normal launch behavior, including
your saved Show widget preference (hide it to start tray-only).

The app stores the quoted executable path as `NetSpeedWidget` under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Disabling the option
removes only that Run value. The checkmark is read from the registry whenever
the menu opens, including Windows Startup Apps / Task Manager approval state.
If Windows disables the entry, enabling it here clears that disabled state.

- After moving or renaming the executable, enable the option again from its new
  location. The old registration appears unchecked and is replaced when enabled.
- All copies share one per-user startup entry; enabling another copy replaces
  the previous path. For portable or removable-drive usage, leave it disabled
  unless that path will be available at sign-in. Quit other copies before launching.
- If registry permissions or policy block an update, the app displays an error
  and records it in the rolling log. Failed approval updates restore the prior
  Run value; any failure to restore it is also reported. Organization policy can
  still prevent startup independently of this registration.

## Display

The live display uses neutral, semibold values with fixed digit cells and separate
unit columns. During normal operation, down arrows are cyan and up arrows are
lavender. Warnings turn all readout text amber; failed probes turn it red. Graph
colors stay unchanged. Fonts, spacing and placement gaps scale with the selected
taskbar's DPI.

Choose **Appearance → Layout** from the context menu:

| Layout             | Contents                                      |
| ------------------ | --------------------------------------------- |
| Compact            | Stacked live download/upload and probe status |
| Balanced (default) | Stacked speeds, optional graph, probe status  |
| Single line        | Both speeds and probe status in one row       |

Width is measured from the selected font and DPI, with fixed space reserved for
all supported rate values, units and status labels. Balanced also shows the
numeric latency below `SLOW`.

Taskbars shorter than 34 logical pixels use Single line to preserve readability.
Auto-fit crowded taskbars, enabled by default, lets automatic placement switch
Balanced to Compact before falling back to the tray. Density changes settle for
two seconds; expansion also requires extra free-space margin. Numeric updates and
hovering never change the widget's width.

**Appearance → Show traffic graph** requires Balanced and is disabled with an explanation
when the selected layout or current taskbar size cannot show it. Its saved value is retained;
the Network details graph remains available in every layout.

**Appearance → Animate error indicator** is enabled by default and pulses failed-probe text between two
opaque red shades. Turn it off for steady red text. The pulse also stops while
the widget is hidden, when Windows animations are disabled, or in high contrast.
High contrast uses the system text color for all readouts.

Under **Appearance → Units**, choose decimal bits (Kbps/Mbps/Gbps) or bytes (kB/s/MB/s/GB/s).
Prefixes adapt with hysteresis to avoid boundary flicker. Zero traffic is shown
as `0`; a missing or stale sample is `—`. Very small
nonzero readings use `<0.01` in the displayed unit. Traffic samples more than
three seconds old are treated as unavailable. Displayed rates are smoothed;
zero traffic resets the readout to zero immediately.

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
Both react to taskbar layout notifications, with polling about once per second
as a fallback for controls that do not send notifications. Prefer left side moves
back to the leftmost available gap when other controls shrink or disappear.
Next to the notification area returns to its anchor, with a DPI-scaled gap, when
space allows; while the anchor is blocked, it keeps a safe current position.
In either mode, the widget moves as soon as a usable layout scan completes,
without an additional movement delay. Event-driven scans are rate-limited to
one every 32 ms; detection speed still depends on the accessibility provider.
It waits one second before temporarily hiding when no gap fits or the layout
cannot be read, so brief detection failures do not cause flicker. The tray icon
remains available, and the widget returns automatically when space is available again.

Detection uses Windows accessibility information and visible taskbar child
windows; third-party tools may not expose their occupied area reliably.
Placement is automatic; use **Taskbar** in the context menu to choose a side and display.

The widget uses square corners and fills the taskbar below its one-physical-pixel
top separator. Context menus open at the cursor using native positioning and
stay in front of the widget.

Position follows the selected taskbar, with fallback to the primary display.
Discovery retries after Explorer restarts. The tray icon is re-added if the
shell loses it. If no taskbar is available, the widget waits for one rather than
appearing as a floating window. Horizontal taskbars are the supported layout;
vertical taskbars and separate fullscreen-suppression logic are not supported.

## Network measurements

Windows IP Helper counters are sampled on a background thread once a second.
The app sums counters across all interfaces, so these rates and session totals
are not limited to one adapter or to Internet traffic. Traffic passing through
multiple interfaces, such as a VPN and its underlying adapter, can be counted
more than once.

Latency uses a TCP connection to `1.1.1.1:443`, followed by a two-second wait
between probes. If TCP fails, the app tries one ICMP ping using Windows' system
`ping.exe`. Every layout shows numeric TCP latency below 80 ms, `SLOW`
at 80 ms or above, `CHECK` when TCP fails but ICMP responds, or `FAIL` when both
probes fail. A failed probe does not prove that all internet access is unavailable.
Warnings use amber text and failures use red, except in high contrast. Hover or
open Network details for an explanation.

The status is `—` before the first result or when the result is more than ten
seconds old. During speedtests, `TEST` replaces the probe status and suppresses
alert colors and animation without changing recorded probe health.

Speedtests run manually or every four hours after a measured result. On launch,
an overdue or missing result gets a 20-second grace period. The app uses a
built-in HTTPS transfer test against `speed.cloudflare.com`, with no external
runtime needed.

If the active test fails, the app watches system counters for ten seconds. It
reports a passive estimate only if download or upload averages at least 1 Mbps.
The estimate is not saved as a measurement. Failed tests and passive estimates
schedule another attempt after 15 minutes.

Active tests request 25 MB down and upload a 256 KiB calibration payload, then
up to 8 MiB more. Automatic tests consume data. Results estimate throughput to
that service, not guaranteed line capacity.

## Build and verify

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Check documentation with warnings treated as errors too:

```powershell
$env:RUSTDOCFLAGS = '-D warnings'
cargo doc --locked --workspace --no-deps
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
- `src/win/`: taskbar discovery and attachment, occupancy scans, startup
  registration, tray, menu, dialog and GDI helpers.
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
attaches both a standalone x64 executable and a ZIP containing the executable,
README and license to a draft; publication remains a maintainer action.

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution and release conventions,
[SECURITY.md](SECURITY.md) for private vulnerability reporting, and
[LICENSE](LICENSE) for the MIT license.
