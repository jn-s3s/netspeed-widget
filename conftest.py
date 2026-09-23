"""Shared pytest setup.

Keeps the repository root on sys.path so `import utils...` works no
matter which directory pytest is invoked from.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
