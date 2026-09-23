# NetSpeed Widget

<p align="center">
  <img src="icon.ico" width="96" alt="NetSpeed Widget icon">
</p>

A tiny always-on-top widget for Windows that shows your live network speed, ping and a rolling traffic graph. It sits in a corner of your screen, stays out of the way and gives you a one-click speedtest when you need it.


[![CI](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml)
[![Release](https://github.com/jn-s3s/netspeed-widget/actions/workflows/release.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)



<!-- INSERT IMAGE HERE -->

## What it shows

- Live download and upload speed, in Mb/s.
- Latency to a fast CDN server, measured with a plain TCP connect (with an ICMP ping as a fallback when outbound TCP is blocked). A colored dot tells you at a glance whether things look good, slow or offline.
- A rolling graph of the last minute of traffic that rescales itself to the biggest spike, so busy and quiet moments both stay readable.
- Your last speedtest result on the widget itself (a smaller second row), plus the right-click menu and tray tooltip.
- It follows your screens: if the display it was sitting on gets disconnected or disabled, it snaps back to the active screen instead of hiding off-screen.
- How much data you have downloaded and uploaded since you started the app (right-click menu).
- Peak speeds since startup, written to the log file.

## How to use it

- **Drag** it anywhere you like. The position is remembered between runs.
- **Double-click** to run a speedtest.
- **Right-click** for a menu: run a speedtest, see session totals, change opacity, toggle auto-hide, reset the position, hide or quit.
- **Hover** over it and it politely hides itself until you move the mouse away, if you turn on "Auto-hide on hover".
- **Hotkey** Ctrl+Shift+Alt+N shows or hides the widget from anywhere, even when it has no focus. Pick your own combo with right-click > "Change hotkey..." (also in the tray menu). If the combo is already taken by another app or reserved by Windows, the widget says so and keeps the old one.
- The tray icon offers the same actions, plus a live speed readout in its tooltip.

## Speedtest engine

Speedtests are handled by a small chain, tried in order:

1. A bundled Node.js runtime plus fast-cli (fast.com) packed next to the app.
2. fast-cli or the `fast` command on your PATH, if you have it.
3. The speedtest-cli Python library.
4. As a last resort, a passive estimate from observed traffic.

An automatic speedtest runs every 4 hours so the number you see is never stale.

## Performance

The old version updated from a worker thread, spawned a `ping.exe` every second and redrew the whole graph each tick. This rewrite fixes that:

- All UI work happens on the Tk main thread, polling background samplers every 250 ms.
- Throughput is read from psutil counters, and latency comes from a lightweight TCP connect. No per-second subprocesses.
- The heavy speedtest library loads lazily, only when a test actually runs, so startup stays instant.

## Install and run

```powershell
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt
.venv\Scripts\python app.py
```

## Build the standalone app

```powershell
.\build.ps1
```

This produces a single folder under `dist/` with the exe, the icon, the bundled Node runtime and fast-cli. Double-click the exe and the widget appears.

## Development

Linting and formatting use [Ruff](https://docs.astral.sh/ruff/), configured in `ruff.toml`. Install the dev tools once:

```powershell
.venv\Scripts\pip install -r requirements-dev.txt
```

Run both checks (same commands CI runs):

```powershell
.\lint.ps1
```

Or call the tools directly:

```powershell
.venv\Scripts\ruff check .          # lint
.venv\Scripts\ruff check --fix .    # lint and apply safe fixes
.venv\Scripts\ruff format .         # format in place
.venv\Scripts\ruff format --check . # verify formatting only
```

`ruff check .` and `ruff format --check .` must both exit clean before a commit. CI runs them in `.github/workflows/ci.yml` and again before every release build.

## Project layout

- `app.py` - the widget itself: window, labels, graph, menus, hotkeys.
- `utils/sampler.py` - reads network counters once a second and keeps history.
- `utils/latency.py` - probes latency in the background.
- `utils/speedtest.py` - the speedtest provider chain.
- `utils/config.py` - small persistent settings in `%APPDATA%\NetSpeedWidget\config.json`.
- `utils/paths.py` - resolves resource paths for source runs and PyInstaller builds.
- `utils/logger.py` - timestamped logging to console and a log file.
- `tray/container.py` - the system tray icon and menu.
- `build.ps1` and `build.py` - packaging helpers.

---

## Tech stack

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup instructions and conventions. Please follow Conventional Commits and make sure `pnpm typecheck` passes before opening a pull request.


## Security

Found a vulnerability? Please do not open a public issue. See [SECURITY.md](SECURITY.md) for how to report it privately.

## License

Released under the [MIT License](LICENSE).