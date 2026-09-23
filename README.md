# NetSpeed Widget

<p align="center">
  <img src="icon.ico" width="96" alt="NetSpeed Widget icon">
</p>

A tiny always-on-top widget for Windows that shows your live network speed, ping and a rolling traffic graph. It sits in a corner of your screen, stays out of the way and gives you a one-click speedtest when you need it.


[![CI](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/ci.yml)
[![Release](https://github.com/jn-s3s/netspeed-widget/actions/workflows/release.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)


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
4. As a last resort, an estimate from the traffic already crossing the wire.

The first three measure your line. The fourth only watches what happens to be
moving, so it is shown as an "estimate" instead of a speedtest result, it is
not saved as your last result, and the app retries a real measurement sooner
than the usual four hours. If an idle connection is all it can see, it reports
nothing rather than a believable zero.

An automatic speedtest runs every 4 hours so the number you see is never stale.

## Performance

- All UI work happens on the Tk main thread, polling background samplers every 250 ms.
- Throughput is read from psutil counters, and latency comes from a lightweight TCP connect. No per-second subprocesses.
- The heavy speedtest library loads lazily, only when a test actually runs, so startup stays instant.

## Install and run

```powershell
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt
.venv\Scripts\python app.py
```

`requirements.txt` is a fully pinned, hash-checked lock covering CPython 3.11
and 3.12 on 64-bit Windows. On another interpreter or platform pip stops with
a missing-hash error rather than installing something the file never approved;
the regeneration command is documented at the top of that file.

## Build the standalone app

```powershell
.venv\Scripts\python build.py
```

The build machine needs Node.js and npm. `build.py` vendors a real `node.exe`
and installs fast-cli with `npm ci`, which resolves nothing new: it installs
exactly the tree recorded in the committed `third_party/fast-bundle/package-lock.json`.
Bumping fast-cli means changing `FAST_CLI_VERSION`, regenerating that lockfile
and reviewing which transitive packages moved, all in one commit.

The output is a single `dist/NetSpeedWidget.exe` with the icon, the Node runtime
and fast-cli embedded. Release builds set the `APP_VERSION` environment variable
(CI passes the git tag), so release artifacts are named
`dist/NetSpeedWidget-<version>.exe`. A tagged build whose tag differs from
`utils/version.APP_VERSION` is refused, because that constant is what the
window title, the tray tooltip and the log report. Double-click the exe and the
widget appears.

## Development

Linting and formatting use [Ruff](https://docs.astral.sh/ruff/), configured in `ruff.toml`. Install the dev tools once:

```powershell
.venv\Scripts\pip install -r requirements.txt
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

The test suite under `tests/` uses pytest. An autouse fixture repoints
`APPDATA` at a temp directory, so no run ever touches your real
`%APPDATA%\NetSpeedWidget` settings or log. It covers hotkey parsing,
formatting and the register-before-release swap, the speedtest provider order,
JSON parsing and the passive estimate's idle and counter-reset limits, sampler
rate math and thread lifecycle (including counter-reset and zero-elapsed edge
cases), monitor visibility and the off-screen rescue, speedtest scheduling,
graph scaling, config load, save, locking and validation, log levels and
rollover, path resolution, tray menu routing and the widget's drag, hover,
opacity, tick and shutdown behavior against a recorded fake window. Run it
locally:

```powershell
.venv\Scripts\python -m pytest tests -q
```

CI runs the same command on every push and pull request, and again before every release build.

## Project layout

- `app.py` - the widget itself: window, text rows, menus, interaction and the tick loop.
- `utils/sampler.py` - reads network counters once a second and keeps history.
- `utils/latency.py` - probes latency in the background.
- `utils/hotkeys.py` - registers the global show/hide hotkey and listens for it.
- `utils/hotkey_dialog.py` - the capture window used to rebind that hotkey.
- `utils/speedtest.py` - the speedtest provider chain.
- `utils/schedule.py` - decides when the next automatic speedtest is due.
- `utils/graph.py` - the rolling traffic plot and its peak scaling.
- `utils/monitors.py` - display geometry, so the widget can tell when it is stranded.
- `utils/theme.py` - the palette and the rounded pill shape.
- `utils/config.py` - small persistent settings in `%APPDATA%\NetSpeedWidget\config.json`.
- `utils/paths.py` - resolves resource and state paths for source runs and PyInstaller builds.
- `utils/logger.py` - levelled, subsystem-tagged logging with size-based rollover.
- `utils/version.py` - the one place the app version is written.
- `utils/format.py` - shared number formatting.
- `tray/container.py` - the system tray icon and menu.
- `build.py` and `clean.py` - packaging helpers.
- `lint.ps1` - runs the Ruff lint and format checks.
- `tests/` - pytest unit tests; see Development above.
- `conftest.py` - puts the repo root on the import path and provides the isolated, fake-window fixtures.

## Tech stack

- Python 3.11, with Tkinter (the standard library GUI toolkit) for the widget UI.
- pystray and Pillow for the tray icon, pywin32 for the global hotkey and Win32 integration.
- psutil for network counters, speedtest-cli as one of the speedtest providers.
- A bundled Node.js runtime with fast-cli as the primary speedtest provider.
- PyInstaller builds the exe, Ruff lints and formats, GitHub Actions runs CI and releases.

---

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup instructions and conventions, and make sure `.\lint.ps1` (or `ruff check .` and `ruff format --check .`) passes before opening a pull request.

## Security

Found a vulnerability? Please do not open a public issue. See [SECURITY.md](SECURITY.md) for how to report it privately.

## License

Released under the [MIT License](LICENSE).