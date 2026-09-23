"""Config persistence tests, run against real files in an isolated APPDATA.

Everything here writes actual JSON to the temp directory the autouse fixture
provides, because the behaviors that matter, a corrupt file, a torn write and
two threads saving at once, cannot be reached through a mocked open().
"""

import json
import threading
from unittest.mock import MagicMock

import pytest

from utils import config
from utils.logger import Logger
from utils.paths import config_path


def _raw() -> str:
    return config_path(config.CONFIG_FILE).read_text(encoding="utf-8")


def _write_raw(text: str) -> None:
    config_path(config.CONFIG_FILE).write_text(text, encoding="utf-8")


@pytest.fixture
def spy(monkeypatch):
    """Swap the module logger for a mock so reported failures are assertable."""
    mock = MagicMock(spec=Logger)
    monkeypatch.setattr(config, "_log", mock)
    return mock


class TestLoadAndSave:
    def test_missing_file_reads_empty(self, spy):
        assert config.load_config() == {}
        spy.warning.assert_not_called()

    def test_round_trip_keeps_every_key(self):
        config.set_opacity(0.55)
        config.set_position(12, 34)
        config.set_hotkey("ctrl+alt+k")

        saved = config.load_config()
        assert saved["opacity"] == 0.55
        assert saved["position"] == {"x": 12, "y": 34}
        assert saved["hotkey"] == "ctrl+alt+k"

    def test_corrupt_file_is_reported_then_treated_as_absent(self, spy):
        _write_raw('{"opacity": 0.5')

        assert config.load_config() == {}
        spy.warning.assert_called_once()
        assert "unreadable config" in spy.warning.call_args.args[0]

    def test_json_that_is_not_an_object_is_ignored(self, spy):
        _write_raw("[1, 2]")

        assert config.load_config() == {}
        spy.warning.assert_called_once()

    def test_save_leaves_no_staging_file(self):
        config.set_position(1, 2)

        staging = config_path(config.CONFIG_FILE).with_name("config.json.tmp")
        assert not staging.exists()

    def test_failed_save_keeps_the_previous_file(self, spy):
        config.set_opacity(0.6)
        before = _raw()

        config.save_config({"bad": object()})

        spy.error.assert_called_once()
        assert _raw() == before

    def test_a_failing_write_is_not_silently_swallowed(self, spy, monkeypatch):
        monkeypatch.setattr(
            config, "config_path", lambda *_a: (_ for _ in ()).throw(OSError("ro"))
        )

        config.save_config({"opacity": 0.5})

        spy.error.assert_called_once()

    def test_a_locked_config_is_not_overwritten_from_an_empty_read(
        self, isolated_appdata, monkeypatch
    ):
        """Windows reports a sharing violation mid-rename.

        Treating that as "no settings yet" and saving on top of it would drop
        every other key, so a write is skipped when the read failed outright.
        """
        config.set_position(7, 8)
        config.set_hotkey("ctrl+alt+j")
        path = config_path(config.CONFIG_FILE)
        before = path.read_text(encoding="utf-8")
        real_open = open

        def locked(file, mode="r", *args, **kwargs):
            if "r" in mode and str(file).endswith("config.json"):
                raise PermissionError("locked by another process")
            return real_open(file, mode, *args, **kwargs)

        monkeypatch.setattr("builtins.open", locked)

        config.set_opacity(0.45)

        monkeypatch.undo()
        assert path.read_text(encoding="utf-8") == before
        assert json.loads(before)["hotkey"] == "ctrl+alt+j"


class TestConcurrency:
    """The file is shared by the Tk, speedtest and tray threads."""

    def test_parallel_writers_keep_every_key(self):
        keys = [f"key{index}" for index in range(24)]

        def write(key: str) -> None:
            config._mutate(key, key)

        threads = [threading.Thread(target=write, args=(key,)) for key in keys]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()

        saved = config.load_config()
        assert all(saved.get(key) == key for key in keys)

    def test_a_reader_never_sees_a_half_written_file(self):
        stop = threading.Event()
        broken: list[str] = []

        def write_forever() -> None:
            while not stop.is_set():
                config.set_position(10, 20)

        def read_forever() -> None:
            while not stop.is_set():
                try:
                    json.loads(_raw())
                except (FileNotFoundError, PermissionError):
                    # Not yet created, or momentarily locked by the rename on
                    # Windows. Neither is a partial read: the writer replaces
                    # the file in one step, so a reader sees the old content
                    # or the new content.
                    pass
                except ValueError as err:
                    broken.append(str(err))

        writer = threading.Thread(target=write_forever)
        reader = threading.Thread(target=read_forever)
        writer.start()
        reader.start()
        threading.Event().wait(0.3)
        stop.set()
        writer.join()
        reader.join()

        assert broken == []


class TestValidation:
    """Malformed values must degrade to a default, never raise into a caller."""

    @pytest.mark.parametrize(
        "payload",
        [
            {"speedtest": {"down_mbps": "abc", "up_mbps": 1.0, "ts": 1.0}},
            {"speedtest": {"down_mbps": 1.0, "up_mbps": 1.0, "ts": None}},
            {"speedtest": {"down_mbps": 1.0}},
            {"speedtest": "not a dict"},
            {"speedtest": {"down_mbps": [], "up_mbps": 1.0, "ts": 1.0}},
        ],
    )
    def test_bad_speedtest_records_return_the_default(self, payload, spy):
        _write_raw(json.dumps(payload))

        assert config.get_speedtest() is None
        assert config.get_speedtest({"down_mbps": 0.0, "up_mbps": 0.0, "ts": 0.0}) == {
            "down_mbps": 0.0,
            "up_mbps": 0.0,
            "ts": 0.0,
        }

    def test_good_speedtest_round_trips_with_floats(self):
        config.set_speedtest(11.239, 4.56)

        record = config.get_speedtest()
        assert record == {
            "down_mbps": 11.24,
            "up_mbps": 4.56,
            "ts": pytest.approx(record["ts"]),
        }

    def test_schedule_reads_saved_ts_as_a_number(self):
        config.set_speedtest(5.0, 5.0, ts=1_000.0)

        assert config.get_speedtest()["ts"] == 1_000.0

    @pytest.mark.parametrize(
        "payload",
        [
            {"position": "top"},
            {"position": {"x": 1}},
            {"position": {"x": "a", "y": 1}},
            {"position": None},
        ],
    )
    def test_unusable_positions_return_none(self, payload):
        _write_raw(json.dumps(payload))

        assert config.get_position() is None

    def test_opacity_is_clamped_into_the_supported_range(self):
        stored = config.set_opacity(3.5)

        assert stored == config.MAX_OPACITY
        assert config.get_opacity() == config.MAX_OPACITY

    def test_non_numeric_opacity_falls_back_to_default(self):
        _write_raw(json.dumps({"opacity": "glass"}))

        assert config.get_opacity(default=0.66) == 0.66

    @pytest.mark.parametrize(
        ("value", "expected"),
        [(1.0, 1.0), (0.2, config.MIN_OPACITY), (0.72, 0.72)],
    )
    def test_clamp_opacity(self, value, expected):
        assert config.clamp_opacity(value) == expected

    def test_clamp_opacity_rejects_a_non_number(self):
        with pytest.raises((TypeError, ValueError)):
            config.clamp_opacity("glass")

    def test_hotkey_must_be_a_non_empty_string(self):
        _write_raw(json.dumps({"hotkey": 42}))

        assert config.get_hotkey() == config.DEFAULT_HOTKEY
