//! NetSpeed Widget: a live network speed, latency and traffic readout that
//! lives inside the Windows taskbar.
//!
//! The app owns one independent taskbar overlay that shows download and upload
//! rates, a latency probe and a rolling traffic graph. Hover/click details show
//! session totals and previous speedtests. A tray icon carries the same menu, so the
//! widget can be hidden without losing the way back to it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas;
mod clock;
mod config;
mod format;
mod graph;
mod hotkey;
mod latency;
mod logger;
mod paint;
mod paths;
mod sampler;
mod schedule;
mod speedtest;
mod theme;
mod win;

use std::os::windows::io::{FromRawHandle, OwnedHandle};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

pub use clock::now_epoch;

/// Name of the mutex that keeps a second copy from starting.
const INSTANCE_MUTEX: &str = "Local\\NetSpeedWidgetSingleInstance";

fn main() {
    let _mutex = match claim_single_instance() {
        Ok(mutex) => mutex,
        Err(message) => {
            logger::error("app", &message);
            startup_message(&message);
            std::process::exit(1);
        }
    };

    if let Err(err) = app::run() {
        logger::error("app", &format!("fatal startup error: {err}"));
        startup_message(&format!("NetSpeed Widget failed to start: {err}"));
        std::process::exit(1);
    }
}

fn startup_message(message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let text = win::gdi::to_wide(message);
    let title = win::gdi::to_wide("NetSpeed Widget");
    // SAFETY: Both strings are terminated UTF-16 buffers that outlive this
    // synchronous call, and the message box has no owner window.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Claims the single-instance mutex.
///
/// A second copy would fight the first one over the taskbar slot, so it
/// refuses to start instead.
fn claim_single_instance() -> Result<OwnedHandle, String> {
    // SAFETY: The mutex name remains terminated and live throughout creation.
    // A successful call returns a unique owned kernel handle, transferred once
    // to OwnedHandle so every returning path closes it through RAII.
    unsafe {
        let name: Vec<u16> = INSTANCE_MUTEX
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            Ok(handle) => {
                let already_exists = GetLastError() == ERROR_ALREADY_EXISTS;
                let handle = OwnedHandle::from_raw_handle(handle.0);
                if already_exists {
                    return Err("another copy of NetSpeed Widget is already running".to_string());
                }
                Ok(handle)
            }
            Err(err) => Err(format!("could not create the single-instance mutex: {err}")),
        }
    }
}
