import subprocess
import sys
import io
import clean

# Ensure stdout uses UTF-8 encoding for consistent emoji/log output
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")


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
        [sys.executable, "-m", "pip", "install", "-r", "requirements.txt"],
        check=True
    )

    # 3. Run PyInstaller to create the EXE
    print("⚙️ Building NetSpeed Widget.exe...")
    command = [
        sys.executable, "-m", "PyInstaller",
        "--onefile",                 # Bundle into a single EXE
        "--noconsole",               # Hide console window
        "--noconfirm",               # Overwrite existing build
        "--icon", "icon.ico",        # Set application icon
        "--add-data", "icon.ico;.",  # Include icon resource in bundle
        "--name", "NetSpeed Widget", # Set application name
        "app.py"                     # Entry point
    ]

    try:
        subprocess.run(command, check=True)
        print("✅ Build successful!")
    except subprocess.CalledProcessError as e:
        print("❌ Build failed:", e)


if __name__ == "__main__":
    main()
