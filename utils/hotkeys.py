"""Global hotkey support for NetSpeed Widget.

Registers one system-wide hotkey via RegisterHotKey so it fires even
when the widget has no focus. A hidden message-only window on a daemon
thread owns the registration and receives WM_HOTKEY; register and
unregister requests from the Tk thread are marshaled over PostMessage.

Conflict safety: RegisterHotKey fails loudly when a combo is already
owned by another process or reserved by Windows, so apply_combo can
report "taken, pick another" instead of silently stealing keys.
"""

import re
import threading
from collections.abc import Callable
from typing import Any

import pywintypes
import win32api
import win32con
import win32gui

from utils.logger import warn

_WM_APPLY = win32con.WM_USER + 1

# win32con does not define HWND_MESSAGE (-3); it makes the sink
# window message-only so it never appears or steals focus.
_HWND_MESSAGE = -3

_MOD_FLAGS = {
    "ctrl": win32con.MOD_CONTROL,
    "shift": win32con.MOD_SHIFT,
    "alt": win32con.MOD_ALT,
}
_MOD_ORDER = ("ctrl", "shift", "alt")

_EXTRA_KEYS = {
    "space": win32con.VK_SPACE,
    "tab": win32con.VK_TAB,
    "enter": win32con.VK_RETURN,
    "backspace": win32con.VK_BACK,
    "delete": win32con.VK_DELETE,
    "insert": win32con.VK_INSERT,
    "home": win32con.VK_HOME,
    "end": win32con.VK_END,
    "pageup": win32con.VK_PRIOR,
    "pagedown": win32con.VK_NEXT,
    "up": win32con.VK_UP,
    "down": win32con.VK_DOWN,
    "left": win32con.VK_LEFT,
    "right": win32con.VK_RIGHT,
}

# Tk reports modifier-only presses with these keysyms; they mean
# "keep waiting" rather than "this is the combo".
_IGNORE_KEYSYMS = {
    "shift_l",
    "shift_r",
    "control_l",
    "control_r",
    "alt_l",
    "alt_r",
    "iso_level3_shift",
    "caps_lock",
    "num_lock",
    "scroll_lock",
    "meta_l",
    "meta_r",
    "super_l",
    "super_r",
}

# Tk keysyms that differ from our canonical key names.
_TK_KEY_ALIASES = {"return": "enter", "prior": "pageup", "next": "pagedown"}

# On a US layout, Shift+digit reports the symbol keysym instead.
_SHIFTED_DIGITS = {
    "exclam": "1",
    "at": "2",
    "numbersign": "3",
    "dollar": "4",
    "percent": "5",
    "asciicircum": "6",
    "ampersand": "7",
    "asterisk": "8",
    "parenleft": "9",
    "parenright": "0",
}


def _key_to_vk(key: str) -> int | None:
    if len(key) == 1 and key.isalnum():
        return ord(key.upper())
    if re.fullmatch(r"f([1-9]|1[0-2])", key):
        return win32con.VK_F1 + int(key[1:]) - 1
    return _EXTRA_KEYS.get(key)


def parse_hotkey(combo: str) -> tuple[int, int]:
    """Parse "ctrl+shift+n" into (modifier flags, virtual key code).

    Raises ValueError when the combo is malformed, uses an unsupported
    key, or lacks Ctrl/Alt (bare or Shift-only combos would hijack
    normal typing in other applications).
    """
    parts = [p for p in combo.strip().lower().split("+") if p]
    mods = [p for p in parts if p in _MOD_FLAGS]
    keys = [p for p in parts if p not in _MOD_FLAGS]
    if len(keys) != 1 or len(set(mods)) != len(mods) or not mods:
        raise ValueError("expected modifiers plus one key, e.g. ctrl+shift+n")
    if "ctrl" not in mods and "alt" not in mods:
        raise ValueError("combo must include Ctrl or Alt")
    vk = _key_to_vk(keys[0])
    if vk is None:
        raise ValueError(f"unsupported key '{keys[0]}'")
    flags = 0
    for mod in mods:
        flags |= _MOD_FLAGS[mod]
    return flags, vk


def format_hotkey(combo: str) -> str:
    """Format a combo for display, e.g. "ctrl+shift+n" > "Ctrl+Shift+N"."""
    parts = [p for p in combo.strip().lower().split("+") if p]
    pretty = []
    for mod in _MOD_ORDER:
        if mod in parts:
            pretty.append(mod.capitalize())
    for part in parts:
        if part not in _MOD_FLAGS:
            pretty.append(part.upper() if len(part) == 1 else part.capitalize())
    return "+".join(pretty)


def combo_from_tk_event(event: Any) -> str | None:
    """Convert a Tk KeyPress event into a canonical combo string.

    Returns None for pure modifier presses (caller keeps listening) and
    raises ValueError for keys that cannot map to a virtual key code.
    """
    keysym = (getattr(event, "keysym", "") or "").lower()
    if keysym in _IGNORE_KEYSYMS:
        return None
    key = _TK_KEY_ALIASES.get(keysym, keysym)
    key = _SHIFTED_DIGITS.get(key, key)
    if _key_to_vk(key) is None:
        raise ValueError(f"unsupported key '{keysym}'")

    mods = []
    state = int(getattr(event, "state", 0))
    if state & 0x0004:
        mods.append("ctrl")
    if state & 0x0001:
        mods.append("shift")
    if state & 0x0008 or state & 0x0080 or state & 0x20000:
        mods.append("alt")
    return "+".join([m for m in _MOD_ORDER if m in mods] + [key])


class GlobalHotkey:
    """Owns one system-wide hotkey registration and its listener thread.

    on_trigger is invoked from the listener thread and must be safe to
    call off the Tk thread (for example, wrapped in app.ui_call).
    """

    HOTKEY_ID = 1

    def __init__(self, on_trigger: Callable[[], None]) -> None:
        self._on_trigger = on_trigger
        self._hwnd: int | None = None
        self._init_error: str | None = None
        self._pending: dict | None = None
        ready = threading.Event()
        self._thread = threading.Thread(
            target=self._run, args=(ready,), name="hotkey-listener", daemon=True
        )
        self._thread.start()
        ready.wait(timeout=2)

    def _run(self, ready: threading.Event) -> None:
        try:
            wnd_class = win32gui.WNDCLASS()
            # Unique per instance: RegisterClass fails when the name is
            # already taken in this process.
            wnd_class.lpszClassName = f"NetSpeedWidgetHotkeySink{id(self):x}"
            wnd_class.lpfnWndProc = {
                win32con.WM_HOTKEY: self._wm_hotkey,
                win32con.WM_CLOSE: self._wm_close,
                _WM_APPLY: self._wm_apply,
            }
            atom = win32gui.RegisterClass(wnd_class)
            self._hwnd = win32gui.CreateWindowEx(
                0,
                atom,
                "NetSpeedHotkeySink",
                0,
                0,
                0,
                0,
                0,
                _HWND_MESSAGE,
                0,
                win32api.GetModuleHandle(None),
                None,
            )
        except Exception as err:
            self._init_error = str(err)
        ready.set()
        if self._hwnd is not None:
            win32gui.PumpMessages()

    def apply_combo(self, combo: str) -> tuple[bool, str | None]:
        """Validate and register a combo. Returns (ok, error message)."""
        try:
            modifiers, vk = parse_hotkey(combo)
        except ValueError as err:
            return False, str(err)
        if self._hwnd is None:
            return False, self._init_error or "hotkey listener unavailable"

        done = threading.Event()
        self._pending = {
            "mod": modifiers,
            "vk": vk,
            "done": done,
            "ok": False,
            "error": None,
        }
        try:
            win32gui.PostMessage(self._hwnd, _WM_APPLY, 0, 0)
        except pywintypes.error as err:
            return False, str(err)
        if not done.wait(timeout=2):
            return False, "hotkey listener did not respond"
        return bool(self._pending["ok"]), self._pending["error"]

    def stop(self) -> None:
        """Unregister the hotkey and shut the listener thread down."""
        if self._hwnd is None:
            return
        try:
            win32gui.PostMessage(self._hwnd, win32con.WM_CLOSE, 0, 0)
        except pywintypes.error:
            pass
        self._hwnd = None

    def _wm_apply(self, hwnd: int, msg: int, wparam: int, lparam: int) -> int:
        req = self._pending
        try:
            win32gui.UnregisterHotKey(self._hwnd, self.HOTKEY_ID)
        except pywintypes.error:
            pass  # nothing registered yet
        try:
            win32gui.RegisterHotKey(
                self._hwnd,
                self.HOTKEY_ID,
                req["mod"] | win32con.MOD_NOREPEAT,
                req["vk"],
            )
            req["ok"] = True
        except pywintypes.error as err:
            req["error"] = getattr(err, "strerror", None) or str(err)
        req["done"].set()
        return 0

    def _wm_hotkey(self, hwnd: int, msg: int, wparam: int, lparam: int) -> int:
        if wparam == self.HOTKEY_ID:
            try:
                self._on_trigger()
            except Exception as err:
                warn(f"[HOTKEY] callback failed: {err}")
        return 0

    def _wm_close(self, hwnd: int, msg: int, wparam: int, lparam: int) -> int:
        try:
            win32gui.UnregisterHotKey(hwnd, self.HOTKEY_ID)
        except pywintypes.error:
            pass  # nothing registered
        win32gui.DestroyWindow(hwnd)
        win32gui.PostQuitMessage(0)
        return 0
