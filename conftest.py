"""Shared pytest setup: importable project root, and hermetic app fixtures.

Two guarantees the suite depends on:

* Nothing may touch the real user profile. The autouse `isolated_appdata`
  fixture repoints APPDATA at a temp directory, so `utils.paths`, the config
  module and the log file all land under `tmp_path` instead of
  `%APPDATA%\\NetSpeedWidget`, which a test run used to append to for real.
* `NetSpeedWidget` is built against recorded fakes rather than a live Tk
  window, so an assertion can inspect what the widget actually did to its
  window and canvas instead of what a mock was told to return.
"""

import sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import app as app_module
from utils import monitors


@pytest.fixture(autouse=True)
def isolated_appdata(tmp_path, monkeypatch):
    """Point all config and log writes at a per-test temp directory."""
    root = tmp_path / "appdata"
    root.mkdir()
    monkeypatch.setenv("APPDATA", str(root))
    return root


class FakeVar:
    """Stands in for tk.BooleanVar, which needs a real Tk root."""

    def __init__(self, value=None):
        self._value = value

    def get(self):
        return self._value

    def set(self, value):
        self._value = value


class FakeCanvas:
    """Records canvas mutations so a test can read back what was drawn."""

    def __init__(self, *_args, **_kwargs):
        self.items: dict[int, dict] = {}
        self._next_id = 1
        self.deleted_tags: list[str] = []
        self.handlers: dict[str, object] = {}

    def _new_item(self, kind: str, points, **kwargs) -> int:
        item_id = self._next_id
        self._next_id += 1
        self.items[item_id] = {"kind": kind, "points": list(points), **kwargs}
        return item_id

    def create_polygon(self, *points, **kwargs) -> int:
        return self._new_item("polygon", points, **kwargs)

    def create_rectangle(self, *args, **kwargs) -> int:
        return self._new_item("rectangle", args, **kwargs)

    def create_oval(self, *args, **kwargs) -> int:
        return self._new_item("oval", args, **kwargs)

    def create_line(self, *points, **kwargs) -> int:
        return self._new_item("line", points, **kwargs)

    def create_text(self, *args, **kwargs) -> int:
        return self._new_item("text", args, **kwargs)

    def itemconfig(self, item_id: int, **kwargs) -> None:
        self.items.setdefault(item_id, {"kind": "unknown"}).update(kwargs)

    def delete(self, tag: str) -> None:
        self.deleted_tags.append(tag)

    def pack(self, **_kwargs) -> None:
        return None

    def bind(self, sequence: str, handler) -> None:
        self.handlers[sequence] = handler

    def text_of(self, item_id: int) -> str:
        """Text currently drawn on an item."""
        return self.items[item_id].get("text", "")


class FakeRoot:
    """Records the window and scheduling calls the widget makes."""

    def __init__(self, *_args, **_kwargs):
        self.calls: list[tuple[str, object]] = []
        self.after_queue: list[tuple[int, object]] = []
        self.destroyed = False
        self._state = "normal"
        self._geometry = ""
        self.position = (0, 0)

    def _record(self, name: str, arg=None) -> None:
        self.calls.append((name, arg))

    def states(self, name: str) -> int:
        """How many times a named window call was made."""
        return sum(1 for call, _ in self.calls if call == name)

    def state(self) -> str:
        return self._state

    def withdraw(self) -> None:
        self._record("withdraw")
        self._state = "withdrawn"

    def deiconify(self) -> None:
        self._record("deiconify")
        self._state = "normal"

    def title(self, text=None) -> str | None:
        self._record("title", text)
        return text

    def configure(self, **kwargs) -> None:
        self._record("configure", kwargs)

    def protocol(self, name: str, handler) -> None:
        self._record("protocol", (name, handler))

    def attributes(self, *args) -> object:
        self._record("attributes", args)
        return None

    def overrideredirect(self, flag: bool) -> None:
        self._record("overrideredirect", flag)

    def iconbitmap(self, path=None) -> None:
        self._record("iconbitmap", path)

    def geometry(self, spec=None) -> str:
        if spec is not None:
            self._record("geometry", spec)
            self._geometry = spec
        return self._geometry

    def winfo_x(self) -> int:
        return self.position[0]

    def winfo_y(self) -> int:
        return self.position[1]

    def bind(self, sequence: str, handler) -> None:
        self._record("bind", (sequence, handler))

    def after(self, delay_ms: int, func) -> int:
        self.after_queue.append((delay_ms, func))
        return len(self.after_queue)

    def destroy(self) -> None:
        self._record("destroy")
        self.destroyed = True
        self.after_queue.clear()

    def run_after(self, *, drop: set[int] | None = None) -> int:
        """Invoke every queued `after` callback once, oldest first.

        Returns how many ran, so a test can assert the loop rescheduled
        itself instead of only that the body executed.
        """
        pending, self.after_queue = self.after_queue, []
        count = 0
        for index, (_delay, func) in enumerate(pending):
            if drop and index in drop:
                continue
            func()
            count += 1
        return count


@pytest.fixture
def fake_gui(monkeypatch):
    """Replace Tk surface the widget constructs with recording fakes."""
    root = FakeRoot()
    canvas = FakeCanvas()
    menu = MagicMock()
    menu.index.return_value = 1
    font = MagicMock()

    monkeypatch.setattr(app_module.tk, "Canvas", lambda *a, **k: canvas)
    monkeypatch.setattr(app_module.tk, "Menu", lambda *a, **k: menu)
    monkeypatch.setattr(app_module.tk, "BooleanVar", FakeVar)
    monkeypatch.setattr(app_module.tkfont, "Font", lambda *a, **k: font)
    monkeypatch.setattr(app_module, "LatencyProbe", MagicMock())
    return SimpleNamespace(root=root, canvas=canvas, menu=menu, font=font)


PRIMARY_SCREEN = [(0, 0, 1920, 1080)]
OFF_SCREEN = [(0, 0, 800, 600)]


@pytest.fixture
def make_widget(fake_gui, monkeypatch, request):
    """Build a NetSpeedWidget on fakes, with the background services stubbed.

    Monitors are stubbed to one primary screen by default, so a test that
    exercises the off-monitor rescue overrides them explicitly instead of
    depending on the machine running the suite. Every widget is shut down
    with the test, which stops the scheduler thread it starts.
    """

    def _make(
        *,
        position: tuple[int, int] | None = (500, 300),
        rects: list[tuple[int, int, int, int]] | None = None,
    ):
        monkeypatch.setattr(app_module, "get_position", lambda: position)
        monkeypatch.setattr(
            monitors, "active_monitor_rects", lambda: rects or PRIMARY_SCREEN
        )

        hotkey = MagicMock()
        hotkey.apply_combo.return_value = (True, None)
        monkeypatch.setattr(app_module, "GlobalHotkey", lambda _cb: hotkey)

        sampler = MagicMock()
        sampler.latest = None
        sampler.history = []
        sampler.session_totals = (0.0, 0.0)
        monkeypatch.setattr(app_module, "NetSampler", lambda **_kw: sampler)

        widget = app_module.NetSpeedWidget(fake_gui.root)
        request.addfinalizer(widget.shutdown)
        probe = MagicMock()
        probe.latest.ok = True
        probe.latest.ms = 20.0
        widget.probe = probe
        return widget

    return _make
