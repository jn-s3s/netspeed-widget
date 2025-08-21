import shutil
import os
import glob

def exec():
    try:
        print("🧹 Cleaning build folders...")
        shutil.rmtree("build", ignore_errors=True)
        shutil.rmtree("dist", ignore_errors=True)
        for file in glob.glob("*.spec"):
            os.remove(file)
    except Exception:
        pass

if __name__ == "__main__":
    exec()