from utils import config

WINDOW_ALPHA = 0.72
ALPHA_STEP = 0.05
ALPHA_MIN = 0.20
ALPHA_MAX = 1.00

class Hotkeys:

    def __init__(self, app):
        self.app = app

    def bind(self):
        self._bind_hotkeys()
        self._set_initial_opacity()

    def _bind_hotkeys(self):
        self.app.root.bind("<Control-Shift-Alt-Up>", self._alpha_up)
        self.app.root.bind("<Control-Shift-Alt-Down>", self._alpha_down)
        self.app.root.bind("<Control-Shift-Alt-Left>", self._alpha_reset)

    def _set_initial_opacity(self):
        self.alpha = float(config.load_config().get("opacity", WINDOW_ALPHA))
        self._apply_alpha()

    def _apply_alpha(self):
        self.alpha = max(ALPHA_MIN, min(ALPHA_MAX, self.alpha))
        self.app.root.attributes("-alpha", self.alpha)
        config.save_config({"opacity": self.alpha})

    def _alpha_up(self, _evt=None):
        self.alpha += ALPHA_STEP
        self._apply_alpha()

    def _alpha_down(self, _evt=None):
        self.alpha -= ALPHA_STEP
        self._apply_alpha()

    def _alpha_reset(self, _evt=None):
        self.alpha = WINDOW_ALPHA
        self._apply_alpha()
