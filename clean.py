import shutil
import os
import glob


def exec() -> None:
    """
    Clean up build artifacts created by PyInstaller.
    """
    try:
        print("🧹 Cleaning build folders...")
        shutil.rmtree("build", ignore_errors=True)
        shutil.rmtree("dist", ignore_errors=True)

        for file in glob.glob("*.spec"):
            os.remove(file)
    except OSError as err:
        # Cleanup is best effort; locked or already-removed files are fine to skip.
        print(f"⚠️ Cleanup skipped: {err}")


if __name__ == "__main__":
    exec()
