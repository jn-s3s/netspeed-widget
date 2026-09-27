"""Widget interaction tests built on recorded fakes instead of a live Tk.

Each test asserts on what the widget did to its own window, canvas and
persisted state, which is the behavior a regression could actually break.
"""

from types import SimpleNamespace
from unittest.mock import MagicMock

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
        palette = app_module.THEMES["Default"]
        expected_color = {
            20: palette.down,
            120: palette.warn,
            400: palette.bad,
        }[ms]
        expected_up_color = palette.up if ms == 20 else expected_color
        assert fake_gui.canvas.items[widget._t_down_val]["fill"] == expected_color
        assert fake_gui.canvas.items[widget._t_up_val]["fill"] == expected_up_color
        assert fake_gui.canvas.items[widget._t_up_arrow]["fill"] == expected_up_color
        assert fake_gui.canvas.items[widget._t_st_up]["fill"] == expected_up_color

    def test_offline_recolors_speed_rows_to_red(self, make_widget, fake_gui):
        widget = make_widget()
        widget.probe.latest = SimpleNamespace(ok=False, ms=None, ts=0.0)

        widget._render_latency()

        bad_color = app_module.THEMES["Default"].bad
        assert fake_gui.canvas.text_of(widget._t_ping) == "offline"
        assert fake_gui.canvas.items[widget._t_down_val]["fill"] == bad_color
        assert fake_gui.canvas.items[widget._t_up_val]["fill"] == bad_color
        assert fake_gui.canvas.items[widget._t_up_arrow]["fill"] == bad_color
        assert fake_gui.canvas.items[widget._t_st_up]["fill"] == bad_color

    def test_pending_measurement_reads_dim(self, make_widget, fake_gui):
        """TCP has no number yet while the ICMP second opinion still answers.

        This is the widget's state for the first seconds of every run, and the
        one path where the readout color and the recorded health differ, so it
        must not feed a resolved palette color back into the health lookup.
        """
        widget = make_widget()
        widget.probe.latest = SimpleNamespace(ok=True, ms=None, ts=0.0)

        widget._render_latency()

        palette = app_module.THEMES["Default"]
        assert fake_gui.canvas.text_of(widget._t_ping) == "-- ms"
        assert fake_gui.canvas.items[widget._t_ping]["fill"] == palette.fg_dim
        assert fake_gui.canvas.items[widget._dot_item]["fill"] == palette.warn
        assert widget._health == app_module.Health.WARN

    def test_speedtest_run_freezes_health_recording(
        self, make_widget, fake_gui, monkeypatch
    ):
        """A running speedtest must not record its inflated latency as health.

        The test saturates the connection, so 400 ms of probe latency is an
        artifact, not an outage: `_record_status` must be skipped entirely and
        the rows must keep the color they had before the test started.
        """
        widget = make_widget()
        widget._speedtest_running = True
        widget.probe.latest = SimpleNamespace(ok=True, ms=400.0, ts=0.0)
        recorded = []
        monkeypatch.setattr(
            widget, "_record_status", lambda *_a: recorded.append(1) or False
        )

        widget._render_latency()

        assert recorded == []
        assert fake_gui.canvas.text_of(widget._t_ping) == "test..."
        palette = app_module.THEMES["Default"]
        assert fake_gui.canvas.items[widget._t_down_val]["fill"] == palette.down
        assert fake_gui.canvas.items[widget._dot_item]["fill"] != palette.bad


class TestThemeSwitching:
    """A theme change repaints the widget, persists and rebuilds the menu."""

    def test_hover_indicator_tracks_menu_and_tray_changes(self, make_widget, fake_gui):
        widget = make_widget()

        def hover_labels():
            return [
                call.kwargs["label"]
                for call in fake_gui.menu.entryconfig.call_args_list
                if "Auto-hide on hover" in call.kwargs.get("label", "")
            ]

        widget._popup_menu(SimpleNamespace(x_root=0, y_root=0))
        assert "  Auto-hide on hover" in hover_labels()[-1]
        assert fake_gui.menu.add_checkbutton.call_args.kwargs["indicatoron"] is False

        widget.toggle_hover_hide()  # tray action
        assert "✓  Auto-hide on hover" in hover_labels()[-1]

        config.set_hide_on_hover(False)  # another process/UI changed the setting
        widget._popup_menu(SimpleNamespace(x_root=0, y_root=0))
        assert "  Auto-hide on hover" in hover_labels()[-1]

    def test_opacity_indicator_tracks_selection_and_current_value(
        self, make_widget, fake_gui
    ):
        widget = make_widget()

        widget.set_opacity(0.8)
        fake_gui.root.run_after()

        labels = [
            call.kwargs["label"]
            for call in fake_gui.menu.entryconfig.call_args_list
            if "Opacity (" in call.kwargs.get("label", "")
        ]
        assert labels[-1] == "Opacity (80%)"
        opacity_choices = [
            call.kwargs["label"]
            for call in fake_gui.menu.entryconfig.call_args_list
            if call.kwargs.get("label", "").endswith("%")
        ]
        assert "✓  80%" in opacity_choices

        widget._apply_theme("Light")
        widget._popup_menu(SimpleNamespace(x_root=0, y_root=0))
        labels = [
            call.kwargs["label"]
            for call in fake_gui.menu.entryconfig.call_args_list
            if "Opacity (" in call.kwargs.get("label", "")
        ]
        assert labels[-1] == "Opacity (80%)"

    def test_theme_menu_marks_selected_entry_in_light_and_dark_palettes(
        self, make_widget, fake_gui
    ):
        widget = make_widget()

        def selected_label():
            labels = [
                call.kwargs["label"]
                for call in fake_gui.menu.add_radiobutton.call_args_list
            ]
            return next(label for label in reversed(labels) if "✓" in label)

        assert selected_label().strip() == "✓  Default"
        assert all(
            call.kwargs["indicatoron"] is False
            for call in fake_gui.menu.add_radiobutton.call_args_list
        )

        widget._apply_theme("Light")

        assert selected_label().strip() == "✓  Light"
        assert app_module.THEMES["Light"].fg != app_module.THEMES["Light"].surface

    def test_apply_theme_recolors_and_rebuilds_menu(
        self, make_widget, fake_gui, monkeypatch
    ):
        widget = make_widget()
        builds = []
        original_build = widget._build_menu
        monkeypatch.setattr(
            widget, "_build_menu", lambda: builds.append(1) or original_build()
        )
        old_pill_fill = fake_gui.canvas.items[widget._pill]["fill"]

        widget._apply_theme("Ocean")

        assert builds == [1]
        assert widget._theme_name == "Ocean"
        assert config.get_theme() == "Ocean"
        assert (
            fake_gui.canvas.items[widget._pill]["fill"]
            == app_module.THEMES["Ocean"].surface
        )
        assert old_pill_fill != app_module.THEMES["Ocean"].surface

    def test_apply_unknown_theme_is_ignored(self, make_widget, fake_gui, monkeypatch):
        widget = make_widget()
        builds = []
        original_build = widget._build_menu
        monkeypatch.setattr(
            widget, "_build_menu", lambda: builds.append(1) or original_build()
        )

        widget._apply_theme("No Such Theme")

        assert builds == []
        assert widget._theme_name == "Default"
        assert config.get_theme() == "Default"

    def test_public_theme_change_marshals_and_refreshes_tray(
        self, make_widget, monkeypatch
    ):
        widget = make_widget()
        tray = MagicMock()
        widget.attach_tray(tray)

        widget.set_theme("Ocean")

        assert widget._theme_name == "Default"
        widget.root.run_after()
        assert widget._theme_name == "Ocean"
        tray.update_settings.assert_called_once_with()

    def test_status_dot_is_painted_at_startup(self, make_widget, fake_gui):
        """A fresh widget shows its health color immediately, not after a tick."""
        widget = make_widget()

        assert (
            fake_gui.canvas.items[widget._dot_item]["fill"]
            == app_module.THEMES["Default"].good
        )

    @pytest.mark.parametrize("rounded", [True, False])
    def test_theme_switch_repaints_the_canvas_background(
        self, make_widget, fake_gui, rounded
    ):
        """The flat fallback paints its surface on the canvas, not just the pill.

        Where the transparency key is unavailable the canvas background is the
        only surface behind the pill, so a stale one shows through at the edges.
        """
        widget = make_widget()
        widget._rounded = rounded

        widget._apply_theme("Ocean")

        expected = (
            app_module.TRANSPARENT if rounded else app_module.THEMES["Ocean"].surface
        )
        assert fake_gui.canvas.config["bg"] == expected
