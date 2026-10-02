//! Snapshots of the profiles file taken before every save, so a bad change can be
//! reverted with `lopi restore` (no file: choose a snapshot).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

use crate::time::{compact_utc, parse_compact};

/// How many snapshots are kept; older ones are deleted.
pub const KEEP: usize = 10;
const PREFIX: &str = "profiles-";
const SUFFIX: &str = ".toml";

/// Copies `file` into `dir` as `profiles-<UTC time>.toml` and deletes the oldest snapshots
/// beyond [`KEEP`]. Does nothing when `file` does not exist yet.
pub fn snapshot(file: &Path, dir: &Path, now: SystemTime) -> Result<Option<PathBuf>> {
    if !file.exists() {
        return Ok(None);
    }
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let target = free_name(dir, &compact_utc(now));
    fs::copy(file, &target)
        .with_context(|| format!("cannot back up {} to {}", file.display(), target.display()))?;
    prune(dir, KEEP)?;
    Ok(Some(target))
}

/// Snapshot files in `dir`, oldest first. Only names this module creates are listed, so
/// other files the user puts there are never touched.
pub fn list(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", dir.display())),
    };
    let mut files: Vec<((SystemTime, u32), PathBuf)> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter_map(|path| Some((parse_name(path.file_name()?.to_str()?)?, path)))
        .collect();
    // By time, then by the `-N` counter (plain name order would put `…Z-1` before `…Z`).
    files.sort();
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// A snapshot as offered by `restore`.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub path: PathBuf,
    pub time: SystemTime,
    /// Number of profiles in it; `None` if it cannot be read as a profiles file.
    pub profiles: Option<usize>,
}

/// Snapshots in `dir`, newest first.
pub fn list_snapshots(dir: &Path) -> Result<Vec<Snapshot>> {
    let mut snapshots: Vec<Snapshot> = list(dir)?
        .into_iter()
        .filter_map(|path| {
            let (time, _) = parse_name(path.file_name()?.to_str()?)?;
            let profiles = fs::read_to_string(&path)
                .ok()
                .and_then(|text| super::store::parse(&text).ok())
                .map(|config| config.profiles.len());
            Some(Snapshot {
                path,
                time,
                profiles,
            })
        })
        .collect();
    snapshots.reverse();
    Ok(snapshots)
}

/// `profiles-<stamp>[-N].toml` → (time, N); `None` for any other name.
fn parse_name(name: &str) -> Option<(SystemTime, u32)> {
    let middle = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let (stamp, counter) = match middle.split_once('-') {
        Some((stamp, n)) => (stamp, n.parse().ok()?),
        None => (middle, 0),
    };
    Some((parse_compact(stamp)?, counter))
}

fn prune(dir: &Path, keep: usize) -> Result<()> {
    let files = list(dir)?;
    let excess = files.len().saturating_sub(keep);
    for old in &files[..excess] {
        fs::remove_file(old).with_context(|| format!("cannot delete {}", old.display()))?;
    }
    Ok(())
}

/// `profiles-<stamp>.toml`, or `profiles-<stamp>-N.toml` if two saves share a millisecond.
fn free_name(dir: &Path, stamp: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{PREFIX}{stamp}{SUFFIX}"));
    let mut n = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{PREFIX}{stamp}-{n}{SUFFIX}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn keeps_the_newest_ten_and_ignores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("profiles.toml");
        let backups = dir.path().join("backups");
        fs::create_dir_all(&backups).unwrap();
        fs::write(backups.join("notes.txt"), "mine").unwrap();

        for i in 0..13u64 {
            fs::write(&file, format!("version {i}")).unwrap();
            let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000 + i);
            snapshot(&file, &backups, now).unwrap();
        }
        let kept = list(&backups).unwrap();
        assert_eq!(kept.len(), KEEP);
        assert_eq!(fs::read_to_string(&kept[0]).unwrap(), "version 3");
        assert_eq!(fs::read_to_string(&kept[9]).unwrap(), "version 12");
        assert!(backups.join("notes.txt").exists());
    }

    #[test]
    fn same_millisecond_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("profiles.toml");
        fs::write(&file, "x").unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let a = snapshot(&file, dir.path(), now).unwrap().unwrap();
        let b = snapshot(&file, dir.path(), now).unwrap().unwrap();
        assert_ne!(a, b);
        // `…Z-1.toml` was made after `…Z.toml`, so it must list after it
        assert_eq!(list(dir.path()).unwrap(), [a, b]);
    }

    #[test]
    fn snapshots_newest_first_with_profile_counts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("profiles.toml");
        let backups = dir.path().join("backups");
        let base = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::write(&file, "[profiles.a]\nhost = \"a\"\n").unwrap();
        snapshot(&file, &backups, base).unwrap();
        fs::write(&file, "broken = = file").unwrap();
        snapshot(&file, &backups, base + Duration::from_secs(60)).unwrap();

        let snapshots = list_snapshots(&backups).unwrap();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].time, base + Duration::from_secs(60));
        assert_eq!(snapshots[0].profiles, None, "unreadable snapshot");
        assert_eq!(snapshots[1].profiles, Some(1));
    }

    #[test]
    fn missing_file_needs_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let result = snapshot(&dir.path().join("none.toml"), dir.path(), SystemTime::now());
        assert!(result.unwrap().is_none());
    }
}
