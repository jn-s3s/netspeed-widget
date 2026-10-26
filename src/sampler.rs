//! Background network throughput sampling.
//!
//! A worker queries counters and updates a locked sample history. The UI copies
//! snapshots from that history without waiting for a counter query.
//!
//! Windows IP Helper supplies cumulative octet totals, summed across every
//! interface. Traffic reported on multiple interfaces can be counted more than
//! once; the totals are not restricted to Internet traffic.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

use crate::clock::sleep_until;
use crate::logger;

/// One throughput snapshot in megabits per second.
#[derive(Clone, Copy, Debug)]
pub struct NetSample {
    pub down_mbps: f64,
    pub up_mbps: f64,
    /// Wall clock time of the sample, in epoch seconds.
    pub ts: f64,
}

/// The mutable part of the sampler, guarded by one lock.
struct State {
    samples: VecDeque<NetSample>,
    session_down_bytes: u64,
    session_up_bytes: u64,
    last_sent: Option<u64>,
    last_recv: Option<u64>,
    last_ts: Option<Instant>,
}

/// Samples system network counters at a fixed interval.
pub struct NetSampler {
    interval: Duration,
    history: usize,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl NetSampler {
    /// Creates a sampler.
    ///
    /// `interval` is the time between samples and `history` the number of
    /// recent samples kept for graphing.
    pub fn new(interval: Duration, history: usize) -> Self {
        Self {
            interval,
            history,
            state: Arc::new(Mutex::new(State {
                samples: VecDeque::with_capacity(history),
                session_down_bytes: 0,
                session_up_bytes: 0,
                last_sent: None,
                last_recv: None,
                last_ts: None,
            })),
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    /// Starts the sampling thread once.
    pub fn start(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|err| err.into_inner());
        if worker.is_some() {
            return;
        }
        let interval = self.interval;
        let history = self.history;
        let stop = Arc::clone(&self.stop);
        let state = Arc::clone(&self.state);
        match std::thread::Builder::new()
            .name("net-sampler".to_string())
            .spawn(move || sample_loop(interval, history, state, stop))
        {
            Ok(handle) => *worker = Some(handle),
            Err(err) => logger::error(
                "sampler",
                &format!("could not start sampling thread: {err}"),
            ),
        }
    }

    /// Requests shutdown and waits for the sampling thread to exit.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let mut worker = self.worker.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(handle) = worker.take() {
            if handle.join().is_err() {
                logger::error("sampler", "sampling thread panicked");
            }
        }
    }

    /// Most recent sample, or `None` before the first tick.
    pub fn latest(&self) -> Option<NetSample> {
        let state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        state.samples.back().copied()
    }

    /// Copy of the rolling sample window, oldest first.
    pub fn history(&self) -> Vec<NetSample> {
        let state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        state.samples.iter().copied().collect()
    }

    /// Returns `(download, upload)` in decimal megabytes since the first baseline.
    pub fn session_totals(&self) -> (f64, f64) {
        let state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        (
            state.session_down_bytes as f64 / 1_000_000.0,
            state.session_up_bytes as f64 / 1_000_000.0,
        )
    }
}

/// Folds one counter reading into rates, session totals and history.
///
/// The first call after start only seeds the baseline. Deltas are clamped at
/// zero because an adapter reset can drop the counters below the previous
/// reading; without the clamp the widget would briefly show negative speeds
/// and the session totals would shrink.
fn record_sample(
    state: &Mutex<State>,
    history: usize,
    bytes_sent: u64,
    bytes_recv: u64,
    now: Instant,
) {
    let mut guard = state.lock().unwrap_or_else(|err| err.into_inner());
    let (last_sent, last_recv, last_ts) = match (guard.last_sent, guard.last_recv, guard.last_ts) {
        (Some(sent), Some(recv), Some(ts)) => (sent, recv, ts),
        _ => {
            guard.last_sent = Some(bytes_sent);
            guard.last_recv = Some(bytes_recv);
            guard.last_ts = Some(now);
            return;
        }
    };

    let elapsed = now.duration_since(last_ts).as_secs_f64().max(1e-6);
    let down_bytes = bytes_recv.saturating_sub(last_recv);
    let up_bytes = bytes_sent.saturating_sub(last_sent);
    let down_mbps = down_bytes as f64 * 8.0 / 1e6 / elapsed;
    let up_mbps = up_bytes as f64 * 8.0 / 1e6 / elapsed;

    guard.session_down_bytes = guard.session_down_bytes.saturating_add(down_bytes);
    guard.session_up_bytes = guard.session_up_bytes.saturating_add(up_bytes);
    guard.samples.push_back(NetSample {
        down_mbps,
        up_mbps,
        ts: crate::now_epoch(),
    });
    while guard.samples.len() > history {
        guard.samples.pop_front();
    }

    guard.last_sent = Some(bytes_sent);
    guard.last_recv = Some(bytes_recv);
    guard.last_ts = Some(now);
}

/// Cumulative received and sent octets summed across interfaces.
pub struct NetworkCounters {
    pub received: u64,
    pub sent: u64,
}

/// Sums octet totals across every interface, returning `None` if the query fails.
pub fn read_network_counters() -> Option<NetworkCounters> {
    // SAFETY: GetIfTable2 initializes the table pointer. A successful allocation
    // contains NumEntries rows, borrowed only until its matching FreeMibTable.
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        let status = GetIfTable2(&mut table);
        if status.0 != 0 || table.is_null() {
            if !table.is_null() {
                FreeMibTable(table.cast());
            }
            logger::warning(
                "sampler",
                &format!("GetIfTable2 failed with status {}", status.0),
            );
            return None;
        }
        let header = &*table;
        let rows = std::slice::from_raw_parts(header.Table.as_ptr(), header.NumEntries as usize);
        let mut received: u64 = 0;
        let mut sent: u64 = 0;
        for row in rows {
            received = received.saturating_add(row.InOctets);
            sent = sent.saturating_add(row.OutOctets);
        }
        FreeMibTable(table as *const std::ffi::c_void);
        Some(NetworkCounters { received, sent })
    }
}

/// The sampling loop, running on its own thread.
fn sample_loop(
    interval: Duration,
    history: usize,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
) {
    if let Some(counters) = read_network_counters() {
        record_sample(
            &state,
            history,
            counters.sent,
            counters.received,
            Instant::now(),
        );
    }

    while !stop.load(Ordering::Relaxed) {
        sleep_until(&stop, interval);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let current = match read_network_counters() {
            Some(value) => value,
            None => {
                continue;
            }
        };
        record_sample(
            &state,
            history,
            current.sent,
            current.received,
            Instant::now(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampler_with_history(history: usize) -> NetSampler {
        NetSampler::new(Duration::from_secs(1), history)
    }

    #[test]
    fn counters_are_readable_on_this_machine() {
        assert!(read_network_counters().is_some());
    }

    #[test]
    fn unequal_counter_deltas_preserve_direction_units_and_totals() {
        let sampler = sampler_with_history(4);
        let start = Instant::now();
        record_sample(&sampler.state, 4, 100, 200, start);
        record_sample(
            &sampler.state,
            4,
            1_000_100,
            2_000_200,
            start + Duration::from_secs(1),
        );
        let sample = sampler.latest().unwrap();
        assert_eq!(sample.down_mbps, 16.0);
        assert_eq!(sample.up_mbps, 8.0);
        assert_eq!(sampler.session_totals(), (2.0, 1.0));
    }

    #[test]
    fn the_first_sample_only_seeds_the_baseline() {
        let sampler = sampler_with_history(4);
        let state = Arc::clone(&sampler.state);
        record_sample(&state, 4, 100, 200, Instant::now());
        assert!(sampler.latest().is_none());
        assert_eq!(sampler.session_totals(), (0.0, 0.0));
    }

    #[test]
    fn a_counter_reset_does_not_produce_negative_speeds() {
        let sampler = sampler_with_history(4);
        let state = Arc::clone(&sampler.state);
        let start = Instant::now();
        record_sample(&state, 4, 1_000, 1_000, start);
        record_sample(&state, 4, 10, 10, start + Duration::from_secs(1));
        let sample = sampler.latest().unwrap();
        assert!(sample.down_mbps >= 0.0);
        assert!(sample.up_mbps >= 0.0);
        assert_eq!(sampler.session_totals(), (0.0, 0.0));
    }

    #[test]
    fn the_history_window_is_bounded() {
        let sampler = sampler_with_history(3);
        let state = Arc::clone(&sampler.state);
        let start = Instant::now();
        record_sample(&state, 3, 0, 0, start);
        for index in 1..10 {
            record_sample(
                &state,
                3,
                index * 100,
                index * 100,
                start + Duration::from_secs(index),
            );
        }
        assert_eq!(sampler.history().len(), 3);
    }
}
