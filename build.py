import io
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import clean

# Ensure stdout uses UTF-8 encoding for consistent emoji/log output
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

ROOT = Path(__file__).resolve().parent
TP = ROOT / "third_party"
NODE_EXE = "node.exe"
NODE_DEST = TP / "node" / NODE_EXE
FAST_BUNDLE = TP / "fast-bundle"
FAST_PACKAGE_JSON = FAST_BUNDLE / "package.json"
FAST_CLI_VERSION = os.environ.get("FAST_CLI_VERSION", "latest")

# Release workflows pass APP_VERSION (the git tag); local builds stay unversioned.
APP_VERSION = os.environ.get("APP_VERSION", "").strip().lstrip("v")
EXE_NAME = f"NetSpeedWidget-{APP_VERSION}" if APP_VERSION else "NetSpeedWidget"


def main() -> None:
    """
    Build script for packaging the NetSpeed Widget application into a standalone
    Windows executable using PyInstaller.
    """

    # 1. Clean up previous build artifacts
    clean.exec()

    # 2. Install required dependencies from requirements.txt
    print("📦 Installing requirements...")
    subprocess.run(
        [sys.executable, "-m", "pip", "install", "-r", "requirements.txt"], check=True
    )

    # 3. Run PyInstaller to create the EXE
    print(f"⚙️ Building {EXE_NAME}.exe...")
    command = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--onefile",  # Bundle into a single EXE
        "--noconsole",  # Hide console window
        "--noconfirm",  # Overwrite existing build
        "--icon",
        "icon.ico",  # Set application icon
        "--add-data",
        "icon.ico;.",  # Include icon resource in bundle
        "--name",
        EXE_NAME,  # Set application name (versioned on release builds)
        "app.py",  # Entry point
        "--add-binary",
        f"{NODE_DEST};third_party/node",  # Binary for node.exe
        "--add-data",
        f"{FAST_BUNDLE};third_party/fast-bundle",  # Data for the fast-bundle folder
        "--hidden-import=win32api",
        "--hidden-import=win32con",
        "--hidden-import=pywintypes",
        "--hidden-import=pythoncom",  # win32api
    ]

    try:
        subprocess.run(command, check=True)
        print("✅ Build successful!")
    except subprocess.CalledProcessError as e:
        print("❌ Build failed:", e)
        # Non-zero exit so the release workflow aborts instead of publishing a stale exe.
        sys.exit(1)


def ensure_node_runtime() -> None:
    """
    Copies a real node.exe into third_party/node/node.exe for bundling.
    """
    if NODE_DEST.exists():
        return
    src = _where_node()
    if not src:
        raise RuntimeError("❌ Node.js not found on this build machine.")
    NODE_DEST.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, NODE_DEST)
    print(f"✅ Node runtime: {src} -> {NODE_DEST}")


def ensure_fast_bundle() -> None:
    """
    Creates third_party/fast-bundle and installs fast-cli locally inside it.
    Users do not need Node at runtime; we ship node.exe + fast-bundle.
    """
    FAST_BUNDLE.mkdir(parents=True, exist_ok=True)
    if not FAST_PACKAGE_JSON.exists():
        FAST_PACKAGE_JSON.write_text(
            json.dumps({"name": "fast-bundle", "private": True}, indent=2)
        )

    npm = shutil.which("npm")
    if not npm:
        raise RuntimeError("⚠️ npm is required on the build machine to vendor fast-cli.")

    # Install fast-cli with production deps only
    subprocess.run(
        [
            npm,
            "install",
            f"fast-cli@{FAST_CLI_VERSION}",
            "--omit=dev",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
        ],
        cwd=str(FAST_BUNDLE),
        check=True,
    )

    cli_js = FAST_BUNDLE / "node_modules" / "fast-cli" / "distribution" / "cli.js"
    if not cli_js.exists():
        raise RuntimeError(
            "❌ fast-cli install did not produce distribution/cli.js; version mismatch?"
        )
    print(f"✅ fast-cli ready: {cli_js}")


def _where_node() -> Path | None:
    """
    Returns a real node.exe. Prefers nvm-windows install folders over PATH shims.
    """
    nvm = Path(os.environ.get("APPDATA", "")) / "nvm"
    if nvm.exists():
        versions = sorted(
            [d for d in nvm.iterdir() if d.is_dir() and (d / NODE_EXE).exists()],
            reverse=True,
        )
        if versions:
            return versions[0] / NODE_EXE
    try:
        out = subprocess.check_output(["where", "node"], text=True).strip().splitlines()
        for line in out:
            path = Path(line.strip())
            if path.name.lower() == NODE_EXE and path.is_file():
                return path
    except (OSError, subprocess.SubprocessError):
        return None
    return None


if __name__ == "__main__":
    ensure_node_runtime()
    ensure_fast_bundle()
    main()
