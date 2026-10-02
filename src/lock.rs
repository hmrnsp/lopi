//! Cross-process lock around read-modify-write of a file, using a separate `<file>.lock`
//! (locking the data file itself would block the atomic rename on Windows). The lock file
//! is never deleted: deleting it would let two processes lock two different files.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// How long `mutate` waits for another lopi to finish saving.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_EVERY: Duration = Duration::from_millis(50);

/// Held while the guarded file is being changed; the OS releases the lock when this is
/// dropped (or when the process dies), so a crash never leaves a stale lock.
#[derive(Debug)]
pub struct FileLock {
    _file: File,
}

impl FileLock {
    /// Waits up to `timeout` for the lock that guards `path`.
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        let started = Instant::now();
        loop {
            if let Some(lock) = Self::try_acquire(path)? {
                return Ok(lock);
            }
            if started.elapsed() >= timeout {
                bail!(
                    "another lopi is changing {} right now; try again in a moment",
                    path.display()
                );
            }
            thread::sleep(RETRY_EVERY);
        }
    }

    /// Takes the lock if it is free, without waiting.
    pub fn try_acquire(path: &Path) -> Result<Option<Self>> {
        let lock_path = lock_path(path);
        if let Some(dir) = lock_path.parent()
            && !dir.as_os_str().is_empty()
        {
            fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("cannot open lock file {}", lock_path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => {
                Err(err).with_context(|| format!("cannot lock {}", lock_path.display()))
            }
        }
    }
}

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_waits_then_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("profiles.toml");
        let held = FileLock::acquire(&path, DEFAULT_TIMEOUT).unwrap();

        assert!(FileLock::try_acquire(&path).unwrap().is_none());
        let started = Instant::now();
        let err = FileLock::acquire(&path, Duration::from_millis(120)).unwrap_err();
        assert!(started.elapsed() >= Duration::from_millis(120));
        assert!(err.to_string().contains("another lopi"), "{err}");

        drop(held);
        assert!(FileLock::try_acquire(&path).unwrap().is_some());
    }
}
