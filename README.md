# NetSpeed Widget

A lightweight floating network speed monitor for **Windows**.
It displays real-time **download/upload speeds** (Mbps) with a compact graph that stays on top of other windows.

Built with **Python, Tkinter, psutil, and PyInstaller**.

[![📦 Build release executable file](https://github.com/jn-s3s/netspeed-widget/actions/workflows/build-release.yml/badge.svg)](https://github.com/jn-s3s/netspeed-widget/actions/workflows/build-release.yml)

## ✨ Features

- 📡 Real-time network monitoring (download + upload Mbps)
- 📊 Mini graph for last ~10 seconds of activity
- 👀 Auto-hide on hover (disappears when cursor enters, reappears when it leaves)
- ⌨️ Hotkeys for opacity:
  - `Ctrl + Shift + Alt + Up` → Increase opacity
  - `Ctrl + Shift + Alt + Down` → Decrease opacity
  - `Ctrl + Shift + Alt + Left` → Reset opacity
- 🛠 System tray integration (Show / Hide / Quit)
- 🎛️ Tray settings for opacity
- ⏱️ Periodic speedtest (default every ~4 hours) with fallbacks
- 💾 Settings persist across app restarts (including opacity and speedtest state)
- 🪟 Windows-only

---

## 📥 Download

You don't need to build it yourself if you just want to use it.
A prebuilt **`.exe`** is available in the **[Latest Release](../../releases/latest)** of this repository.

---

## ⚙️ Requirements

- Windows 10/11 with Python 3.10+ (for running from source)
- Install deps:
  ```bash
  pip install -r requirements.txt
  ```
- Optional (improves speedtest accuracy/availability):
  - **fast-cli** (Node): `npm i -g fast-cli`
  - **speedtest-cli** (Python): `pip install speedtest-cli`

> Note: The app will gracefully fall back if a tool isn't present.

---

## ▶️ Run from source

```bash
pip install -r requirements.txt
python app.py
```

---

## 🧪 How speedtest works (quick overview)

1) Try **fast-cli**
2) If unavailable, try **speedtest-cli**
3) If still unavailable, estimate via **psutil** net I/O deltas

Results are cached so the **previous speedtest** is shown on startup until the next scheduled run completes.

Default schedule: **every ~4 hours** while the app is running.

---

## ⚙️ Build a standalone EXE

```bash
python build.py
```

This will:
- Clean previous builds
- Install dependencies
- Create `dist/NetSpeedWidget.exe`

---

## 📜 License

[MIT License © 2025 jn-s3s](LICENSE)
