# NetSpeed Widget

A lightweight floating network speed monitor for **Windows**.
It displays real-time **download/upload speeds** (Mbps) with a compact graph that stays on top of other windows.

Built with **Python, Tkinter, psutil, and PyInstaller**.

---

## ✨ Features

- 📡 Real-time network monitoring (download + upload Mbps)
- 📊 Mini graph for last ~10 seconds of activity
- 👀 Auto-hide on hover (disappears when cursor enters, reappears when it leaves)
- ⌨️ Hotkeys for opacity:
  - `Ctrl + Shift + Alt + Up` → Increase opacity
  - `Ctrl + Shift + Alt + Down` → Decrease opacity
  - `Ctrl + Shift + Alt + Left` → Reset opacity
- 🛠 System tray integration (Show / Hide / Quit)
- 💾 Saves opacity setting
- 🪟 Windows-only

---

## 📥 Download

You don't need to build it yourself if you just want to use it.
A prebuilt **`.exe`** file is available in the **[Latest Release](../../releases/latest)** section of this repository.

---
## 📦 Requirements

- Python 3.10+ on Windows
- Dependencies (install with `pip install -r requirements.txt`):
  - psutil
  - pystray
  - pyinstaller (for building exe)

---

## ▶️ Run from source

```bash
pip install -r requirements.txt
python app.py
```

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
