//! Per-user sign-in startup. Registry state, not app settings, is authoritative.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_OPTION_NON_VOLATILE,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RRF_NOEXPAND, RRF_RT_ANY,
};

use super::gdi::to_wide;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVAL_KEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const VALUE_NAME: &str = "NetSpeedWidget";

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns a successfully opened registry handle.
        // Closing it cannot usefully be retried from Drop.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

impl Key {
    fn open(path: &str, access: REG_SAM_FLAGS) -> Result<Option<Self>, String> {
        let path = to_wide(path);
        let mut key = HKEY::default();
        // SAFETY: The path is terminated and key is a writable output handle.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                0,
                access,
                &mut key,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status, "open startup registry key")?;
        Ok(Some(Self(key)))
    }

    fn create(path: &str) -> Result<Self, String> {
        let path = to_wide(path);
        let mut key = HKEY::default();
        // SAFETY: The terminated path and writable output remain live for the call.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                0,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        check(status, "create startup registry key")?;
        Ok(Self(key))
    }

    fn read(&self, name: &str) -> Result<Option<Value>, String> {
        let name = to_wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0;
        // SAFETY: All outputs are writable; no data buffer is supplied for the size query.
        let status = unsafe {
            RegGetValueW(
                self.0,
                None,
                PCWSTR(name.as_ptr()),
                RRF_RT_ANY | RRF_NOEXPAND,
                Some(&mut kind),
                None,
                Some(&mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status, "read startup registry value size")?;
        // Retry a bounded number of times if another process changes the value.
        for _ in 0..3 {
            let mut data = vec![0; size as usize];
            // SAFETY: data has the queried byte capacity; size reports that capacity on entry.
            let status = unsafe {
                RegGetValueW(
                    self.0,
                    None,
                    PCWSTR(name.as_ptr()),
                    RRF_RT_ANY | RRF_NOEXPAND,
                    Some(&mut kind),
                    Some(data.as_mut_ptr().cast()),
                    Some(&mut size),
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if status == ERROR_MORE_DATA {
                continue;
            }
            check(status, "read startup registry value")?;
            data.truncate(size as usize);
            return Ok(Some(Value { kind, data }));
        }
        Err("startup registry value kept changing; please try again".to_string())
    }

    fn write(&self, name: &str, value: Option<&Value>) -> Result<(), String> {
        let name = to_wide(name);
        // SAFETY: The handle is live and the terminated name and optional byte slice
        // outlive the call. RegSetValueExW copies the supplied bytes.
        let status = unsafe {
            match value {
                Some(value) => RegSetValueExW(
                    self.0,
                    PCWSTR(name.as_ptr()),
                    0,
                    value.kind,
                    Some(&value.data),
                ),
                None => RegDeleteValueW(self.0, PCWSTR(name.as_ptr())),
            }
        };
        if value.is_none() && status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status, "update startup registry value")
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Value {
    kind: REG_VALUE_TYPE,
    data: Vec<u8>,
}

fn check(status: WIN32_ERROR, operation: &str) -> Result<(), String> {
    status
        .ok()
        .map_err(|err| format!("could not {operation}: {err}"))
}

fn command(executable: &Path) -> Value {
    let wide: Vec<u16> = std::iter::once(b'"' as u16)
        .chain(executable.as_os_str().encode_wide())
        .chain([b'"' as u16, 0])
        .collect();
    Value {
        kind: REG_SZ,
        data: wide.into_iter().flat_map(u16::to_le_bytes).collect(),
    }
}

fn approved(value: Option<&Value>) -> bool {
    match value {
        None => true,
        // StartupApproved is a DWORD state followed by an eight-byte timestamp.
        // Both enabled states are seen on Windows; unknown/malformed data is not enabled.
        Some(value) => {
            value.kind == REG_BINARY
                && value.data.len() == 12
                && matches!(
                    u32::from_le_bytes(value.data[..4].try_into().expect("length checked")),
                    2 | 6
                )
        }
    }
}

/// Whether this exact executable is registered and not disabled in Startup Apps.
///
/// # Errors
/// Returns an error if the executable path or registry state cannot be read.
pub fn enabled() -> Result<bool, String> {
    let executable =
        std::env::current_exe().map_err(|err| format!("could not locate executable: {err}"))?;
    enabled_at(RUN_KEY, APPROVAL_KEY, VALUE_NAME, &executable)
}

fn enabled_at(
    run_path: &str,
    approval_path: &str,
    name: &str,
    executable: &Path,
) -> Result<bool, String> {
    let Some(run) = Key::open(run_path, KEY_QUERY_VALUE)? else {
        return Ok(false);
    };
    if run.read(name)?.as_ref() != Some(&command(executable)) {
        return Ok(false);
    }
    let approval = Key::open(approval_path, KEY_QUERY_VALUE)?
        .map(|key| key.read(name))
        .transpose()?
        .flatten();
    Ok(approved(approval.as_ref()))
}

/// Registers the current executable or removes only this app's Run entry.
///
/// # Errors
/// Returns an error if the executable path or registry update fails. If resetting
/// approval fails, attempts to restore the old Run value and includes any rollback
/// failure in the error.
pub fn set_enabled(enable: bool) -> Result<(), String> {
    if !enable {
        if let Some(run) = Key::open(RUN_KEY, KEY_SET_VALUE)? {
            run.write(VALUE_NAME, None)?;
        }
        return Ok(());
    }
    let executable =
        std::env::current_exe().map_err(|err| format!("could not locate executable: {err}"))?;
    let run = Key::create(RUN_KEY)?;
    // Open both keys before changing anything, so denied access preserves prior state.
    let approval = Key::open(APPROVAL_KEY, KEY_QUERY_VALUE | KEY_SET_VALUE)?;
    enable_entry(&run, approval.as_ref(), VALUE_NAME, &command(&executable))
}

fn enable_entry(
    run: &Key,
    approval: Option<&Key>,
    name: &str,
    value: &Value,
) -> Result<(), String> {
    let previous = run.read(name)?;
    run.write(name, Some(value))?;
    // Absence of StartupApproved means enabled. Delete only our value; do not
    // manufacture undocumented binary approval data or touch other applications.
    if let Some(approval) = approval {
        if let Err(err) = approval.write(name, None) {
            return match run.write(name, previous.as_ref()) {
                Ok(()) => Err(err),
                Err(rollback) => Err(format!(
                    "{err}; restoring previous startup entry also failed: {rollback}"
                )),
            };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real Win32 calls, but never the actual Windows startup keys.
    struct TestRegistry(String);

    impl TestRegistry {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(format!(
                r"Software\NetSpeedWidget-StartupTests-{}-{nonce}",
                std::process::id()
            ))
        }

        fn paths(&self) -> (String, String) {
            (format!(r"{}\Run", self.0), format!(r"{}\Approval", self.0))
        }
    }

    impl Drop for TestRegistry {
        fn drop(&mut self) {
            let path = to_wide(&self.0);
            // SAFETY: Only this test's unique, terminated HKCU sandbox path is removed.
            let status = unsafe {
                windows::Win32::System::Registry::RegDeleteTreeW(
                    HKEY_CURRENT_USER,
                    PCWSTR(path.as_ptr()),
                )
            };
            assert!(
                status.is_ok() || status == ERROR_FILE_NOT_FOUND,
                "test registry cleanup: {status:?}"
            );
        }
    }

    #[test]
    fn registry_toggle_repairs_stale_and_disabled_entries_and_preserves_other_values() {
        let sandbox = TestRegistry::new();
        let (run_path, approval_path) = sandbox.paths();
        let old_path = Path::new(r"C:\Old\widget.exe");
        let current_path = Path::new(r"C:\New Location\widget.exe");
        let enabled = || enabled_at(&run_path, &approval_path, VALUE_NAME, current_path).unwrap();
        assert!(!enabled());
        let run = Key::create(&run_path).unwrap();
        enable_entry(&run, None, VALUE_NAME, &command(current_path)).unwrap();
        assert!(enabled()); // A missing approval key is normal on a fresh profile.
        let approval = Key::create(&approval_path).unwrap();
        let old = command(old_path);
        let current = command(current_path);
        run.write("OtherApp", Some(&old)).unwrap();
        approval.write("OtherApp", Some(&old)).unwrap();
        run.write(VALUE_NAME, Some(&old)).unwrap();
        assert!(!enabled());

        enable_entry(&run, Some(&approval), VALUE_NAME, &current).unwrap();
        assert!(enabled()); // Reopens keys, as after restarting the app.
        assert_eq!(run.read(VALUE_NAME).unwrap(), Some(current));
        let disabled = Value {
            kind: REG_BINARY,
            data: vec![3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        approval.write(VALUE_NAME, Some(&disabled)).unwrap();
        assert!(!enabled());
        enable_entry(&run, Some(&approval), VALUE_NAME, &command(current_path)).unwrap();
        assert!(enabled());
        assert_eq!(approval.read(VALUE_NAME).unwrap(), None);

        run.write(VALUE_NAME, None).unwrap();
        run.write(VALUE_NAME, None).unwrap(); // Disabling is idempotent.
        assert!(!enabled());
        assert_eq!(run.read("OtherApp").unwrap(), Some(command(old_path)));
        assert_eq!(approval.read("OtherApp").unwrap(), Some(command(old_path)));
    }

    #[test]
    fn registry_write_failures_preserve_previous_values() {
        let sandbox = TestRegistry::new();
        let (run_path, approval_path) = sandbox.paths();
        let run = Key::create(&run_path).unwrap();
        let approval = Key::create(&approval_path).unwrap();
        let old = command(Path::new(r"C:\Old\widget.exe"));
        let new = command(Path::new(r"C:\New\widget.exe"));
        let disabled = Value {
            kind: REG_BINARY,
            data: vec![3; 12],
        };
        run.write(VALUE_NAME, Some(&old)).unwrap();
        approval.write(VALUE_NAME, Some(&disabled)).unwrap();
        let readonly_run = Key::open(&run_path, KEY_QUERY_VALUE).unwrap().unwrap();
        assert!(enable_entry(&readonly_run, Some(&approval), VALUE_NAME, &new).is_err());
        assert_eq!(
            run.read(VALUE_NAME).unwrap(),
            Some(command(Path::new(r"C:\Old\widget.exe")))
        );
        assert_eq!(approval.read(VALUE_NAME).unwrap(), Some(disabled));
        assert!(readonly_run.write(VALUE_NAME, None).is_err());

        let readonly_approval = Key::open(&approval_path, KEY_QUERY_VALUE).unwrap().unwrap();
        assert!(enable_entry(&run, Some(&readonly_approval), VALUE_NAME, &new).is_err());
        assert_eq!(run.read(VALUE_NAME).unwrap(), Some(old));
        run.write(VALUE_NAME, None).unwrap();
        assert!(enable_entry(&run, Some(&readonly_approval), VALUE_NAME, &new).is_err());
        assert_eq!(run.read(VALUE_NAME).unwrap(), None);
    }

    #[test]
    fn quotes_unicode_executable_paths_without_arguments() {
        let value = command(Path::new(r"C:\My Apps\网络\netspeed-widget.exe"));
        let wide: Vec<_> = value
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            String::from_utf16(&wide).unwrap(),
            "\"C:\\My Apps\\网络\\netspeed-widget.exe\"\0"
        );
        assert_eq!(value.kind, REG_SZ);
        assert_ne!(value, command(Path::new(r"D:\Moved\netspeed-widget.exe")));
    }

    #[test]
    fn approval_handles_enabled_disabled_and_malformed_states() {
        assert!(approved(None));
        for state in [2_u32, 3, 6, 7, 0, 99] {
            let mut data = vec![0; 12];
            data[..4].copy_from_slice(&state.to_le_bytes());
            let value = Value {
                kind: REG_BINARY,
                data,
            };
            assert_eq!(approved(Some(&value)), matches!(state, 2 | 6));
        }
        assert!(!approved(Some(&Value {
            kind: REG_BINARY,
            data: vec![2]
        })));
        assert!(!approved(Some(&Value {
            kind: REG_SZ,
            data: vec![0; 12]
        })));
    }
}
