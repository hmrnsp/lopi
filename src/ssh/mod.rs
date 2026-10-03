pub mod args;
pub mod jump;
#[cfg(unix)]
mod launch_unix;
#[cfg(windows)]
mod launch_windows;

use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

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

/// What a non-interactive ssh run produced (see [`run_captured`]).
#[derive(Debug)]
pub struct Captured {
    /// ssh exited with 0.
    pub success: bool,
    pub stderr: String,
    /// From start until ssh exited (or was stopped).
    pub elapsed: Duration,
    /// ssh was stopped after the time limit.
    pub timed_out: bool,
}

/// Runs ssh without the terminal: stdin and stdout are discarded and stderr is captured.
/// Only for checks that must never ask anything (`ping`); connections use [`launch`],
/// which hands the terminal to ssh. ssh is stopped after `limit`.
pub fn run_captured(
    bin: &OsStr,
    args: &[OsString],
    env: &[(OsString, OsString)],
    limit: Duration,
) -> anyhow::Result<Captured> {
    let started = Instant::now();
    let mut child = Command::new(bin)
        .args(args)
        .envs(env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| start_error(err, bin))?;
    // Read on a thread so a chatty ssh cannot fill the pipe and stall. A jump host's ssh
    // (a child of ssh) may keep the pipe open after ssh is stopped, so the reader is
    // waited for only briefly.
    let mut pipe = child.stderr.take().expect("stderr is piped");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    let (success, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status.success(), false);
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            break (false, true);
        }
        thread::sleep(Duration::from_millis(20));
    };
    let elapsed = started.elapsed();
    let stderr = receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap_or_default();
    Ok(Captured {
        success,
        stderr,
        elapsed,
        timed_out,
    })
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
