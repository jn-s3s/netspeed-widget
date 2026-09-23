"""Log writing, level formatting and size-based rollover.

These run against the real file under the isolated APPDATA directory, since
rollover and a failed write are both properties of the file itself.
"""

from utils import logger
from utils.logger import Logger
from utils.paths import config_path


def _text() -> str:
    path = config_path(logger.LOG_FILE)
    return path.read_text(encoding="utf-8") if path.exists() else ""


class TestLevels:
    """Every line carries a level and the subsystem that wrote it."""

    def test_info_warn_and_error_are_distinct(self, isolated_appdata):
        log = logger.get("sampler")

        log.info("one")
        log.warning("two")
        log.error("three")

        written = _text()
        assert "[INFO] [SAMPLER] one" in written
        assert "[WARN] [SAMPLER] two" in written
        assert "[ERROR] [SAMPLER] three" in written

    def test_timestamp_starts_every_line(self, isolated_appdata):
        logger.get("app").info("marked")

        line = _text().strip().splitlines()[-1]
        assert line.startswith("[20")

    def test_get_shares_one_instance_per_subsystem(self):
        assert logger.get("tray") is logger.get("TRAY")
        assert isinstance(logger.get("tray"), Logger)

    def test_startup_banner_names_the_app(self, isolated_appdata):
        logger.startup("NetSpeed Widget v9.9.9")

        written = _text()
        assert "Startup - NetSpeed Widget v9.9.9" in written
        assert "python=" in written

    def test_no_message_is_prefixed_with_a_bracket_tag_by_callers(
        self, isolated_appdata
    ):
        """The subsystem tag belongs to the logger, not to 41 message strings."""
        log = logger.get("net")

        log.warning("[NET] legacy style")

        assert "[NET] [NET]" in _text()


class TestRotation:
    """A widget that runs for weeks must not grow the file without limit."""

    def test_the_file_rolls_once_it_passes_the_cap(self, isolated_appdata, monkeypatch):
        monkeypatch.setattr(logger, "MAX_LOG_BYTES", 200)
        log = logger.get("app")

        for _ in range(40):
            log.info("a line long enough to fill the cap quickly" * 2)

        assert config_path(logger.LOG_FILE).with_name("log.txt.1").exists()
        assert config_path(logger.LOG_FILE).stat().st_size <= 400

    def test_only_the_configured_number_of_backups_survive(
        self, isolated_appdata, monkeypatch
    ):
        monkeypatch.setattr(logger, "MAX_LOG_BYTES", 100)
        monkeypatch.setattr(logger, "ROTATED_FILES", 2)
        log = logger.get("app")

        for _ in range(60):
            log.info("x" * 60)

        folder = config_path(logger.LOG_FILE).parent
        present = sorted(p.name for p in folder.glob("log.txt*"))
        assert present == ["log.txt", "log.txt.1", "log.txt.2"]


class TestFailureHandling:
    """A broken log path cannot be allowed to break the app."""

    def test_a_failed_write_raises_nothing(self, monkeypatch, capsys):
        def refuse(*_a, **_k):
            raise OSError("profile is read-only")

        monkeypatch.setattr(logger, "config_path", refuse)

        logger.get("app").warning("still trying")

        assert "write failed" in capsys.readouterr().err

    def test_the_dropped_line_is_named_in_the_fallback(self, monkeypatch, capsys):
        def refuse(*_a, **_k):
            raise OSError("disk full")

        monkeypatch.setattr(logger, "config_path", refuse)

        logger.get("config").error("the message that could not be stored")

        assert "the message that could not be stored" in capsys.readouterr().err
