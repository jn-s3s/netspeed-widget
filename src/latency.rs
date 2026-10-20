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

/// Compact readout, independent of window state and wall-clock access.
pub fn ping_label(result: LatencyResult, now: f64, testing: bool) -> String {
    if testing {
        return "Test".to_string();
    }
    if result.ts == 0.0 || now - result.ts > 10.0 {
        return "—".to_string();
    }
    if !result.ok {
        return "Fail".to_string();
    }
    match result.ms {
        Some(ms) if ms < 1000.0 => format!("{ms:.0} ms"),
        Some(_) => ">1 s".to_string(),
        None => "ICMP".to_string(),
    }
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
        let handle = std::thread::Builder::new()
            .name("latency-probe".to_string())
            .spawn(move || probe_loop(interval, timeout, latest, stop))
            .ok();
        *worker = handle;
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
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut directory = [0_u16; 32768];
    let length = unsafe {
        windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut directory))
    } as usize;
    if length == 0 || length >= directory.len() {
        logger::warning("net", "could not resolve the Windows system directory");
        return false;
    }
    let ping =
        std::path::PathBuf::from(String::from_utf16_lossy(&directory[..length])).join("ping.exe");
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
    fn tcp_and_icmp_probe_the_same_host() {
        assert_eq!(PROBE_ADDRESS.ip().to_string(), PING_HOST);
        assert_eq!(PROBE_ADDRESS.port(), PING_PORT);
    }

    #[test]
    fn latency_readout_covers_results_staleness_and_testing() {
        let result = |ms, ok| LatencyResult { ms, ok, ts: 100.0 };
        assert_eq!(ping_label(result(Some(25.0), true), 101.0, false), "25 ms");
        assert_eq!(ping_label(result(Some(1000.0), true), 101.0, false), ">1 s");
        assert_eq!(ping_label(result(None, true), 101.0, false), "ICMP");
        assert_eq!(ping_label(result(None, false), 101.0, false), "Fail");
        assert_eq!(ping_label(result(None, false), 101.0, true), "Test");
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
