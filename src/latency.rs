//! Latency probing.
//!
//! A TCP handshake to a well-known host is a solid latency estimate and is
//! far cheaper than spawning `ping.exe` every second. A single ICMP ping is
//! kept as a second opinion, used only when TCP fails, to tell a real outage
//! apart from a blocked port.
//!
//! The target is an anycast address rather than a hostname. Anycast means the
//! packet is answered by the nearest of thousands of identical endpoints, so
//! the number tracks the distance to the ISP instead of the load-balancing
//! choice of one CDN, and an IP literal skips the DNS lookup that would
//! otherwise fold resolver latency into every reading.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::clock::sleep_until;
use crate::logger;

/// Anycast address used for the TCP probe.
pub const PING_HOST: &str = "1.1.1.1";
/// Port used for the TCP probe; 443 passes through most firewalls.
pub const PING_PORT: u16 = 443;
const PROBE_ADDRESS: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 1, 1, 1), PING_PORT));

/// One latency measurement.
///
/// `ms` is `None` when TCP failed but ICMP still reached the host. `ok` is
/// `false` when every probe failed, meaning likely offline.
#[derive(Clone, Copy, Debug)]
pub struct LatencyResult {
    pub ms: Option<f64>,
    pub ok: bool,
    pub ts: f64,
}

impl Default for LatencyResult {
    fn default() -> Self {
        Self {
            ms: None,
            ok: true,
            ts: 0.0,
        }
    }
}

/// TCP latency at or above this threshold is a warning.
pub const LATENCY_GOOD_MS: f64 = 80.0;

/// Presentation overrides never change recorded probe health.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DisplayState {
    Healthy(f64),
    Slow(f64),
    Check,
    Failed,
    Waiting,
    Stale,
    Testing,
}

impl DisplayState {
    /// Resolve temporary overrides before considering probe health.
    pub fn derive(result: LatencyResult, now: f64, testing: bool) -> Self {
        if testing {
            Self::Testing
        } else if result.ts == 0.0 {
            Self::Waiting
        } else if now - result.ts > 10.0 {
            Self::Stale
        } else if !result.ok {
            Self::Failed
        } else {
            match result.ms {
                Some(ms) if ms < LATENCY_GOOD_MS => Self::Healthy(ms),
                Some(ms) => Self::Slow(ms),
                None => Self::Check,
            }
        }
    }

    /// Short, explicit status for every widget layout.
    pub fn label(self) -> String {
        match self {
            Self::Healthy(ms) => latency_label(ms),
            Self::Slow(_) => "SLOW".to_string(),
            Self::Check => "CHECK".to_string(),
            Self::Failed => "FAIL".to_string(),
            Self::Waiting | Self::Stale => "—".to_string(),
            Self::Testing => "TEST".to_string(),
        }
    }

    /// Shared hover and details explanation; failure refers only to these probes.
    pub fn explanation(self) -> String {
        match self {
            Self::Healthy(ms) => format!("TCP latency: {ms:.0} ms."),
            Self::Slow(ms) => {
                format!("High latency: {ms:.0} ms. Warning threshold: {LATENCY_GOOD_MS:.0} ms.")
            }
            Self::Check => {
                "TCP probe failed; ICMP responded. TCP latency is unavailable.".to_string()
            }
            Self::Failed => format!("TCP and ICMP probes to {PING_HOST} failed."),
            Self::Waiting => "Waiting for a latency result".to_string(),
            Self::Stale => "Latency result is stale.".to_string(),
            Self::Testing => {
                "Speed test running; connection alerts are temporarily suppressed.".to_string()
            }
        }
    }
}

/// Bounded latency text keeps the reserved status column stable.
pub fn latency_label(ms: f64) -> String {
    if ms < 999.5 {
        format!("{ms:.0} ms")
    } else {
        ">1 s".to_string()
    }
}

/// Compact readout, independent of window state and wall-clock access.
pub fn ping_label(result: LatencyResult, now: f64, testing: bool) -> String {
    DisplayState::derive(result, now, testing).label()
}

/// Measures latency on its own thread.
pub struct LatencyProbe {
    interval: Duration,
    timeout: Duration,
    latest: Arc<Mutex<LatencyResult>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl LatencyProbe {
    /// Creates a probe.
    pub fn new(interval: Duration, timeout: Duration) -> Self {
        Self {
            interval,
            timeout,
            latest: Arc::new(Mutex::new(LatencyResult::default())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    /// Starts the probe thread once.
    pub fn start(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|err| err.into_inner());
        if worker.is_some() {
            return;
        }
        let interval = self.interval;
        let timeout = self.timeout;
        let latest = Arc::clone(&self.latest);
        let stop = Arc::clone(&self.stop);
        match std::thread::Builder::new()
            .name("latency-probe".to_string())
            .spawn(move || probe_loop(interval, timeout, latest, stop))
        {
            Ok(handle) => *worker = Some(handle),
            Err(err) => logger::error("net", &format!("could not start latency thread: {err}")),
        }
    }

    /// Signals the probe thread to exit.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let mut worker = self.worker.lock().unwrap_or_else(|err| err.into_inner());
        // The worker owns only Arc snapshots. Do not block the UI on a network
        // timeout during process shutdown; the stop flag prevents another probe.
        worker.take();
    }

    /// Most recent measurement.
    pub fn latest(&self) -> LatencyResult {
        *self.latest.lock().unwrap_or_else(|err| err.into_inner())
    }
}

/// The probe loop, running on its own thread.
fn probe_loop(
    interval: Duration,
    timeout: Duration,
    latest: Arc<Mutex<LatencyResult>>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        let result = measure(timeout, &stop);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let was_ok = {
            let mut guard = latest.lock().unwrap_or_else(|err| err.into_inner());
            let was_ok = guard.ok;
            *guard = result;
            was_ok
        };
        // File I/O must not hold the snapshot lock read by the UI thread.
        if result.ok && !was_ok {
            logger::info("net", "connectivity restored");
        } else if !result.ok && was_ok {
            logger::warning("net", "connectivity lost");
        }
        sleep_until(&stop, interval);
    }
}

/// Takes one measurement, falling back to ICMP when the TCP probe fails.
fn measure(timeout: Duration, stop: &AtomicBool) -> LatencyResult {
    let start = Instant::now();
    if let Ok(stream) = TcpStream::connect_timeout(&PROBE_ADDRESS, timeout) {
        drop(stream);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        return LatencyResult {
            ms: Some(ms),
            ok: true,
            ts: crate::now_epoch(),
        };
    }

    if !stop.load(Ordering::Relaxed) && icmp_ping() {
        return LatencyResult {
            ms: None,
            ok: true,
            ts: crate::now_epoch(),
        };
    }
    LatencyResult {
        ms: None,
        ok: false,
        ts: crate::now_epoch(),
    }
}

/// Single ICMP echo via the OS ping tool. Fallback path only.
fn icmp_ping() -> bool {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut directory = [0_u16; 32768];
    // SAFETY: The API receives a writable UTF-16 buffer and its length.
    let length = unsafe {
        windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut directory))
    } as usize;
    if length == 0 || length >= directory.len() {
        logger::warning("net", "could not resolve the Windows system directory");
        return false;
    }
    let ping = std::path::PathBuf::from(std::ffi::OsString::from_wide(&directory[..length]))
        .join("ping.exe");
    let result = std::process::Command::new(ping)
        .args([PING_HOST, "-n", "1", "-w", "1000"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(status) => status.success(),
        Err(err) => {
            logger::warning("net", &format!("icmp fallback could not run ping: {err}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_threshold_and_overrides_are_independent_of_old_probe_health() {
        let result = |ms, ok| LatencyResult { ms, ok, ts: 100.0 };
        assert_eq!(
            DisplayState::derive(result(Some(79.9), true), 101.0, false),
            DisplayState::Healthy(79.9)
        );
        assert_eq!(
            DisplayState::derive(result(Some(80.0), true), 101.0, false),
            DisplayState::Slow(80.0)
        );
        assert_eq!(
            DisplayState::derive(result(None, true), 101.0, false),
            DisplayState::Check
        );
        assert_eq!(
            DisplayState::derive(result(None, false), 101.0, false),
            DisplayState::Failed
        );
        for probe in [
            result(Some(120.0), true),
            result(None, true),
            result(None, false),
        ] {
            assert_eq!(
                DisplayState::derive(probe, 110.0, true),
                DisplayState::Testing
            );
            assert_eq!(
                DisplayState::derive(probe, 110.001, false),
                DisplayState::Stale
            );
            assert_eq!(
                DisplayState::derive(probe, 111.0, true),
                DisplayState::Testing
            );
        }
        assert_eq!(
            DisplayState::Slow(120.0).explanation(),
            "High latency: 120 ms. Warning threshold: 80 ms."
        );
        assert_eq!(
            DisplayState::Check.explanation(),
            "TCP probe failed; ICMP responded. TCP latency is unavailable."
        );
        assert_eq!(
            DisplayState::Failed.explanation(),
            "TCP and ICMP probes to 1.1.1.1 failed."
        );
        assert_eq!(latency_label(999.49), "999 ms");
        assert_eq!(latency_label(999.5), ">1 s");
    }

    #[test]
    fn tcp_and_icmp_probe_the_same_host() {
        assert_eq!(PROBE_ADDRESS.ip().to_string(), PING_HOST);
        assert_eq!(PROBE_ADDRESS.port(), PING_PORT);
    }

    #[test]
    fn latency_readout_covers_results_staleness_and_testing() {
        let result = |ms, ok| LatencyResult { ms, ok, ts: 100.0 };
        assert_eq!(ping_label(result(Some(25.0), true), 101.0, false), "25 ms");
        assert_eq!(ping_label(result(Some(1000.0), true), 101.0, false), "SLOW");
        assert_eq!(ping_label(result(None, true), 101.0, false), "CHECK");
        assert_eq!(ping_label(result(None, false), 101.0, false), "FAIL");
        assert_eq!(ping_label(result(None, false), 101.0, true), "TEST");
        assert_eq!(ping_label(LatencyResult::default(), 101.0, false), "—");
        assert_eq!(ping_label(result(Some(25.0), true), 111.0, false), "—");
        assert_eq!(ping_label(result(Some(25.0), true), 110.0, false), "25 ms");
    }

    #[test]
    fn the_worker_stops_without_needing_internet_access() {
        // Exercise the worker lifecycle without depending on Internet access.
        let probe = LatencyProbe::new(Duration::from_millis(10), Duration::from_millis(10));
        probe.start();
        probe.stop();
        assert!(probe.worker.lock().unwrap().is_none());
    }
}
