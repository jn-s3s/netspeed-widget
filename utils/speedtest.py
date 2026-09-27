"""Speedtest measurement with pluggable backends.

Providers are tried in order and the first successful result wins:

1. fast-cli bundled with the app (Node runtime under third_party)
2. fast-cli or fast on PATH
3. the speedtest-cli Python package (imported lazily for fast startup)
4. a passive estimate from psutil counter deltas

Only the first three measure the link, so only they are kept as a result.
The passive provider reports what traffic happened to cross the wire, which
is meaningless on an idle connection, so it returns None unless it observed
a real load and its outcome is labelled as an estimate wherever it appears.
"""

import json
import os
import shutil
import subprocess
import time
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

import psutil

from utils.logger import get
from utils.paths import bundled_node_path, fast_bundle_dir, fast_cli_entry_path

FAST_TIMEOUT_SEC = 180
PASSIVE_SAMPLE_SEC = 10
PASSIVE_MIN_MBPS = 1.0

SOURCE_FAST = "fast-cli"
SOURCE_SPEEDTEST = "speedtest-cli"
SOURCE_PASSIVE = "passive-estimate"

_log = get("speedtest")


@dataclass(frozen=True)
class SpeedtestResult:
    """Measured throughput in megabits per second, with its provenance.

    `source` names the provider that produced the number. Only
    `SOURCE_PASSIVE` is an observation of incidental traffic rather than a
    measurement, which is what tells the caller whether to persist it.
    """

    down_mbps: float
    up_mbps: float
    source: str

    @property
    def measured(self) -> bool:
        """True when a provider probed the link rather than watched it."""
        return self.source != SOURCE_PASSIVE


def measure_speed() -> SpeedtestResult:
    """Try each provider in order and return the first success.

    Raises:
        RuntimeError: If every provider failed, naming them for the log.
    """
    providers: tuple[tuple[str, Callable[[], SpeedtestResult | None]], ...] = (
        (SOURCE_FAST, _measure_fast_cli),
        (SOURCE_SPEEDTEST, _measure_python_speedtest),
        (SOURCE_PASSIVE, _measure_passive_estimate),
    )
    tried: list[str] = []
    for label, provider in providers:
        try:
            result = provider()
        except Exception as err:  # noqa: BLE001 - a backend can fail in any shape
            _log.warning(f"{label} raised: {err}")
            tried.append(label)
            continue
        if result is not None:
            return result
        tried.append(label)
    raise RuntimeError(
        f"no speedtest result; every backend failed or gave nothing: {', '.join(tried)}"
    )


def _measure_fast_cli() -> SpeedtestResult | None:
    """Measure with fast.com via the bundled Node bundle or PATH."""
    data = _run_fast_cli()
    down, up = _parse_fast_result(data)
    if down is None or up is None:
        _log.warning("fast-cli gave no usable speeds, trying next backend")
        return None
    return SpeedtestResult(down_mbps=down, up_mbps=up, source=SOURCE_FAST)


def _measure_python_speedtest() -> SpeedtestResult | None:
    """Measure with the speedtest-cli package via its in-process API."""
    try:
        import speedtest as speedtest_module
    except ImportError:
        _log.warning("speedtest-cli not installed, trying next backend")
        return None

    _log.info("backend: speedtest-cli (python module)")
    try:
        tester = speedtest_module.Speedtest()  # type: ignore[reportAttributeAccessIssue]
        tester.get_servers(None)
        tester.get_best_server()
        _configure_speedtest(tester)

        try:
            tester.download(threads=8)
        except TypeError:
            tester.download()

        try:
            tester.upload(threads=8, pre_allocate=True)
        except TypeError:
            try:
                tester.upload(pre_allocate=True)
            except TypeError:
                tester.upload()

        result = tester.results.dict()  # bits per second
        return SpeedtestResult(
            down_mbps=float(result.get("download", 0.0)) / 1_000_000.0,
            up_mbps=float(result.get("upload", 0.0)) / 1_000_000.0,
            source=SOURCE_SPEEDTEST,
        )
    except Exception as err:  # noqa: BLE001 - third-party failures are not typed
        _log.warning(f"speedtest-cli failed: {err}")
        return None


def _measure_passive_estimate() -> SpeedtestResult | None:
    """Estimate throughput from OS counters, only when real traffic ran.

    Returns None on an idle window. Counting bytes that nobody asked to
    move would otherwise publish a near-zero figure as the user's line
    rate, and a negative one after an adapter reset.
    """
    _log.info(f"backend: psutil passive estimate over {PASSIVE_SAMPLE_SEC}s")
    start = time.monotonic()
    first = psutil.net_io_counters()
    time.sleep(PASSIVE_SAMPLE_SEC)
    second = psutil.net_io_counters()

    elapsed = max(time.monotonic() - start, 1e-6)
    down_bytes = max(second.bytes_recv - first.bytes_recv, 0)
    up_bytes = max(second.bytes_sent - first.bytes_sent, 0)
    down = down_bytes * 8.0 / elapsed / 1e6
    up = up_bytes * 8.0 / elapsed / 1e6
    if max(down, up) < PASSIVE_MIN_MBPS:
        _log.warning(
            f"passive estimate saw no usable traffic in {elapsed:.1f}s "
            f"({down:.2f} down, {up:.2f} up Mb/s)"
        )
        return None
    return SpeedtestResult(down_mbps=down, up_mbps=up, source=SOURCE_PASSIVE)


def _configure_speedtest(tester: Any) -> None:
    """Bias speedtest-cli toward larger upload payloads on Windows.

    Larger chunks reduce under-reporting by saturating the pipe more
    consistently.
    """
    try:
        config = tester.get_config()
        sizes = config.get("sizes", {})
        sizes["upload"] = [
            256 * 1024,
            512 * 1024,
            1 * 1024 * 1024,
            2 * 1024 * 1024,
            5 * 1024 * 1024,
            10 * 1024 * 1024,
            20 * 1024 * 1024,
            30 * 1024 * 1024,
        ]
        sizes["upload_min"] = 256 * 1024
        sizes["upload_max"] = 30 * 1024 * 1024
        config["sizes"] = sizes
        tester.config.update(config)
    except Exception as err:  # noqa: BLE001 - tuning is optional, never fatal
        _log.warning(f"could not tune upload sizes: {err}")


def _run_fast_cli() -> dict | None:
    """Run fast.com via the bundled Node first, then via PATH."""
    spawn_kw = _fast_spawn_settings()
    return _run_node_bundle_fast(**spawn_kw) or _run_path_fast(**spawn_kw)


def _fast_spawn_settings() -> dict:
    """Build subprocess kwargs that suppress child windows on Windows."""
    startupinfo = None
    creationflags = 0
    if os.name == "nt":
        startupinfo = subprocess.STARTUPINFO()
        startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startupinfo.wShowWindow = 0
        creationflags = subprocess.CREATE_NO_WINDOW
    return {"startupinfo": startupinfo, "creationflags": creationflags}


def _run_node_bundle_fast(**spawn_kw: Any) -> dict | None:
    """Execute the bundled node.exe and fast-cli cli.js with --json."""
    node_exe = bundled_node_path()
    cli_js = fast_cli_entry_path()
    bundle_cwd = fast_bundle_dir()

    if not (node_exe.is_file() and cli_js.is_file()):
        return None

    _log.info(f"backend: fast-cli (bundled Node at {node_exe.parent})")
    try:
        process = subprocess.run(
            [str(node_exe), str(cli_js), "--upload", "--json"],
            cwd=str(bundle_cwd),
            capture_output=True,
            text=True,
            timeout=FAST_TIMEOUT_SEC,
            check=True,
            **spawn_kw,
        )
        return json.loads(process.stdout.strip() or "{}")
    except (OSError, subprocess.SubprocessError, json.JSONDecodeError) as err:
        _log.warning(f"bundled fast-cli failed: {err}")
        return None


def _run_path_fast(**spawn_kw: Any) -> dict | None:
    """Execute fast or fast-cli from PATH with --json."""
    on_path = shutil.which("fast") or shutil.which("fast-cli")
    if not on_path:
        return None

    _log.info(f"backend: fast-cli (PATH at {on_path})")
    try:
        process = subprocess.run(
            [on_path, "--upload", "--json"],
            capture_output=True,
            text=True,
            timeout=FAST_TIMEOUT_SEC,
            check=True,
            **spawn_kw,
        )
        return json.loads(process.stdout.strip() or "{}")
    except (OSError, subprocess.SubprocessError, json.JSONDecodeError) as err:
        _log.warning(f"PATH fast-cli failed: {err}")
        return None


def _parse_fast_result(data: dict | None) -> tuple[float | None, float | None]:
    """Parse fast.com JSON emitted by fast-cli."""
    if not isinstance(data, dict):
        return None, None

    candidates: list[tuple[dict, tuple[str, ...], tuple[str, ...]]] = [
        (data, ("downloadSpeed", "download"), ("uploadSpeed", "upload")),
    ]
    speeds = data.get("speeds")
    if isinstance(speeds, dict):
        candidates.append((speeds, ("download",), ("upload",)))

    for source, down_keys, up_keys in candidates:
        down = _first_float(source, down_keys)
        up = _first_float(source, up_keys)
        if down is not None and up is not None:
            return down, up
    return None, None


def _first_float(data: dict, keys: tuple[str, ...]) -> float | None:
    """Return the first value under any key that converts to float."""
    for key in keys:
        value = data.get(key)
        if value is None:
            continue
        try:
            return float(value)
        except (TypeError, ValueError):
            continue
    return None
