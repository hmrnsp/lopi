//! The user `PATH` lives in `HKCU\Environment\Path`. It is read and written raw: the
//! process `PATH` (`env::var("PATH")`) is the system and user values merged and expanded,
//! and writing that back would corrupt the user's setting.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::process::Command;
use std::ptr;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, RRF_NOEXPAND,
    RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};

use super::{PathValue, UserPath};

/// `HKCU\Environment\Path`.
pub struct RegistryPath;

impl UserPath for RegistryPath {
    fn read(&self) -> Result<Option<PathValue>> {
        let key = Key::open(KEY_QUERY_VALUE)?;
        let name = wide("Path");
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: valid key and NUL-terminated name; a null buffer asks for the size.
        let status = unsafe {
            RegGetValueW(
                key.0,
                ptr::null(),
                name.as_ptr(),
                flags,
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status, "read the user PATH")?;
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        // SAFETY: `buffer` holds `size` bytes, as the registry just reported.
        let status = unsafe {
            RegGetValueW(
                key.0,
                ptr::null(),
                name.as_ptr(),
                flags,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        check(status, "read the user PATH")?;
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Ok(Some(PathValue {
            text: String::from_utf16_lossy(&buffer[..len]),
            expandable: kind == REG_EXPAND_SZ,
        }))
    }

    fn write(&mut self, value: &PathValue) -> Result<()> {
        let key = Key::open(KEY_SET_VALUE)?;
        let name = wide("Path");
        let data = wide(&value.text);
        let kind = if value.expandable {
            REG_EXPAND_SZ
        } else {
            REG_SZ
        };
        // SAFETY: valid key; `data` is NUL-terminated UTF-16 and its byte length is given.
        let status = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                kind,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        check(status, "change the user PATH")?;
        broadcast_environment_change();
        Ok(())
    }
}

/// Tells Explorer (and so terminals opened from now on) that the environment changed.
/// Best effort: a window that does not answer within 5 seconds is skipped.
fn broadcast_environment_change() {
    let area = wide("Environment");
    let mut result = 0usize;
    // SAFETY: `area` outlives the call; SMTO_ABORTIFHUNG bounds the wait.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            area.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
}

/// The effective PowerShell execution policy (e.g. `RemoteSigned`, `Restricted`), or
/// `None` if Windows PowerShell could not be asked.
pub fn execution_policy() -> Option<String> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-ExecutionPolicy",
        ])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Closes the key when dropped.
struct Key(HKEY);

impl Key {
    fn open(access: u32) -> Result<Self> {
        let subkey = wide("Environment");
        let mut key: HKEY = ptr::null_mut();
        // SAFETY: valid root key and NUL-terminated subkey; `key` receives the handle.
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut key) };
        check(status, "open HKEY_CURRENT_USER\\Environment")?;
        Ok(Self(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegOpenKeyExW and is closed once.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn check(status: u32, action: &str) -> Result<()> {
    if status != ERROR_SUCCESS {
        let err = std::io::Error::from_raw_os_error(status as i32);
        bail!("cannot {action}: {err}");
    }
    Ok(())
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}
