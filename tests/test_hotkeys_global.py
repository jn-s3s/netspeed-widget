"""Global hotkey registration tests, with the Win32 layer stubbed.

The listener thread's message pump is replaced and the window procedures are
called the way Windows would call them. Without this the suite leaked a live
`hotkey-listener` thread per test and never exercised the contract that
matters most: a combo the system rejects must not unbind the working hotkey.
"""

import threading
from types import SimpleNamespace
from unittest.mock import MagicMock

import pytest
import pywintypes

from utils import hotkeys as hk
from utils.hotkeys import GlobalHotkey, _ApplyRequest

MODS = hk.win32con.MOD_CONTROL


@pytest.fixture
def win32(monkeypatch):
    """Stub the windowing calls and keep the pump from running for real."""
    gui = MagicMock()
    close_signal = threading.Event()
    gui.PumpMessages.side_effect = lambda: close_signal.wait(timeout=5)
    monkeypatch.setattr(hk, "win32gui", gui)
    monkeypatch.setattr(hk, "win32api", MagicMock(GetModuleHandle=lambda _n: 0))
    return SimpleNamespace(gui=gui, close=close_signal)


@pytest.fixture
def listener(win32):
    """A registered listener that is always joined before the test ends."""
    key = GlobalHotkey(lambda: None)
    key._hwnd = 1234
    yield key
    key._hwnd = None
    win32.close.set()
    key._thread.join(timeout=5)
    assert not key._thread.is_alive(), "the listener thread outlived the test"


def _request(vk: int = ord("N")) -> _ApplyRequest:
    return _ApplyRequest(mod=MODS, vk=vk, done=threading.Event())


class TestRegistrationSwap:
    def test_the_new_combo_is_bound_before_the_old_one_is_released(
        self, listener, win32
    ):
        listener._active_id = 1
        request = _request()
        listener._request = request

        listener._wm_apply(1234, 0, 0, 0)

        assert win32.gui.RegisterHotKey.call_args.args[1] == 2
        win32.gui.UnregisterHotKey.assert_called_once_with(1234, 1)
        assert listener._active_id == 2
        assert request.ok is True
        assert request.done.is_set()

    def test_a_rejected_combo_keeps_the_working_registration(self, listener, win32):
        listener._active_id = 2
        request = _request()
        listener._request = request
        win32.gui.RegisterHotKey.side_effect = pywintypes.error(
            1409, "RegisterHotKey", "Hotkey already registered"
        )

        listener._wm_apply(1234, 0, 0, 0)

        assert request.ok is False
        assert request.error == "Hotkey already registered"
        win32.gui.UnregisterHotKey.assert_not_called()
        assert listener._active_id == 2
        assert request.done.is_set()

    def test_ids_alternate_so_the_next_swap_has_a_free_slot(self, listener, win32):
        listener._active_id = None

        for expected in (1, 2, 1):
            request = _request()
            listener._request = request

            listener._wm_apply(1234, 0, 0, 0)

            assert request.ok is True
            assert win32.gui.RegisterHotKey.call_args.args[1] == expected
            assert listener._active_id == expected

    def test_an_abandoned_request_is_not_applied(self, listener, win32):
        """A request the caller gave up on must not register anything later."""
        listener._request = None

        assert listener._wm_apply(1234, 0, 0, 0) == 0
        win32.gui.RegisterHotKey.assert_not_called()


class TestApplyCombo:
    def test_a_malformed_combo_is_refused_without_touching_the_listener(
        self, listener, win32
    ):
        ok, error = listener.apply_combo("n")

        assert ok is False
        assert "modifiers" in error
        win32.gui.PostMessage.assert_not_called()

    def test_a_dead_listener_reports_its_startup_error_instead_of_waiting(
        self, listener, win32
    ):
        listener._hwnd = None
        listener._init_error = "window class already registered"

        ok, error = listener.apply_combo("ctrl+shift+n")

        assert ok is False
        assert error == "window class already registered"
        win32.gui.PostMessage.assert_not_called()

    def test_a_listener_that_never_answers_times_out_and_forgets(
        self, listener, win32, monkeypatch
    ):
        """The wait is bounded, and no stale request survives for the next caller."""
        monkeypatch.setattr(listener, "_take_request", lambda: None)

        ok, error = listener.apply_combo("ctrl+alt+m")

        assert ok is False
        assert "did not respond" in error
        assert listener._request is None


class TestShutdown:
    def test_stop_signals_close_and_unbinds_both_slots(self, listener, win32):
        listener._active_id = 1

        listener.stop()
        listener._wm_close(1234, 0, 0, 0)

        released = {call.args[1] for call in win32.gui.UnregisterHotKey.call_args_list}
        assert released == {1, 2}
        assert listener._active_id is None
        assert listener._hwnd is None

    def test_stop_twice_posts_once(self, listener, win32):
        listener.stop()
        before = win32.gui.PostMessage.call_count

        listener.stop()

        assert win32.gui.PostMessage.call_count == before

    def test_the_window_class_is_unregistered_when_the_pump_returns(self, win32):
        key = GlobalHotkey(lambda: None)
        key._hwnd = 4321
        assert key._class_atom is not None

        win32.close.set()
        key._thread.join(timeout=5)

        win32.gui.UnregisterClass.assert_called_once()
        assert key._hwnd is None
        assert key._class_atom is None


class TestTrigger:
    def test_either_registration_id_fires_the_callback(self, win32):
        calls = []
        key = GlobalHotkey(lambda: calls.append(1))
        key._hwnd = 7

        key._wm_hotkey(7, 0, 1, 0)
        key._wm_hotkey(7, 0, 2, 0)
        key._wm_hotkey(7, 0, 9, 0)

        assert len(calls) == 2
        key._hwnd = None
        win32.close.set()
        key._thread.join(timeout=5)

    def test_a_raising_callback_does_not_kill_the_pump(self, win32):
        def explode():
            raise ValueError("widget code failed")

        key = GlobalHotkey(explode)
        key._hwnd = 7

        assert key._wm_hotkey(7, 0, 1, 0) == 0

        key._hwnd = None
        win32.close.set()
        key._thread.join(timeout=5)
