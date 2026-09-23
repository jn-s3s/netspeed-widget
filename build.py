"""Package NetSpeed Widget into a standalone Windows executable.

Vendoring happens first: a real node.exe is copied in and the fast-cli npm
bundle is installed from the committed lockfile, so the speedtest backend
inside the exe is the same tree every build gets. PyInstaller then bundles
the app, the runtime and the bundle into one file.

Run as `python build.py`. Set APP_VERSION to the release tag, as the release
workflow does, to name the artifact after its version.
"""

import io
import os
import shutil
import subprocess
import sys
from pathlib import Path

import clean
from utils.paths import (
    FAST_BUNDLE_DIR,
    FAST_CLI_ENTRY,
    NODE_BINARY,
    NODE_DIR,
    THIRD_PARTY,
)
from utils.version import APP_VERSION

ROOT = Path(__file__).resolve().parent

# Ensure stdout uses UTF-8 encoding so the status glyphs below stay printable
# no matter which code page the shell launched us with.
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
TP = ROOT / THIRD_PARTY
NODE_DEST = TP / NODE_DIR / NODE_BINARY
FAST_BUNDLE = TP / FAST_BUNDLE_DIR
FAST_PACKAGE_JSON = FAST_BUNDLE / "package.json"
FAST_PACKAGE_LOCK = FAST_BUNDLE / "package-lock.json"

# Reviewed and locked in third_party/fast-bundle/package-lock.json. A build
# installs from that lockfile, so changing this means regenerating it in the
# same commit, where the diff shows which transitive packages moved.
FAST_CLI_VERSION = os.environ.get("FAST_CLI_VERSION", "5.2.0").strip()

# Release workflows pass APP_VERSION (the git tag); local builds stay
# unversioned.
RELEASE_TAG = os.environ.get("APP_VERSION", "").strip().lstrip("v")
EXE_NAME = f"NetSpeedWidget-{RELEASE_TAG}" if RELEASE_TAG else "NetSpeedWidget"


def check_release_version() -> None:
    """Abort a tagged build whose tag disagrees with the declared version.

    The version string appears in the window title, the tray tooltip and the
    log, while the tag decides the released filename. Letting them differ
    makes every bug report that quotes one wrong about the other.
    """
    if RELEASE_TAG and RELEASE_TAG != APP_VERSION:
        raise RuntimeError(
            f"release tag v{RELEASE_TAG} does not match utils/version.APP_VERSION "
            f"({APP_VERSION}); bump the constant and re-tag"
        )


def main() -> None:
    """Build the standalone exe, aborting non-zero on any failed step."""
    check_release_version()

    clean.run()

    print("📦 Installing requirements...")
    subprocess.run(
        [sys.executable, "-m", "pip", "install", "-r", "requirements.txt"], check=True
    )

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
        f"{NODE_DEST};{THIRD_PARTY}/{NODE_DIR}",  # Binary for node.exe
        "--add-data",
        f"{FAST_BUNDLE};{THIRD_PARTY}/{FAST_BUNDLE_DIR}",  # Data for the bundle
        "--hidden-import=win32api",
        "--hidden-import=win32con",
        "--hidden-import=pywintypes",
        "--hidden-import=pythoncom",  # win32api
    ]

    try:
        subprocess.run(command, check=True)
        print("✅ Build successful!")
    except subprocess.CalledProcessError as err:
        # Non-zero exit so the release workflow aborts rather than publishing
        # whatever exe an earlier run left in dist.
        print("❌ Build failed:", err)
        raise SystemExit(1) from err


def ensure_node_runtime() -> None:
    """Copy a real node.exe into third_party/node for bundling.

    Raises:
        RuntimeError: No Node installation was found on this machine.
    """
    if NODE_DEST.exists():
        return
    source = _where_node()
    if not source:
        raise RuntimeError("❌ Node.js not found on this build machine.")
    NODE_DEST.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, NODE_DEST)
    print(f"✅ Node runtime: {source} -> {NODE_DEST}")


def ensure_fast_bundle() -> None:
    """Install fast-cli inside third_party/fast-bundle from the lockfile.

    Users need no Node at runtime: the exe ships node.exe plus this bundle.

    Raises:
        RuntimeError: npm is missing, the lockfile is absent, or the install
            did not produce the entry script utils/speedtest.py expects.
    """
    if not FAST_PACKAGE_LOCK.exists():
        raise RuntimeError(
            f"missing {FAST_PACKAGE_LOCK.relative_to(ROOT)}; fast-cli is only "
            "installed from a committed lockfile so a release cannot pick up "
            "an unreviewed transitive package"
        )

    npm = shutil.which("npm")
    if not npm:
        raise RuntimeError("❌ npm is required on the build machine to vendor fast-cli")

    # `npm ci` installs exactly what the lockfile pins and replaces any
    # existing tree, rather than resolving package.json ranges again.
    subprocess.run(
        [npm, "ci", "--omit=dev", "--no-audit", "--no-fund", "--loglevel=error"],
        cwd=str(FAST_BUNDLE),
        check=True,
    )

    cli_js = FAST_BUNDLE.joinpath(*FAST_CLI_ENTRY)
    if not cli_js.exists():
        raise RuntimeError(
            f"❌ fast-cli install did not produce {cli_js.name}; "
            f"does package-lock.json still hold {FAST_CLI_VERSION}?"
        )
    print(f"✅ fast-cli ready: {cli_js}")


def _where_node() -> Path | None:
    """Return a real node.exe, preferring nvm-windows folders over PATH shims."""
    nvm = Path(os.environ.get("APPDATA", "")) / "nvm"
    if nvm.exists():
        versions = sorted(
            [d for d in nvm.iterdir() if d.is_dir() and (d / NODE_BINARY).exists()],
            reverse=True,
        )
        if versions:
            return versions[0] / NODE_BINARY
    try:
        lines = (
            subprocess.check_output(["where", "node"], text=True).strip().splitlines()
        )
    except (OSError, subprocess.SubprocessError) as err:
        print(f"⚠️ Could not query PATH for node: {err}")
        return None
    for line in lines:
        path = Path(line.strip())
        if path.name.lower() == NODE_BINARY and path.is_file():
            return path
    return None


if __name__ == "__main__":
    ensure_node_runtime()
    ensure_fast_bundle()
    main()
