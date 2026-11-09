//! Background system resource sampling (CPU, memory, disk activity, and disk storage).
//!
//! A background worker queries Windows kernel system times, memory status,
//! performance counters, and disk space while resource meters are enabled.
//! The UI thread reads snapshots from this worker without blocking.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE,
    PDH_FMT_DOUBLE,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::clock::sleep_until;
use crate::logger;
use crate::win::gdi::to_wide;

/// Win32 `DRIVE_FIXED` return value from `GetDriveTypeW`.
const DRIVE_FIXED: u32 = 3;

/// Minimum interval between fixed-drive space queries.
const DRIVE_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Information for one mounted disk drive.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskDriveInfo {
    pub letter: char,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub used_ratio: f32,
}

/// One snapshot of system resource utilization.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemSnapshot {
    /// Overall CPU utilization ratio between 0.0 and 1.0.
    pub cpu_usage: f32,
    /// Physical memory utilization ratio between 0.0 and 1.0.
    pub memory_usage: f32,
    /// Used physical memory in bytes.
    pub memory_used_bytes: u64,
    /// Total physical memory in bytes.
    pub memory_total_bytes: u64,
    /// Active disk time (I/O) ratio between 0.0 and 1.0.
    pub disk_activity: f32,
    /// Overall / primary disk storage space utilization ratio between 0.0 and 1.0.
    pub disk_storage_ratio: f32,
    /// Mounted drives and their space usages.
    pub drives: Vec<DiskDriveInfo>,
}

/// RAII wrapper for a PDH query handle.
struct PdhDiskCounter {
    query: isize,
    counter: isize,
}

impl PdhDiskCounter {
    fn new() -> Option<Self> {
        let mut query = 0isize;
        // SAFETY: `query` is a valid writable out-pointer for the opened PDH query handle.
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let mut counter = 0isize;
        let counter_path = to_wide(r"\PhysicalDisk(_Total)\% Idle Time");
        // SAFETY: `query` is an open PDH query handle, `counter_path` is null-terminated UTF-16
        // that outlives the call, and `counter` is a valid writable out-pointer.
        let status =
            unsafe { PdhAddEnglishCounterW(query, PCWSTR(counter_path.as_ptr()), 0, &mut counter) };
        if status != 0 {
            // SAFETY: `query` was opened by `PdhOpenQueryW` above and is not used after closing.
            unsafe {
                let _ = PdhCloseQuery(query);
            }
            return None;
        }
        // Prime the rate counter with an initial baseline collection.
        // SAFETY: `query` is an open PDH query handle with an attached counter.
        unsafe {
            let _ = PdhCollectQueryData(query);
        }
        Some(Self { query, counter })
    }

    fn sample(&mut self) -> f32 {
        // SAFETY: `self.query` is an open PDH query handle owned by `self`.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return 0.0;
        }
        let mut val = PDH_FMT_COUNTERVALUE::default();
        // SAFETY: `self.counter` belongs to `self.query` and `val` is a valid writable out-pointer.
        let status =
            unsafe { PdhGetFormattedCounterValue(self.counter, PDH_FMT_DOUBLE, None, &mut val) };
        if status != 0
            || (val.CStatus != PDH_CSTATUS_VALID_DATA && val.CStatus != PDH_CSTATUS_NEW_DATA)
        {
            return 0.0;
        }
        // SAFETY: `PdhGetFormattedCounterValue` succeeded with `PDH_FMT_DOUBLE` and valid status,
        // so the `doubleValue` union field is initialized.
        let idle_pct = unsafe { val.Anonymous.doubleValue };
        if !idle_pct.is_finite() {
            return 0.0;
        }
        ((100.0 - idle_pct as f32) / 100.0).clamp(0.0, 1.0)
    }
}

impl Drop for PdhDiskCounter {
    fn drop(&mut self) {
        if self.query != 0 {
            // SAFETY: `self.query` is owned by `self` and closed at most once on drop.
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }
}

struct Worker {
    stop: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<Arc<SystemSnapshot>>>>,
}

/// Background system resources sampler.
pub struct SystemSampler {
    interval: Duration,
    worker: Mutex<Option<Worker>>,
}

impl SystemSampler {
    /// Creates a system resource sampler.
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            worker: Mutex::new(None),
        }
    }

    /// Starts the sampling thread if not already running.
    pub fn start(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|err| err.into_inner());
        if worker.is_some() {
            return;
        }
        let interval = self.interval;
        let stop = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(Mutex::new(None));
        let thread_stop = Arc::clone(&stop);
        let thread_latest = Arc::clone(&latest);
        match std::thread::Builder::new()
            .name("system-sampler".to_string())
            .spawn(move || sample_loop(interval, thread_latest, thread_stop))
        {
            Ok(_handle) => {
                *worker = Some(Worker { stop, latest });
            }
            Err(err) => logger::error(
                "system-sampler",
                &format!("could not start sampling thread: {err}"),
            ),
        }
    }

    /// Requests shutdown without blocking the UI thread on in-flight queries.
    pub fn stop(&self) {
        let mut worker = self.worker.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(active) = worker.take() {
            active.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Returns the latest sampled system snapshot.
    pub fn latest(&self) -> Option<Arc<SystemSnapshot>> {
        let worker = self.worker.lock().unwrap_or_else(|err| err.into_inner());
        let active = worker.as_ref()?;
        let latest = active.latest.lock().unwrap_or_else(|err| err.into_inner());
        latest.clone()
    }
}

impl Drop for SystemSampler {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Copy)]
struct CpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

fn filetime_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

fn read_cpu_times() -> Option<CpuTimes> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: `idle`, `kernel`, and `user` are valid writable `FILETIME` out-pointers.
    if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
        return None;
    }
    Some(CpuTimes {
        idle: filetime_to_u64(idle),
        kernel: filetime_to_u64(kernel),
        user: filetime_to_u64(user),
    })
}

fn cpu_busy_ratio(prev: &mut Option<CpuTimes>) -> f32 {
    let Some(curr) = read_cpu_times() else {
        return 0.0;
    };
    let Some(last) = prev.replace(curr) else {
        return 0.0;
    };
    let idle_delta = curr.idle.saturating_sub(last.idle);
    let kernel_delta = curr.kernel.saturating_sub(last.kernel);
    let user_delta = curr.user.saturating_sub(last.user);
    let total_delta = kernel_delta.saturating_add(user_delta);
    let busy_delta = total_delta.saturating_sub(idle_delta);
    if total_delta > 0 {
        (busy_delta as f32 / total_delta as f32).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn sample_memory() -> (f32, u64, u64) {
    let mut mem = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `mem` is a valid writable `MEMORYSTATUSEX` with `dwLength` initialized to its size.
    if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() && mem.ullTotalPhys > 0 {
        let used = mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys);
        let ratio = (used as f32 / mem.ullTotalPhys as f32).clamp(0.0, 1.0);
        (ratio, used, mem.ullTotalPhys)
    } else {
        (0.0, 0, 0)
    }
}

fn sample_drives() -> (f32, Vec<DiskDriveInfo>) {
    // SAFETY: `GetLogicalDrives` takes no pointers and returns a bitmask of mounted drive letters.
    let drives_bitmask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    let mut total_all_bytes = 0u64;
    let mut total_used_bytes = 0u64;
    let mut primary_ratio: Option<f32> = None;

    for i in 0..26 {
        if (drives_bitmask & (1 << i)) != 0 {
            let drive_letter = (b'A' + i) as char;
            let wide = to_wide(&format!("{drive_letter}:\\"));
            // SAFETY: `wide` is a null-terminated UTF-16 root path that outlives the call.
            let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
            if drive_type == DRIVE_FIXED {
                let mut free_bytes_avail = 0u64;
                let mut total_bytes = 0u64;
                let mut total_free = 0u64;
                // SAFETY: `wide` is a null-terminated UTF-16 path and the byte counters are valid
                // writable `u64` out-pointers.
                let ok = unsafe {
                    GetDiskFreeSpaceExW(
                        PCWSTR(wide.as_ptr()),
                        Some(&mut free_bytes_avail),
                        Some(&mut total_bytes),
                        Some(&mut total_free),
                    )
                };
                if ok.is_ok() && total_bytes > 0 {
                    let used_bytes = total_bytes.saturating_sub(total_free);
                    let ratio = (used_bytes as f32 / total_bytes as f32).clamp(0.0, 1.0);
                    if drive_letter == 'C' {
                        primary_ratio = Some(ratio);
                    }
                    total_all_bytes = total_all_bytes.saturating_add(total_bytes);
                    total_used_bytes = total_used_bytes.saturating_add(used_bytes);
                    drives.push(DiskDriveInfo {
                        letter: drive_letter,
                        total_bytes,
                        used_bytes,
                        used_ratio: ratio,
                    });
                }
            }
        }
    }

    let overall_ratio = if let Some(c_ratio) = primary_ratio {
        c_ratio
    } else if total_all_bytes > 0 {
        (total_used_bytes as f32 / total_all_bytes as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    (overall_ratio, drives)
}

fn sample_loop(
    interval: Duration,
    latest: Arc<Mutex<Option<Arc<SystemSnapshot>>>>,
    stop: Arc<AtomicBool>,
) {
    let mut cpu_baseline = None;
    let _ = cpu_busy_ratio(&mut cpu_baseline);
    let mut cached_drives = sample_drives();
    let mut last_drive_sample = Instant::now();

    // Populate an initial snapshot immediately so callers have memory and storage data right away.
    {
        let (mem_ratio, mem_used, mem_total) = sample_memory();
        let snapshot = Arc::new(SystemSnapshot {
            cpu_usage: 0.0,
            memory_usage: mem_ratio,
            memory_used_bytes: mem_used,
            memory_total_bytes: mem_total,
            disk_activity: 0.0,
            disk_storage_ratio: cached_drives.0,
            drives: cached_drives.1.clone(),
        });
        let mut guard = latest.lock().unwrap_or_else(|err| err.into_inner());
        *guard = Some(snapshot);
    }

    if stop.load(Ordering::Relaxed) {
        return;
    }

    let mut pdh = PdhDiskCounter::new();

    while !stop.load(Ordering::Relaxed) {
        sleep_until(&stop, interval);

        if stop.load(Ordering::Relaxed) {
            break;
        }

        let disk_activity = pdh.as_mut().map(|p| p.sample()).unwrap_or(0.0);
        let (mem_ratio, mem_used, mem_total) = sample_memory();
        let cpu_ratio = cpu_busy_ratio(&mut cpu_baseline);

        if last_drive_sample.elapsed() >= DRIVE_REFRESH_INTERVAL {
            cached_drives = sample_drives();
            last_drive_sample = Instant::now();
        }

        let snapshot = Arc::new(SystemSnapshot {
            cpu_usage: cpu_ratio,
            memory_usage: mem_ratio,
            memory_used_bytes: mem_used,
            memory_total_bytes: mem_total,
            disk_activity,
            disk_storage_ratio: cached_drives.0,
            drives: cached_drives.1.clone(),
        });
        let mut guard = latest.lock().unwrap_or_else(|err| err.into_inner());
        *guard = Some(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_sampler_samples_and_stops() {
        let sampler = SystemSampler::new(Duration::from_millis(100));
        sampler.start();
        let mut got_snapshot = false;
        for _ in 0..50 {
            if let Some(snapshot) = sampler.latest() {
                assert!((0.0..=1.0).contains(&snapshot.cpu_usage));
                assert!((0.0..=1.0).contains(&snapshot.memory_usage));
                assert!((0.0..=1.0).contains(&snapshot.disk_activity));
                assert!((0.0..=1.0).contains(&snapshot.disk_storage_ratio));
                assert!(!snapshot.drives.is_empty(), "Expected at least C: drive");
                got_snapshot = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        sampler.stop();
        assert!(got_snapshot, "Did not get snapshot within 2.5s");
        assert!(sampler.latest().is_none());

        // Verify restarting after stop works cleanly.
        sampler.start();
        for _ in 0..50 {
            if sampler.latest().is_some() {
                sampler.stop();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        sampler.stop();
        panic!("Did not get snapshot after restart within 2.5s");
    }
}
