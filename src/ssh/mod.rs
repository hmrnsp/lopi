pub mod args;
pub mod jump;
#[cfg(unix)]
mod launch_unix;
#[cfg(windows)]
mod launch_windows;

use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::process::Command;

use anyhow::{Error, anyhow};

/// Overrides the ssh executable (used by tests, or to pick a specific ssh).
pub const SSH_BIN_ENV: &str = "LOPI_SSH_BIN";

/// The ssh executable: `LOPI_SSH_BIN`, else `ssh_bin` from the profiles file
/// (`~` expanded), else `ssh` from PATH.
pub fn ssh_bin(configured: Option<&str>) -> OsString {
    if let Some(bin) = env::var_os(SSH_BIN_ENV).filter(|bin| !bin.is_empty()) {
        return bin;
    }
    match configured {
        Some(bin) => args::expand_tilde(bin, dirs::home_dir().as_deref()).into(),
        None => "ssh".into(),
    }
}

/// Runs ssh with the terminal handed over as-is (stdio inherited, never piped), so ssh can
/// ask for passphrases and host key confirmation itself. Returns ssh's exit code.
/// On Unix the process is replaced by ssh and this only returns on failure.
/// `env` is added to the inherited environment (used for askpass).
pub fn launch(bin: &OsStr, args: &[OsString], env: &[(OsString, OsString)]) -> anyhow::Result<i32> {
    let mut command = Command::new(bin);
    command.args(args);
    command.envs(env.iter().map(|(key, value)| (key, value)));
    #[cfg(unix)]
    return launch_unix::launch(command, bin);
    #[cfg(windows)]
    return launch_windows::launch(command, bin);
}

fn start_error(err: io::Error, bin: &OsStr) -> Error {
    if err.kind() != io::ErrorKind::NotFound {
        return Error::new(err).context(format!("cannot start '{}'", bin.display()));
    }
    let install = if cfg!(windows) {
        "install the OpenSSH client: Settings > System > Optional features > OpenSSH Client, \
         or in an admin PowerShell: Add-WindowsCapability -Online -Name OpenSSH.Client~~~~0.0.1.0"
    } else {
        "install the OpenSSH client, e.g. `sudo apt install openssh-client` \
         or `sudo dnf install openssh-clients`"
    };
    anyhow!(
        "cannot find '{}'; {install}. To use a specific ssh, set ssh_bin in the profiles file \
         (see `lopi path`) or {SSH_BIN_ENV}",
        bin.display()
    )
}
