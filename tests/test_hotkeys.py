"""Tests for hotkey combo parsing, formatting and Tk event mapping."""

from types import SimpleNamespace

import pytest
import win32con

from utils.hotkeys import combo_from_tk_event, format_hotkey, parse_hotkey


def test_parse_ctrl_shift_n() -> None:
    modifiers, vk = parse_hotkey("ctrl+shift+n")
    assert modifiers == win32con.MOD_CONTROL | win32con.MOD_SHIFT
    assert vk == ord("N")


def test_parse_tolerates_case_and_whitespace() -> None:
    modifiers, vk = parse_hotkey("  CTRL+Alt+F5  ")
    assert modifiers == win32con.MOD_CONTROL | win32con.MOD_ALT
    assert vk == win32con.VK_F5


def test_parse_accepts_alt_only_combo() -> None:
    modifiers, vk = parse_hotkey("alt+space")
    assert modifiers == win32con.MOD_ALT
    assert vk == win32con.VK_SPACE


@pytest.mark.parametrize("key", [f"f{n}" for n in range(1, 13)])
def test_parse_function_keys(key: str) -> None:
    _, vk = parse_hotkey(f"ctrl+{key}")
    assert vk == win32con.VK_F1 + int(key[1:]) - 1


@pytest.mark.parametrize(
    "combo",
    [
        "",
        "ctrl",
        "n",
        "ctrl+ctrl+x",
        "ctrl+x+y",
        "shift+space",
        "ctrl+bogus",
        "ctrl+f13",
    ],
)
def test_parse_rejects_malformed_or_unsafe_combos(combo: str) -> None:
    with pytest.raises(ValueError):
        parse_hotkey(combo)


@pytest.mark.parametrize(
    ("combo", "display"),
    [
        ("ctrl+shift+n", "Ctrl+Shift+N"),
        ("alt+space", "Alt+Space"),
        ("ctrl+f12", "Ctrl+F12"),
        ("ctrl+alt+pagedown", "Ctrl+Alt+Pagedown"),
        ("alt+shift+ctrl+x", "Ctrl+Shift+Alt+X"),
    ],
)
def test_format_hotkey_canonicalizes(combo: str, display: str) -> None:
    assert format_hotkey(combo) == display


def _tk_event(keysym: str, state: int) -> SimpleNamespace:
    return SimpleNamespace(keysym=keysym, state=state)


def test_combo_from_plain_key_event() -> None:
    assert combo_from_tk_event(_tk_event("n", 0x0004)) == "ctrl+n"


def test_combo_combines_all_modifiers() -> None:
    state = 0x0001 | 0x0004 | 0x0008
    assert combo_from_tk_event(_tk_event("N", state)) == "ctrl+shift+alt+n"


def test_combo_maps_return_alias() -> None:
    assert combo_from_tk_event(_tk_event("Return", 0x0004)) == "ctrl+enter"


def test_combo_maps_shifted_digit_keysyms() -> None:
    assert combo_from_tk_event(_tk_event("exclam", 0x0004 | 0x0001)) == "ctrl+shift+1"


def test_combo_ignores_modifier_only_presses() -> None:
    assert combo_from_tk_event(_tk_event("Shift_L", 0x0001)) is None


def test_combo_raises_for_unmappable_keys() -> None:
    with pytest.raises(ValueError, match="unsupported key"):
        combo_from_tk_event(_tk_event("F13", 0x0004))


def test_combo_raises_for_empty_keysym() -> None:
    with pytest.raises(ValueError, match="unsupported key"):
        combo_from_tk_event(_tk_event("", 0))
