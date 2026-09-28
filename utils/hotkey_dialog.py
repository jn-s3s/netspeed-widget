"""Capture dialog that rebinds the global show/hide hotkey."""

import tkinter as tk
from collections.abc import Callable

from utils.hotkeys import combo_from_tk_event, format_hotkey
from utils.logger import get
from utils.theme import (
    FONT_FAMILY,
    Palette,
)

PADDING_X = 16
PADDING_Y = 14
CLOSE_DELAY_MS = 700

_log = get("hotkey")


def show(
    parent: tk.Misc,
    current: str,
    apply_combo: Callable[[str], tuple[bool, str | None]],
    palette: Palette,
) -> tk.Toplevel:
    """Open a window that binds whatever combo the user presses next.

    The dialog stays open on a rejected or unsupported combo so the user can
    try again, and closes on a bound one. Esc cancels. Modifier-only presses
    are treated as waiting, not as an attempt.

    Args:
        parent: Tk widget the dialog is built under.
        current: Combo shown as the current binding.
        apply_combo: Validates, registers and persists a combo, returning
            whether it took and the reason it did not.
        palette: Active theme palette.

    Returns:
        The dialog window, so the caller can keep one instance alive.
    """
    bg = palette.surface
    fg = palette.fg
    fg_dim = palette.fg_dim
    good = palette.good
    bad = palette.bad

    top = tk.Toplevel(parent)
    top.title("Set hotkey")
    top.attributes("-topmost", True)
    top.configure(bg=bg, padx=PADDING_X, pady=PADDING_Y)
    top.resizable(False, False)

    tk.Label(
        top,
        text="Press your new show/hide hotkey.",
        font=(FONT_FAMILY, 9, "bold"),
        fg=fg,
        bg=bg,
    ).pack(anchor="w")
    tk.Label(
        top,
        text="Must include Ctrl or Alt. Esc cancels.",
        font=(FONT_FAMILY, 8),
        fg=fg_dim,
        bg=bg,
    ).pack(anchor="w", pady=(4, 0))
    status = tk.Label(
        top,
        text=f"Current: {format_hotkey(current)}",
        font=(FONT_FAMILY, 8),
        fg=fg_dim,
        bg=bg,
    )
    status.pack(anchor="w", pady=(8, 0))

    def on_key(event: tk.Event) -> None:
        if event.keysym == "Escape":
            top.destroy()
            return
        try:
            combo = combo_from_tk_event(event)
        except ValueError as err:
            status.config(text=str(err), fg=bad)
            return
        if combo is None:
            return  # documented contract: modifier-only press, keep listening

        ok, error = apply_combo(combo)
        if not ok:
            _log.warning(f"'{combo}' rejected while capturing: {error}")
            status.config(text=f"Unavailable: {error}", fg=bad)
            return
        status.config(text=f"Hotkey set: {format_hotkey(combo)}", fg=good)
        top.after(CLOSE_DELAY_MS, top.destroy)

    top.bind("<KeyPress>", on_key)
    top.focus_force()
    return top
