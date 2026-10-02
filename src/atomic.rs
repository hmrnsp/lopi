//! Crash-safe file replacement: readers see either the old or the new file, never a
//! half-written one.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

/// Writes `contents` to a temporary file in the same directory, flushes it to disk, then
/// renames it over `path`. On Unix the new file is private to the user (mode 0600).
pub fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    persist(tmp, path)?;
    sync_dir(dir);
    Ok(())
}

/// On Windows a virus scanner, indexer or editor may hold the target open for a moment,
/// making the rename fail with "access denied"; retry for about a second before giving up.
fn persist(mut tmp: NamedTempFile, path: &Path) -> io::Result<()> {
    const ATTEMPTS: u32 = 10;
    let mut delay = std::time::Duration::from_millis(10);
    for attempt in 1..=ATTEMPTS {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(err)
                if cfg!(windows)
                    && err.error.kind() == io::ErrorKind::PermissionDenied
                    && attempt < ATTEMPTS =>
            {
                tmp = err.file;
                std::thread::sleep(delay);
                delay = (delay * 2).min(std::time::Duration::from_millis(200));
            }
            Err(err) => return Err(err.error),
        }
    }
    unreachable!("the last attempt always returns")
}

/// Makes the rename itself durable on Unix. Best effort: some file systems refuse it.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_file_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("a.toml");
        write(&path, b"one").unwrap();
        write(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let entries = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(entries, 1);
    }
}
