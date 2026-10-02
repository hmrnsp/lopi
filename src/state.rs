//! Connection history in `state.toml` (machine-local data dir, separate from the profiles
//! file). History is a convenience: reading it never fails and writing it never blocks or
//! stops a connection.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::atomic;
use crate::config::{Config, Profile, paths};
use crate::lock::FileLock;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// History key (see [`history_key`]) → last connection time in Unix seconds.
    #[serde(default)]
    pub last_used: BTreeMap<String, u64>,
}

impl State {
    pub fn last_used(&self, name: &str, profile: &Profile) -> Option<u64> {
        self.last_used.get(&history_key(name, profile)).copied()
    }

    /// Profile names, most recently used first; never-used ones follow in name order.
    pub fn recent_first<'a>(&self, config: &'a Config) -> Vec<&'a str> {
        let mut names: Vec<(&str, Option<u64>)> = config
            .profiles
            .iter()
            .map(|(name, profile)| (name.as_str(), self.last_used(name, profile)))
            .collect();
        // `None` sorts before `Some`, so compare reversed for "newest first, unused last".
        names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        names.into_iter().map(|(name, _)| name).collect()
    }
}

/// Keyed by id so a rename keeps the history. Hand-written profiles get an id on the next
/// save; until then the name is used.
pub fn history_key(name: &str, profile: &Profile) -> String {
    match &profile.id {
        Some(id) => id.clone(),
        None => format!("name:{name}"),
    }
}

pub fn state_file() -> Result<PathBuf> {
    Ok(paths::state_dir()?.join("state.toml"))
}

/// The saved history, or empty if there is none or it cannot be read.
pub fn load() -> State {
    state_file()
        .map(|path| load_from(&path))
        .unwrap_or_default()
}

pub fn load_from(path: &Path) -> State {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Records a connection. Skips silently when another lopi holds the lock: waiting
/// would delay the connection for nothing but a history entry.
pub fn record_use(config: &Config, name: &str, now: u64) -> Result<()> {
    record_use_at(&state_file()?, config, name, now)
}

pub fn record_use_at(path: &Path, config: &Config, name: &str, now: u64) -> Result<()> {
    let Some(_lock) = FileLock::try_acquire(path)? else {
        return Ok(());
    };
    let mut state = load_from(path);
    let profile = config
        .profiles
        .get(name)
        .context("internal error: recording history for an unknown profile")?;
    state.last_used.insert(history_key(name, profile), now);
    // Forget deleted profiles so the file does not grow forever.
    let known: HashSet<String> = config
        .profiles
        .iter()
        .map(|(name, profile)| history_key(name, profile))
        .collect();
    state.last_used.retain(|key, _| known.contains(key));

    let text = toml::to_string(&state).context("cannot serialize history")?;
    atomic::write(path, text.as_bytes()).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(entries: &[(&str, Option<&str>)]) -> Config {
        let mut config = Config::default();
        for (name, id) in entries {
            let profile = Profile {
                id: id.map(str::to_string),
                ..Profile::new("h")
            };
            config.profiles.insert(name.to_string(), profile);
        }
        config
    }

    #[test]
    fn records_prunes_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        let cfg = config(&[
            ("a", Some("ida")),
            ("b", Some("idb")),
            ("c", None),
            ("d", None),
        ]);

        record_use_at(&path, &cfg, "a", 100).unwrap();
        record_use_at(&path, &cfg, "c", 300).unwrap();
        record_use_at(&path, &cfg, "b", 200).unwrap();
        let state = load_from(&path);
        assert_eq!(state.recent_first(&cfg), ["c", "b", "a", "d"]);
        assert_eq!(state.last_used("c", &cfg.profiles["c"]), Some(300));

        // renaming keeps history (same id); deleting forgets it on the next write
        let mut renamed = cfg.clone();
        let a = renamed.profiles.remove("a").unwrap();
        renamed.profiles.insert("z".into(), a);
        renamed.profiles.remove("b");
        record_use_at(&path, &renamed, "d", 400).unwrap();
        let state = load_from(&path);
        assert_eq!(state.recent_first(&renamed), ["d", "c", "z"]);
        assert!(!state.last_used.contains_key("idb"));
    }

    #[test]
    fn broken_or_missing_history_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        assert_eq!(load_from(&path), State::default());
        fs::write(&path, "not = = toml").unwrap();
        assert_eq!(load_from(&path), State::default());
        // and is replaced by the next write
        let cfg = config(&[("a", Some("ida"))]);
        record_use_at(&path, &cfg, "a", 1).unwrap();
        assert_eq!(load_from(&path).last_used["ida"], 1);
    }

    #[test]
    fn busy_lock_skips_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        let _held = FileLock::try_acquire(&path).unwrap().unwrap();
        let cfg = config(&[("a", Some("ida"))]);
        record_use_at(&path, &cfg, "a", 1).unwrap();
        assert!(!path.exists());
    }
}
