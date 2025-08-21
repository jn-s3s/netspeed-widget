import subprocess
import sys
import io
import clean

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

# Cleanup
clean.exec()

# Install dependencies
print("📦 Installing requirements...")
subprocess.run([sys.executable, "-m", "pip", "install", "-r", "requirements.txt"], check=True)

# Build
print("⚙️ Building NetSpeed Widget.exe...")
command = [
    sys.executable, "-m", "PyInstaller",
    "--onefile",
    "--noconsole",
    "--noconfirm",
    "--icon", "icon.ico",
    "--add-data", "icon.ico;.",
    "--name", "NetSpeed Widget",
    "app.py"
]

try:
    subprocess.run(command, check=True)
    print("✅ Build successful!")
except subprocess.CalledProcessError as e:
    print("❌ Build failed:", e)