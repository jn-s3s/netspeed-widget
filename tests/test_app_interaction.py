"""Widget interaction tests built on recorded fakes instead of a live Tk.

Each test asserts on what the widget did to its own window, canvas and
persisted state, which is the behavior a regression could actually break.
"""

from types import SimpleNamespace

import pytest

import app as app_module
from utils import config
from utils.speedtest import SpeedtestResult


def _event(x_root=0, y_root=0):
    return SimpleNamespace(x_root=x_root, y_root=y_root)


class TestWindowLifecycle:
    """Show, hide and toggle behavior."""

    def test_toggle_from_normal_hides(self, make_widget, fake_gui):
        widget = make_widget()
        fake_gui.root.calls.clear()

        widget.toggle_window()

        assert fake_gui.root.state() == "withdrawn"
        assert widget._run is True

    def test_toggle_from_withdrawn_reshows(self, make_widget, fake_gui):
        widget = make_widget()
        fake_gui.root.withdraw()
        fake_gui.root.calls.clear()

        widget.toggle_window()

        names = [call for call, _ in fake_gui.root.calls]
        assert names == ["deiconify", "attributes"]
        assert fake_gui.root.state() == "normal"

    def test_shutdown_stops_services_and_destroys(self, make_widget, fake_gui):
        widget = make_widget()

        widget.shutdown()

        assert widget._run is False
        widget.sampler.stop.assert_called_once()
        widget.probe.stop.assert_called_once()
        widget.hotkey.stop.assert_called_once()
        assert fake_gui.root.destroyed

    def test_ui_call_after_destroy_is_dropped(self, make_widget, fake_gui):
        widget = make_widget()
        fake_gui.root.destroyed = True

        def raise_after_destroy():
            raise RuntimeError("main thread is not in main loop")

        fake_gui.root.after = lambda *_a, **_k: raise_after_destroy()

        widget.ui_call(print)


class TestDrag:
    """Dragging, and the stuck-release case that used to disable the rescue."""

    def test_drag_moves_and_persists(self, make_widget, monkeypatch):
        widget = make_widget(position=(500, 300))
        monkeypatch.setattr(widget.root, "winfo_x", lambda: 500)
        monkeypatch.setattr(widget.root, "winfo_y", lambda: 300)

        widget._start_drag(_event(x_root=600, y_root=340))
        widget._on_drag(_event(x_root=650, y_root=360))
        widget._end_drag()

        assert (widget.win_x, widget.win_y) == (550, 320)
        assert widget._dragging is False
        assert config.get_position() == (550, 320)

    def test_missing_release_is_healed_on_next_tick(self, make_widget, monkeypatch):
        """A right-click during a drag swallows the release; the tick must end it.

        Without the repair `_dragging` stays set for the rest of the run and
        `_ensure_visible` returns early forever, stranding the widget off a
        detached monitor.
        """
        widget = make_widget(position=(500, 300))
        widget._start_drag(_event(x_root=500, y_root=300))
        assert widget._dragging is True
        monkeypatch.setattr(app_module.win32api, "GetAsyncKeyState", lambda _vk: 0)

        widget._tick()

        assert widget._dragging is False

    def test_held_button_keeps_the_drag_open(self, make_widget, monkeypatch):
        widget = make_widget()
        widget._start_drag(_event())
        monkeypatch.setattr(app_module.win32api, "GetAsyncKeyState", lambda _vk: 0x8000)

        widget._tick()

        assert widget._dragging is True

    def test_hover_hide_is_skipped_while_dragging(self, make_widget, monkeypatch):
        widget = make_widget()
        monkeypatch.setattr(app_module, "get_hide_on_hover", lambda: True)
        widget._dragging = True

        widget._on_mouse_enter()

        assert widget._hover_guard_active is False
        assert widget.root.state() != "withdrawn"


class TestHoverHide:
    """The auto-hide state machine and its failure paths."""

    def test_hover_hides_then_restores_when_cursor_leaves(
        self, make_widget, fake_gui, monkeypatch
    ):
        widget = make_widget(position=(100, 100))
        monkeypatch.setattr(app_module, "get_hide_on_hover", lambda: True)
        inside = (110, 110)
        monkeypatch.setattr(app_module.win32api, "GetCursorPos", lambda: inside)

        widget._on_mouse_enter()
        assert fake_gui.root.state() == "withdrawn"
        assert widget._hover_guard_active is True

        monkeypatch.setattr(app_module.win32api, "GetCursorPos", lambda: (900, 900))
        fake_gui.root.run_after()

        assert fake_gui.root.state() == "normal"
        assert widget._hover_guard_active is False

    def test_failed_cursor_query_releases_the_guard(
        self, make_widget, fake_gui, monkeypatch
    ):
        """A locked or secure desktop blocks GetCursorPos.

        The guard has to be released anyway, or auto-hide and the off-monitor
        rescue stay dead for the rest of the run.
        """
        widget = make_widget(position=(100, 100))
        monkeypatch.setattr(app_module, "get_hide_on_hover", lambda: True)
        monkeypatch.setattr(
            app_module.win32api,
            "GetCursorPos",
            lambda: (_ for _ in ()).throw(OSError("desktop locked")),
        )

        widget._on_mouse_enter()
        widget._poll_cursor_and_restore()

        assert widget._hover_guard_active is False

    def test_poll_limit_forces_a_restore(self, make_widget, fake_gui, monkeypatch):
        widget = make_widget(position=(100, 100))
        monkeypatch.setattr(app_module, "get_hide_on_hover", lambda: True)
        monkeypatch.setattr(app_module.win32api, "GetCursorPos", lambda: (110, 110))

        widget._on_mouse_enter()
        widget._poll_cursor_and_restore(attempts=app_module.HOVER_POLL_LIMIT)

        assert widget._hover_guard_active is False
        assert fake_gui.root.state() == "normal"


class TestMonitorRescue:
    """Repositioning when the saved or current spot leaves every screen."""

    def test_off_monitor_start_falls_back_to_default(self, make_widget, monkeypatch):
        monkeypatch.setattr(
            app_module.monitors,
            "default_position",
            lambda _w, _h, _m: (1500, 900),
        )

        widget = make_widget(position=(4000, 4000), rects=[(0, 0, 1920, 1080)])

        assert (widget.win_x, widget.win_y) == (1500, 900)
        assert config.get_position() == (1500, 900)

    def test_layout_change_snaps_back(self, make_widget, monkeypatch):
        widget = make_widget(position=(500, 300), rects=[(0, 0, 1920, 1080)])
        widget.win_x, widget.win_y = 4000, 4000
        monkeypatch.setattr(
            app_module.monitors, "default_position", lambda _w, _h, _m: (1500, 900)
        )
        monkeypatch.setattr(
            app_module.monitors, "active_monitor_rects", lambda: [(0, 0, 1920, 1080)]
        )

        widget._ensure_visible()

        assert (widget.win_x, widget.win_y) == (1500, 900)
        assert config.get_position() == (1500, 900)

    def test_unverifiable_layout_is_left_alone(self, make_widget, monkeypatch):
        widget = make_widget()
        widget.win_x, widget.win_y = 4000, 4000
        monkeypatch.setattr(app_module.monitors, "active_monitor_rects", list)

        widget._ensure_visible()

        assert (widget.win_x, widget.win_y) == (4000, 4000)


class TestOpacity:
    """Opacity is driven from the tray thread, so it must marshal."""

    def test_set_opacity_from_other_thread_queues_on_tk(self, make_widget, fake_gui):
        widget = make_widget()
        before = len(fake_gui.root.after_queue)

        widget.set_opacity(0.5)

        assert len(fake_gui.root.after_queue) == before + 1
        assert widget.opacity != 0.5
        fake_gui.root.run_after()
        assert widget.opacity == 0.5
        assert config.get_opacity() == 0.5

    def test_set_opacity_clamps_once(self, make_widget, fake_gui):
        widget = make_widget()

        widget.set_opacity(9.0)
        fake_gui.root.run_after()

        assert widget.opacity == 1.0

    def test_set_opacity_ignores_a_non_number(self, make_widget, fake_gui):
        widget = make_widget()
        before = len(fake_gui.root.after_queue)

        widget.set_opacity("nope")

        assert len(fake_gui.root.after_queue) == before


class TestSpeedtestReporting:
    """A measured result is kept; an estimate is labelled and not stored."""

    def test_measured_result_is_persisted(self, make_widget, fake_gui, monkeypatch):
        widget = make_widget()
        result = SpeedtestResult(down_mbps=88.4, up_mbps=21.7, source="fast-cli")
        monkeypatch.setattr(app_module, "measure_speed", lambda: result)
        widget._speedtest_running = True

        widget._speedtest_worker()
        fake_gui.root.run_after()

        assert config.get_speedtest()["down_mbps"] == 88.4
        assert widget._speedtest_summary == "Last speedtest: 88.4 D | 21.7 U Mb/s"
        assert fake_gui.canvas.text_of(widget._t_st_unit) == "Mb/s speedtest"
        assert widget._speedtest_running is False

    def test_estimate_is_labelled_and_not_persisted(
        self, make_widget, fake_gui, monkeypatch
    ):
        widget = make_widget()
        saved = []
        monkeypatch.setattr(app_module, "set_speedtest", lambda *a: saved.append(a))
        result = SpeedtestResult(down_mbps=3.0, up_mbps=0.4, source="passive-estimate")
        monkeypatch.setattr(app_module, "measure_speed", lambda: result)

        widget._speedtest_worker()
        fake_gui.root.run_after()

        assert saved == []
        assert "estimate" in widget._speedtest_summary
        assert fake_gui.canvas.text_of(widget._t_st_unit) == "Mb/s estimate"

    def test_failure_clears_the_running_latch(self, make_widget, monkeypatch):
        widget = make_widget()
        widget._speedtest_running = True

        def boom():
            raise RuntimeError("no provider")

        monkeypatch.setattr(app_module, "measure_speed", boom)

        widget._speedtest_worker()

        assert widget._speedtest_running is False

    def test_duplicate_trigger_is_ignored(self, make_widget, monkeypatch):
        widget = make_widget()
        started = []
        monkeypatch.setattr(
            app_module.threading,
            "Thread",
            lambda **kw: (
                started.append(kw["target"]) or SimpleNamespace(start=lambda: None)
            ),
        )

        widget.run_speedtest_now(manual=True)
        widget.run_speedtest_now(manual=True)

        assert len(started) == 1

    def test_worker_that_cannot_start_releases_the_latch(
        self, make_widget, monkeypatch
    ):
        widget = make_widget()

        class Dead:
            def start(self):
                raise OSError("no more threads")

        monkeypatch.setattr(app_module.threading, "Thread", lambda **_kw: Dead())

        widget.run_speedtest_now(manual=True)

        assert widget._speedtest_running is False


class TestTickLoop:
    """A render failure must not stop the loop or flood the log."""

    def test_loop_reschedules_after_an_exception(self, make_widget, fake_gui):
        widget = make_widget()
        widget.probe.latest = SimpleNamespace(ok=True, ms=None, ts=0.0)
        fake_gui.root.after_queue.clear()

        def explode(*_a):
            raise TypeError("canvas died")

        fake_gui.canvas.itemconfig = explode
        widget._tick()

        assert len(fake_gui.root.after_queue) == 1
        assert widget._tick_failures == 1

    def test_repeated_failures_are_reported_once_in_a_while(
        self, make_widget, fake_gui, monkeypatch
    ):
        widget = make_widget()
        reported = []
        monkeypatch.setattr(app_module._log, "error", lambda msg: reported.append(msg))

        for _ in range(app_module.TICK_REPORT_EVERY + 1):
            widget._report_tick_failure(ValueError("x"))

        assert len(reported) == 2

    @pytest.mark.parametrize("ms", [20, 120, 400])
    def test_latency_color_bands(self, make_widget, fake_gui, ms):
        widget = make_widget()
        widget.probe.latest = SimpleNamespace(ok=True, ms=ms, ts=0.0)

        widget._render_latency()

        assert fake_gui.canvas.text_of(widget._t_ping) == f"{ms:.0f} ms"
        expected_color = {
            20: app_module.GOOD_COLOR,
            120: app_module.WARN_COLOR,
            400: app_module.BAD_COLOR,
        }[ms]
        expected_up_color = app_module.UP_COLOR if ms == 20 else expected_color
        assert fake_gui.canvas.items[widget._t_down_val]["fill"] == expected_color
        assert fake_gui.canvas.items[widget._t_up_val]["fill"] == expected_up_color
        assert fake_gui.canvas.items[widget._t_up_arrow]["fill"] == expected_up_color
        assert fake_gui.canvas.items[widget._t_st_up]["fill"] == expected_up_color

    def test_offline_recolors_speed_rows_to_red(self, make_widget, fake_gui):
        widget = make_widget()
        widget.probe.latest = SimpleNamespace(ok=False, ms=None, ts=0.0)

        widget._render_latency()

        assert fake_gui.canvas.text_of(widget._t_ping) == "offline"
        assert fake_gui.canvas.items[widget._t_down_val]["fill"] == app_module.BAD_COLOR
        assert fake_gui.canvas.items[widget._t_up_val]["fill"] == app_module.BAD_COLOR
        assert fake_gui.canvas.items[widget._t_up_arrow]["fill"] == app_module.BAD_COLOR
        assert fake_gui.canvas.items[widget._t_st_up]["fill"] == app_module.BAD_COLOR
