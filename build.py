import subprocess
import sys
import io
import clean

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

# Cleanup
clean.exec()

print("⚙️ Building NetSpeed Widget.exe...")
command = [
    sys.executable, "-m", "PyInstaller",
    "--onefile",
    "--noconsole",
    "--noconfirm",
    "--name", "NetSpeed Widget",
    "app.py"
]

try:
    subprocess.run(command, check=True)
    print("✅ Build successful!")
except subprocess.CalledProcessError as e:
    print("❌ Build failed:", e)