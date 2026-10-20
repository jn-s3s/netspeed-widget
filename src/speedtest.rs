//! Speedtest measurement with pluggable backends.
//!
//! Providers are tried in order and the first successful result wins:
//!
//! 1. fast-cli bundled with the app (Node runtime under `third_party`)
//! 2. fast-cli or `fast` on `PATH`
//! 3. a plain HTTP transfer test, which needs no extra runtime
//! 4. a passive estimate from the system counter deltas
//!
//! Only the first three measure the link, so only they are kept as a result.
//! The passive provider reports what traffic happened to cross the wire,
//! which is meaningless on an idle connection, so it returns `None` unless it
//! observed a real load and its outcome is labelled as an estimate wherever
//! it appears.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use windows::core::PCWSTR;

use crate::logger;
use crate::paths::{bundled_node_path, fast_bundle_dir, fast_cli_entry_path};
use crate::sampler::read_network_counters;

/// How long the fast-cli backends may take.
const FAST_TIMEOUT_SEC: u64 = 180;
/// How long the passive estimate watches the counters.
const PASSIVE_SAMPLE_SEC: u64 = 10;
/// The passive estimate reports nothing below this rate.
const PASSIVE_MIN_MBPS: f64 = 1.0;
/// How many bytes the HTTP download test pulls.
const HTTP_DOWNLOAD_BYTES: u64 = 25_000_000;
/// Maximum bytes the HTTP upload test pushes after its small calibration.
const HTTP_UPLOAD_BYTES: usize = 8 * 1024 * 1024;
const HTTP_UPLOAD_PROBE_BYTES: usize = 256 * 1024;
/// The smallest transfer that counts as a measurement.
const HTTP_MIN_BYTES: u64 = 1_000_000;

/// Provenance of the fast-cli backends.
pub const SOURCE_FAST: &str = "fast-cli";
/// Provenance of the HTTP backend.
pub const SOURCE_HTTP: &str = "http-transfer";
/// Provenance of the passive estimate.
pub const SOURCE_PASSIVE: &str = "passive-estimate";

/// Measured throughput in megabits per second, with its provenance.
///
/// `source` names the provider that produced the number. Only
/// [`SOURCE_PASSIVE`] is an observation of incidental traffic rather than a
/// measurement, which is what tells the caller whether to persist it.
#[derive(Clone, Debug)]
pub struct SpeedtestResult {
    pub down_mbps: f64,
    pub up_mbps: f64,
    pub source: &'static str,
}

impl SpeedtestResult {
    /// True when a provider probed the link rather than watched it.
    pub fn measured(&self) -> bool {
        self.source != SOURCE_PASSIVE
    }
}

/// Tries each provider in order and returns the first success.
///
/// Returns the list of providers that were tried when every one failed.
pub fn measure_speed() -> Result<SpeedtestResult, String> {
    let mut tried: Vec<&str> = Vec::new();
    if let Some(result) = measure_fast_cli() {
        return Ok(result);
    }
    tried.push(SOURCE_FAST);

    if let Some(result) = measure_http() {
        return Ok(result);
    }
    tried.push(SOURCE_HTTP);

    if let Some(result) = measure_passive_estimate() {
        return Ok(result);
    }
    tried.push(SOURCE_PASSIVE);

    Err(format!(
        "no speedtest result; every backend failed or gave nothing: {}",
        tried.join(", ")
    ))
}

/// Measures with fast.com through the bundled Node bundle or `PATH`.
fn measure_fast_cli() -> Option<SpeedtestResult> {
    let data = run_fast_cli()?;
    let (down, up) = parse_fast_result(&data)?;
    Some(SpeedtestResult {
        down_mbps: down,
        up_mbps: up,
        source: SOURCE_FAST,
    })
}

/// Measures with a plain HTTP transfer against a public speed test host.
fn measure_http() -> Option<SpeedtestResult> {
    logger::info("speedtest", "backend: http transfer test");
    let down = match http_download_speed() {
        Ok(value) => value,
        Err(err) => {
            logger::warning("speedtest", &err);
            return None;
        }
    };
    let up = match http_upload_speed() {
        Ok(value) => value,
        Err(err) => {
            logger::warning("speedtest", &err);
            return None;
        }
    };
    Some(SpeedtestResult {
        down_mbps: down,
        up_mbps: up,
        source: SOURCE_HTTP,
    })
}

/// Downloads a fixed payload and reports the rate it arrived at.
///
/// A transfer that runs into the deadline still counts: what arrived in the
/// time is a valid measurement, and throwing it away would lose the result
/// on a slow link.
fn http_download_speed() -> Result<f64, String> {
    let url = format!("https://speed.cloudflare.com/__down?bytes={HTTP_DOWNLOAD_BYTES}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .timeout(Duration::from_secs(45))
        .build();
    let start = Instant::now();
    let response = agent
        .get(&url)
        .call()
        .map_err(|err| format!("http download failed: {err}"))?;
    let mut reader = response.into_reader();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => total = total.saturating_add(count as u64),
            Err(err) => {
                if total < HTTP_MIN_BYTES {
                    return Err(format!("http download failed: {err}"));
                }
                logger::warning(
                    "speedtest",
                    &format!("http download stopped early after {total} bytes: {err}"),
                );
                break;
            }
        }
    }
    if total < HTTP_MIN_BYTES {
        return Err("http download returned nothing".to_string());
    }
    let elapsed = start.elapsed().as_secs_f64().max(1e-6);
    Ok(total as f64 * 8.0 / 1e6 / elapsed)
}

/// Calibrates with a small upload before choosing a roughly four-second payload.
fn http_upload_speed() -> Result<f64, String> {
    let url = "https://speed.cloudflare.com/__up";
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .timeout(Duration::from_secs(45))
        .build();
    adaptive_upload_speed(|bytes| {
        let payload = vec![0u8; bytes];
        let start = Instant::now();
        agent
            .post(url)
            .send_bytes(&payload)
            .map_err(|err| format!("http upload failed: {err}"))?;
        Ok(start.elapsed())
    })
}

fn adaptive_upload_speed(
    mut upload: impl FnMut(usize) -> Result<Duration, String>,
) -> Result<f64, String> {
    let elapsed = upload(HTTP_UPLOAD_PROBE_BYTES)?.as_secs_f64().max(1e-6);
    let bytes_per_sec = HTTP_UPLOAD_PROBE_BYTES as f64 / elapsed;
    let bytes = ((bytes_per_sec * 4.0) as usize).clamp(HTTP_UPLOAD_PROBE_BYTES, HTTP_UPLOAD_BYTES);
    if bytes == HTTP_UPLOAD_PROBE_BYTES {
        return Ok(bytes_per_sec * 8.0 / 1e6);
    }
    match upload(bytes) {
        Ok(elapsed) => Ok(bytes as f64 * 8.0 / 1e6 / elapsed.as_secs_f64().max(1e-6)),
        Err(err) => {
            // A changing link must not discard the completed calibration upload.
            logger::warning("speedtest", &err);
            Ok(bytes_per_sec * 8.0 / 1e6)
        }
    }
}

/// Estimates throughput from the system counters, only when real traffic ran.
///
/// Returns `None` on an idle window. Counting bytes that nobody asked to
/// move would otherwise publish a near-zero figure as the user's line rate.
fn measure_passive_estimate() -> Option<SpeedtestResult> {
    logger::info(
        "speedtest",
        &format!("backend: passive estimate over {PASSIVE_SAMPLE_SEC}s"),
    );
    let start = Instant::now();
    let first = read_network_counters()?;
    std::thread::sleep(Duration::from_secs(PASSIVE_SAMPLE_SEC));
    let second = read_network_counters()?;

    let elapsed = start.elapsed().as_secs_f64().max(1e-6);
    let down_bytes = second.received.saturating_sub(first.received);
    let up_bytes = second.sent.saturating_sub(first.sent);
    let down = down_bytes as f64 * 8.0 / elapsed / 1e6;
    let up = up_bytes as f64 * 8.0 / elapsed / 1e6;
    if down.max(up) < PASSIVE_MIN_MBPS {
        logger::warning(
            "speedtest",
            &format!(
                "passive estimate saw no usable traffic in {elapsed:.1}s ({down:.2} down, {up:.2} up Mb/s)"
            ),
        );
        return None;
    }
    Some(SpeedtestResult {
        down_mbps: down,
        up_mbps: up,
        source: SOURCE_PASSIVE,
    })
}

/// Runs fast.com via the bundled Node first, then via `PATH`.
fn run_fast_cli() -> Option<Value> {
    run_bundled_fast().or_else(run_path_fast)
}

/// Executes the bundled `node.exe` and the fast-cli entry script.
fn run_bundled_fast() -> Option<Value> {
    let node_exe = bundled_node_path();
    let cli_js = fast_cli_entry_path();
    if !node_exe.is_file() || !cli_js.is_file() {
        return None;
    }
    logger::info(
        "speedtest",
        &format!("backend: fast-cli (bundled Node at {})", node_exe.display()),
    );
    let output = run_with_timeout(
        Command::new(&node_exe)
            .arg(&cli_js)
            .args(["--upload", "--json"])
            .current_dir(fast_bundle_dir()),
        Duration::from_secs(FAST_TIMEOUT_SEC),
    )?;
    parse_fast_output(&output)
}

/// Executes `fast` or `fast-cli` from `PATH`.
fn run_path_fast() -> Option<Value> {
    let on_path = ["fast", "fast-cli"]
        .iter()
        .find_map(|name| find_on_path(name))?;
    logger::info(
        "speedtest",
        &format!("backend: fast-cli (PATH at {on_path})"),
    );
    let output = run_with_timeout(
        Command::new(&on_path).args(["--upload", "--json"]),
        Duration::from_secs(FAST_TIMEOUT_SEC),
    )?;
    parse_fast_output(&output)
}

/// Parses the JSON fast-cli printed on stdout.
fn parse_fast_output(output: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(output);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        logger::warning("speedtest", "fast-cli returned empty output");
        return None;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => Some(value),
        Err(err) => {
            logger::warning("speedtest", &format!("fast-cli gave no usable JSON: {err}"));
            None
        }
    }
}

/// Extracts the download and upload rates from the fast.com JSON.
fn parse_fast_result(data: &Value) -> Option<(f64, f64)> {
    let object = data.as_object()?;
    let mut candidates: Vec<(&Value, &[&str], &[&str])> = vec![(
        data,
        &["downloadSpeed", "download"],
        &["uploadSpeed", "upload"],
    )];
    if let Some(speeds) = object.get("speeds") {
        candidates.push((speeds, &["download"], &["upload"]));
    }
    for (source, down_keys, up_keys) in candidates {
        let down = first_float(source, down_keys);
        let up = first_float(source, up_keys);
        if let (Some(down), Some(up)) = (down, up) {
            return Some((down, up));
        }
    }
    logger::warning(
        "speedtest",
        "fast-cli gave no usable speeds, trying next backend",
    );
    None
}

/// Returns the first value under any key that converts to a number.
fn first_float(data: &Value, keys: &[&str]) -> Option<f64> {
    let object = data.as_object()?;
    for key in keys {
        if let Some(value) = object.get(*key) {
            if let Some(number) = value.as_f64() {
                if number.is_finite() {
                    return Some(number);
                }
            }
        }
    }
    None
}

/// Runs a command with a deadline, killing it when the deadline passes.
fn run_with_timeout(command: &mut Command, timeout: Duration) -> Option<Vec<u8>> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;

    // The OS closes this handle even when the widget exits mid-test. Descendants
    // then die with the backend instead of surviving their parent for minutes.
    struct Job(HANDLE);
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let job = match unsafe { CreateJobObjectW(None, PCWSTR::null()) } {
        Ok(handle) => Job(handle),
        Err(err) => {
            logger::warning("speedtest", &format!("could not create backend job: {err}"));
            return None;
        }
    };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if let Err(err) = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } {
        logger::warning(
            "speedtest",
            &format!("could not configure backend job: {err}"),
        );
        return None;
    }
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            logger::warning("speedtest", &format!("could not start backend: {err}"));
            return None;
        }
    };
    if let Err(err) = unsafe { AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) } {
        logger::warning(
            "speedtest",
            &format!("could not contain backend process: {err}"),
        );
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    // std::process does not expose the primary thread handle on stable Rust.
    // The suspended process has not executed user code or spawned descendants.
    if let Err(err) = resume_backend(child.id()) {
        logger::warning(
            "speedtest",
            &format!("could not resume contained backend: {err}"),
        );
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let output = std::thread::Builder::new()
        .name("speedtest-output".into())
        .spawn(move || drain_output(stdout));
    let errors = std::thread::Builder::new()
        .name("speedtest-errors".into())
        .spawn(move || drain_output(stderr));
    let (output, errors) = match (output, errors) {
        (Ok(output), Ok(errors)) => (output, errors),
        _ => {
            logger::warning("speedtest", "could not start backend output readers");
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    logger::warning("speedtest", "the speedtest backend timed out");
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                logger::warning("speedtest", &format!("the speedtest backend failed: {err}"));
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    drop(job); // Close inherited pipes held by any lingering descendants.
    let stdout = output.join().ok().and_then(Result::ok);
    let stderr = errors.join().ok().and_then(Result::ok).unwrap_or_default();
    let status = status?;
    if !status.success() {
        let snippet: String = String::from_utf8_lossy(&stderr)
            .chars()
            .take(512)
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect();
        logger::warning(
            "speedtest",
            &format!("backend exited with {status}: {snippet}"),
        );
        return None;
    }
    if stdout.is_none() {
        logger::warning("speedtest", "could not read backend stdout");
    }
    stdout
}

/// Resume the initial thread only after assigning its process to our job.
fn resume_backend(process_id: u32) -> windows::core::Result<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)?;
        let result = (|| {
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            Thread32First(snapshot, &mut entry)?;
            loop {
                if entry.th32OwnerProcessID == process_id {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)?;
                    let resumed = ResumeThread(thread);
                    let error = if resumed == u32::MAX {
                        Some(windows::core::Error::from_win32())
                    } else {
                        None
                    };
                    let _ = CloseHandle(thread);
                    return error.map_or(Ok(()), Err);
                }
                Thread32Next(snapshot, &mut entry)?;
            }
        })();
        let _ = CloseHandle(snapshot);
        result
    }
}

/// Drain continuously to avoid pipe deadlock, retaining only a bounded prefix.
fn drain_output(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut saved = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(saved);
        }
        let keep = count.min(65536_usize.saturating_sub(saved.len()));
        saved.extend_from_slice(&buffer[..keep]);
    }
}

/// Finds an executable on `PATH`.
fn find_on_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(format!("{name}.exe"));
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_adapts_to_a_one_megabit_uplink_without_hitting_the_deadline() {
        let mut transfers = Vec::new();
        let speed = adaptive_upload_speed(|bytes| {
            let seconds = bytes as f64 * 8.0 / 1e6;
            assert!(seconds < 45.0);
            transfers.push(bytes);
            Ok(Duration::from_secs_f64(seconds))
        })
        .unwrap();
        assert!((speed - 1.0).abs() < 0.001);
        assert_eq!(transfers, [HTTP_UPLOAD_PROBE_BYTES, 500_000]);
    }

    #[test]
    fn upload_keeps_the_completed_probe_if_the_link_slows_down() {
        let mut calls = 0;
        let speed = adaptive_upload_speed(|_| {
            calls += 1;
            if calls == 1 {
                Ok(Duration::from_secs(1))
            } else {
                Err("deadline exceeded".to_string())
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(speed, HTTP_UPLOAD_PROBE_BYTES as f64 * 8.0 / 1e6);
        assert!(adaptive_upload_speed(|_| Err("offline".to_string())).is_err());
    }

    #[test]
    fn upload_sizes_stay_bounded_for_fast_and_very_slow_links() {
        let mut transfers = Vec::new();
        adaptive_upload_speed(|bytes| {
            transfers.push(bytes);
            Ok(Duration::from_millis(1))
        })
        .unwrap();
        assert_eq!(transfers, [HTTP_UPLOAD_PROBE_BYTES, HTTP_UPLOAD_BYTES]);
        let mut calls = 0;
        adaptive_upload_speed(|bytes| {
            calls += 1;
            assert_eq!(bytes, HTTP_UPLOAD_PROBE_BYTES);
            Ok(Duration::from_secs(10))
        })
        .unwrap();
        assert_eq!(calls, 1);
    }

    #[test]
    fn subprocess_output_failure_and_timeout_are_bounded() {
        let mut directory = [0_u16; 32768];
        let length = unsafe {
            windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut directory))
        } as usize;
        assert!(length > 0 && length < directory.len());
        let shell = std::path::PathBuf::from(String::from_utf16_lossy(&directory[..length]))
            .join("WindowsPowerShell/v1.0/powershell.exe");
        let run = |script: &str, timeout| {
            run_with_timeout(
                Command::new(&shell).args(["-NoProfile", "-NonInteractive", "-Command", script]),
                timeout,
            )
        };
        let output = run(
            "[Console]::Out.Write(('x' * 131072)); [Console]::Error.Write(('e' * 131072))",
            Duration::from_secs(15),
        )
        .unwrap();
        assert_eq!(output, vec![b'x'; 65536]);
        assert!(run(
            "[Console]::Error.Write('expected test failure'); exit 7",
            Duration::from_secs(15)
        )
        .is_none());
        let start = Instant::now();
        assert!(run("Start-Sleep -Seconds 30", Duration::from_millis(100)).is_none());
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_fast_json_shape_is_understood() {
        let data: Value =
            serde_json::from_str(r#"{"downloadSpeed": 120.5, "uploadSpeed": 40.25}"#).unwrap();
        let (down, up) = parse_fast_result(&data).unwrap();
        assert!((down - 120.5).abs() < 0.01);
        assert!((up - 40.25).abs() < 0.01);
    }

    #[test]
    fn the_nested_fast_json_shape_is_understood() {
        let data: Value =
            serde_json::from_str(r#"{"speeds": {"download": 10.0, "upload": 5.0}}"#).unwrap();
        let (down, up) = parse_fast_result(&data).unwrap();
        assert!((down - 10.0).abs() < 0.01);
        assert!((up - 5.0).abs() < 0.01);
    }

    #[test]
    fn a_json_without_speeds_is_rejected() {
        let data: Value = serde_json::from_str(r#"{"error": "nope"}"#).unwrap();
        assert!(parse_fast_result(&data).is_none());
    }

    #[test]
    fn the_passive_provider_is_not_measured() {
        let result = SpeedtestResult {
            down_mbps: 1.0,
            up_mbps: 1.0,
            source: SOURCE_PASSIVE,
        };
        assert!(!result.measured());
    }
}
