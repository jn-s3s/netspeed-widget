"""The application version, declared once for the UI, the log and the build.

build.py compares this against the APP_VERSION value a release workflow
derives from the git tag and aborts on a mismatch, so the title, the tray
tooltip, the log banner and the released filename cannot disagree.
"""

APP_VERSION = "1.1.1"
AUTHOR = "jn-s3s"
APP_NAME = f"NetSpeed Widget v{APP_VERSION} by {AUTHOR}"
