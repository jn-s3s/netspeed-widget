//! HTTPS throughput tests with a passive system-counter fallback.
//!
//! The active test downloads and uploads data at `speed.cloudflare.com`. If
//! either direction fails, a ten-second counter sample can supply a passive
//! estimate when at least one direction averages 1 Mbps or more.
//!
//! Active results measure throughput to that service. Passive results describe
//! incidental traffic across interfaces and must not be saved as measurements.

use std::io::Read;
use std::time::{Duration, Instant};

use crate::logger;
use crate::sampler::read_network_counters;

/// How long the passive estimate watches the counters.
const PASSIVE_SAMPLE_SEC: u64 = 10;
/// The passive estimate reports nothing below this rate.
const PASSIVE_MIN_MBPS: f64 = 1.0;
/// Requested download payload size in bytes.
const HTTP_DOWNLOAD_BYTES: u64 = 25_000_000;
/// Maximum bytes the HTTP upload test pushes after its small calibration.
const HTTP_UPLOAD_BYTES: usize = 8 * 1024 * 1024;
const HTTP_UPLOAD_PROBE_BYTES: usize = 256 * 1024;
/// Minimum received bytes needed for a download measurement.
const HTTP_MIN_BYTES: u64 = 1_000_000;

/// Provenance of the HTTP backend.
pub const SOURCE_HTTP: &str = "http-transfer";
/// Provenance of the passive estimate.
pub const SOURCE_PASSIVE: &str = "passive-estimate";

/// Active throughput or a passive estimate in megabits per second.
///
/// `source` identifies the backend. [`SOURCE_PASSIVE`] marks incidental traffic;
/// callers use [`SpeedtestResult::measured`] to decide whether to persist it.
#[derive(Clone, Debug)]
pub struct SpeedtestResult {
    pub down_mbps: f64,
    pub up_mbps: f64,
    pub source: &'static str,
}

impl SpeedtestResult {
    /// Returns whether the source is anything other than [`SOURCE_PASSIVE`].
    pub fn measured(&self) -> bool {
        self.source != SOURCE_PASSIVE
    }
}

/// Tries the HTTPS test, then the passive estimate, returning the first result.
///
/// # Errors
/// Returns a message listing the attempted backends if neither produces a result.
pub fn measure_speed() -> Result<SpeedtestResult, String> {
    let mut tried: Vec<&str> = Vec::new();
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

/// Measures download and upload throughput over HTTPS at `speed.cloudflare.com`.
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

/// Requests a fixed payload and divides received bits by elapsed seconds.
///
/// A read failure still yields a result if at least [`HTTP_MIN_BYTES`] arrived.
/// Elapsed time includes connection setup and any wait before a read failure.
///
/// # Errors
/// Returns an error if the request fails or fewer than [`HTTP_MIN_BYTES`] arrive.
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
///
/// Uploads 256 KiB first, then up to 8 MiB more. If the second upload fails,
/// the completed calibration supplies the result.
///
/// # Errors
/// Returns an error if the calibration upload fails.
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

/// Estimates throughput from counter deltas over ten seconds.
///
/// Returns `None` if either counter read fails or both directions average less
/// than [`PASSIVE_MIN_MBPS`]. This observes existing traffic without generating a load.
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
    fn the_passive_provider_is_not_measured() {
        let result = SpeedtestResult {
            down_mbps: 1.0,
            up_mbps: 1.0,
            source: SOURCE_PASSIVE,
        };
        assert!(!result.measured());
    }
}
