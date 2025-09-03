from typing import Any, Optional

from utils.config import load_config, save_config
from utils.logger import info

# Default opacity settings
WINDOW_ALPHA: float = 0.72
ALPHA_STEP: float = 0.05
ALPHA_MIN: float = 0.40
ALPHA_MAX: float = 1.00


class Hotkeys:
    """
    Handles keyboard hotkeys for adjusting the app window's opacity.
    """

    def __init__(self, app: Any) -> None:
        """
        Initialize the Hotkeys manager.
        """
        self.app = app
        self.alpha: float = WINDOW_ALPHA


    def bind(self) -> None:
        """
        Bind all hotkeys and set the initial opacity from the config.
        """
        self._bind_hotkeys()
        self._set_initial_opacity()


    def _bind_hotkeys(self) -> None:
        """
        Register the hotkey bindings for opacity adjustments.
        """
        self.app.root.bind("<Control-Shift-Alt-Up>", self._alpha_up)
        self.app.root.bind("<Control-Shift-Alt-Down>", self._alpha_down)
        self.app.root.bind("<Control-Shift-Alt-Left>", self._alpha_reset)


    def _set_initial_opacity(self) -> None:
        """
        Load initial opacity from config or use the default value.
        """
        self.alpha = float(load_config().get("opacity", WINDOW_ALPHA))
        self._apply_alpha()


    def _apply_alpha(self) -> None:
        """
        Apply the current alpha value to the app window and save it to config.
        """
        # Ensure alpha stays within allowed bounds
        self.alpha = max(ALPHA_MIN, min(ALPHA_MAX, self.alpha))

        # Prefer the app API so UI thread and persistence are consistent
        if hasattr(self.app, "set_opacity"):
            self.app.set_opacity(self.alpha)
        else:
            # Fallback path if used outside the main app
            self.app.root.attributes("-alpha", self.alpha)
            config = load_config()
            config["opacity"] = self.alpha
            save_config(config)


    def _alpha_up(self, _evt: Optional[Any] = None) -> None:
        """
        Increase opacity by ALPHA_STEP.
        """
        info("[HOTKEY] Opacity up")
        self.alpha += ALPHA_STEP
        self._apply_alpha()


    def _alpha_down(self, _evt: Optional[Any] = None) -> None:
        """
        Decrease opacity by ALPHA_STEP.
        """
        info("[HOTKEY] Opacity down")
        self.alpha -= ALPHA_STEP
        self._apply_alpha()


    def _alpha_reset(self, _evt: Optional[Any] = None) -> None:
        """
        Reset opacity to the default WINDOW_ALPHA.
        """
        info("[HOTKEY] Opacity reset")
        self.alpha = WINDOW_ALPHA
        self._apply_alpha()
