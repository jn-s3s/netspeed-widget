# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Pushing a `v*` tag runs the release workflow, which builds the app and drafts a GitHub release with the packaged zip. The notes on that release are curated from the matching entry in this file, so an entry here should say what changed for someone running or building the app, not repeat a pull request list.

## [Unreleased]

## [1.1.0] - 2026-09-28

### Added

- Pill-shaped widget with integrated speed graph and tray status indicators
- Theme system with 20 named color palettes, selectable from widget and tray menus
- Health-tinted visualization: graph segments and speed rows reflect connection latency status
- Multi-backend speedtest engine with provider fallback
- Global hotkey to show/hide the widget (replaces per-key opacity controls)
- Persisted configuration: position, hover-hide state, hotkey, and theme preferences saved across sessions
- Dedicated sampler thread for network monitoring with counter reset handling
- Background latency probe with health status tracking
- Pyright type checking integrated into CI and development workflow

### Changed

- Switched to onedir PyInstaller packaging; release artifacts are now zipped directories
- Consolidated CI workflows into `ci.yml` (lint/test) and `release.yml` (build/publish)
- All GitHub Actions pinned to commit SHAs for supply-chain security
- Dependencies pinned with SHA256 hashes; vendored fast-cli tree includes committed lockfile
- Extracted modules: `graph`, `monitors`, `schedule`, `theme`, `version`, `hotkey_dialog`, `menu_labels`
- Narrowed config exception handling and surface save errors
- Shared speed formatter extracted to `utils/format.py`
- Dynamic menu layout with ellipsization for constrained widths
- Tray menu mirrors widget context menu structure

### Fixed

- UI tick rescheduled from `finally` block so render failures no longer freeze updates
- Tray icon properly stopped on shutdown
- UI callbacks dropped after window destroyed to prevent post-exit errors
- Speedtest runs serialized on Tk thread to prevent race conditions
- Menu entries use dynamic indexes instead of hardcoded positions
- Sampler clamps negative deltas on counter resets and seeds baseline on first reading
- Startup monitor lookup hardened with fallback to safe position
- Thread-safe config saves via temp file and `os.replace`
- RegisterHotKey binds new combo before releasing old one
- Correct pystray icon type annotation

## [1.0.0] - 2025-09-03

Initial release of the NetSpeed Widget — a small always-on-top Windows widget that shows live network speed, latency and a rolling traffic graph.

### Added

- Live download and upload speed display
- Rolling traffic graph that rescales to recent peaks
- Latency probe with color-coded health indicator
- System tray integration with context menu (widget lives in the tray, no taskbar icon)
- Periodic background speedtest using a vendored fast-cli backend
- Opacity controls accessible from the tray menu
- Persistent settings across sessions (position, opacity, last speedtest result)
- Logger for diagnostics and troubleshooting
- Modular code structure with separated concerns

[Unreleased]: https://github.com/jn-s3s/netspeed-widget/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/jn-s3s/netspeed-widget/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/jn-s3s/netspeed-widget/releases/tag/v1.0.0
