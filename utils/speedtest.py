"""Speedtest measurement with pluggable backends.

Providers are tried in order and the first successful result wins:

1. fast-cli bundled with the app (Node runtime under third_party)
2. fast-cli or fast on PATH
3. the speedtest-cli Python package (imported lazily for fast startup)
4. a passive estimate from psutil counter deltas
"""

import json
import os
import shutil
import subprocess
import time
from dataclasses import dataclass
from typing import Any, Callable

import psutil

from utils.logger import info, warn
from utils.paths import resource_path

FAST_TIMEOUT_SEC = 180
PASSIVE_SAMPLE_SEC = 10


@dataclass(frozen=True)
class SpeedtestResult:
    """Measured throughput in megabits per second."""

    down_mbps: float
    up_mbps: float


def measure_speed() -> SpeedtestResult:
    """Try each provider in order and return the first success.

    Raises:
        RuntimeError: If every provider fails.
    """
    providers: tuple[Callable[[], SpeedtestResult | None], ...] = (
        _measure_fast_cli,
        _measure_python_speedtest,
        _measure_passive_estimate,
    )
    for provider in providers:
        try:
            result = provider()
        except Exception as err:
            warn(f"[SPEEDTEST] {provider.__name__} raised: {err}")
            continue
        if result is not None:
            return result
    raise RuntimeError("all speedtest providers failed")


def _measure_fast_cli() -> SpeedtestResult | None:
    """Measure with fast.com via the bundled Node bundle or PATH."""
    data = _run_fast_cli()
    down, up = _parse_fast_result(data)
    if down is None or up is None:
        warn("[SPEEDTEST] fast-cli unavailable, trying next backend")
        return None
    return SpeedtestResult(down_mbps=down, up_mbps=up)


def _measure_python_speedtest() -> SpeedtestResult | None:
    """Measure with the speedtest-cli package via its in-process API."""
    try:
        import speedtest as speedtest_module
    except ImportError:
        warn("[SPEEDTEST] speedtest-cli not installed, trying next backend")
        return None

    info("[SPEEDTEST] Backend: speedtest-cli (python module)")
    try:
        tester = speedtest_module.Speedtest()
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
        )
    except Exception as err:
        warn(f"[SPEEDTEST] speedtest-cli failed: {err}")
        return None


def _measure_passive_estimate() -> SpeedtestResult | None:
    """Estimate throughput by sampling OS counters for a few seconds."""
    info("[SPEEDTEST] Backend: psutil (passive fallback)")
    start = time.time()
    first = psutil.net_io_counters()
    time.sleep(PASSIVE_SAMPLE_SEC)
    second = psutil.net_io_counters()

    elapsed = max(time.time() - start, 1e-6)
    return SpeedtestResult(
        down_mbps=(second.bytes_recv - first.bytes_recv) * 8.0 / elapsed / 1e6,
        up_mbps=(second.bytes_sent - first.bytes_sent) * 8.0 / elapsed / 1e6,
    )

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
    except Exception as err:
        warn(f"[SPEEDTEST] could not tune upload sizes: {err}")


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
    """Execute the bundled node.exe + fast-cli cli.js with --json."""
    info("[SPEEDTEST] Backend: fast-cli (bundled Node)")
    node_exe = resource_path(os.path.join("third_party", "node", "node.exe"))
    cli_js = resource_path(
        os.path.join(
            "third_party",
            "fast-bundle",
            "node_modules",
            "fast-cli",
            "distribution",
            "cli.js",
        )
    )
    bundle_cwd = resource_path(os.path.join("third_party", "fast-bundle"))

    if not (os.path.isfile(node_exe) and os.path.isfile(cli_js)):
        return None

    try:
        process = subprocess.run(
            [node_exe, cli_js, "--upload", "--json"],
            cwd=bundle_cwd,
            capture_output=True,
            text=True,
            timeout=FAST_TIMEOUT_SEC,
            check=True,
            **spawn_kw,
        )
        return json.loads(process.stdout.strip() or "{}")
    except (OSError, subprocess.SubprocessError, json.JSONDecodeError) as err:
        warn(f"[SPEEDTEST] bundled fast-cli failed: {err}")
        return None


def _run_path_fast(**spawn_kw: Any) -> dict | None:
    """Execute fast or fast-cli from PATH with --json."""
    info("[SPEEDTEST] Backend: fast-cli (PATH)")
    on_path = shutil.which("fast") or shutil.which("fast-cli")
    if not on_path:
        return None

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
        warn(f"[SPEEDTEST] PATH fast-cli failed: {err}")
        return None


def _parse_fast_result(data: dict | None) -> tuple[float | None, float | None]:
    """Parse fast.com JSON emitted by fast-cli."""
    if not isinstance(data, dict):
        return None, None

    candidates = [
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

