use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Replaces this process with ssh; signals and exit code then belong to ssh directly.
pub fn launch(mut command: Command, bin: &OsStr) -> anyhow::Result<i32> {
    let err = command.exec();
    Err(super::start_error(err, bin))
}
