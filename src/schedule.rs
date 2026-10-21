//! Scheduling for the periodic speedtest, kept free of the UI and the clock.

use crate::config::SpeedtestRecord;

/// Returns the epoch second the next automatic run is due.
///
/// A saved result schedules one full interval after it. If that moment has
/// already passed, or nothing was ever measured, the startup grace applies
/// so a fresh launch does not go straight onto the network.
pub fn next_speedtest_due(
    saved: Option<&SpeedtestRecord>,
    now: f64,
    interval_sec: f64,
    startup_grace_sec: f64,
) -> f64 {
    if let Some(record) = saved {
        let due = record.ts + interval_sec;
        if due > now {
            return due;
        }
    }
    now + startup_grace_sec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_result_schedules_a_full_interval() {
        let record = SpeedtestRecord {
            down_mbps: 100.0,
            up_mbps: 50.0,
            ts: 1_000.0,
        };
        // The saved result is still young enough for its interval to hold.
        let due = next_speedtest_due(Some(&record), 1_050.0, 100.0, 20.0);
        assert_eq!(due, 1_100.0);
    }

    #[test]
    fn an_overdue_result_falls_back_to_the_startup_grace() {
        let record = SpeedtestRecord {
            down_mbps: 100.0,
            up_mbps: 50.0,
            ts: 1_000.0,
        };
        let due = next_speedtest_due(Some(&record), 2_000.0, 100.0, 20.0);
        assert_eq!(due, 2_020.0);
    }

    #[test]
    fn without_a_result_the_startup_grace_applies() {
        let due = next_speedtest_due(None, 2_000.0, 100.0, 20.0);
        assert_eq!(due, 2_020.0);
    }
}
