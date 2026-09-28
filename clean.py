"""Remove PyInstaller build artifacts.

Imported by build.py, which cleans before packaging so a failed build cannot
leave the previous exe in dist for the release step to publish anyway.
"""

import shutil
from pathlib import Path

ARTIFACTS = ("build", "dist")

ROOT = Path(__file__).resolve().parent


def run() -> None:
    """Delete the build and dist folders and any leftover .spec file.

    Failures are reported, not raised: an artifact locked by another process
    is not a reason to abandon a build.
    """
    print("🧹 Cleaning build folders...")
    for name in ARTIFACTS:
        shutil.rmtree(ROOT / name, ignore_errors=True)
    for spec in ROOT.glob("*.spec"):
        try:
            spec.unlink()
        except OSError as err:
            print(f"⚠️ Could not remove {spec.name}: {err}")


if __name__ == "__main__":
    run()
