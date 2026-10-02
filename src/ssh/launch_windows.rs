use std::ffi::OsStr;
use std::process::Command;

use anyhow::Context;
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

/// Windows has no exec: run ssh as a child sharing this console and pass its exit code on
/// (255 = ssh connection error).
pub fn launch(mut command: Command, bin: &OsStr) -> anyhow::Result<i32> {
    let mut child = command
        .spawn()
        .map_err(|err| super::start_error(err, bin))?;

    // Ctrl+C goes to every process on the console. Ignore it here so only ssh reacts and
    // we stay alive to report its exit code. Called after spawn on purpose: the ignore
    // flag is inherited by children created later, and ssh must still get Ctrl+C.
    // SAFETY: a null handler with TRUE only toggles a per-process flag.
    unsafe {
        SetConsoleCtrlHandler(None, 1);
    }

    let status = child.wait().context("failed waiting for ssh")?;
    Ok(status.code().unwrap_or(1))
}
