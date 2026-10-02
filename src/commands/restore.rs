//! `lopi restore [FILE]`: brings back a `lopi backup` file (profiles, saved
//! passwords, key files), a plain profiles file, or (without FILE) one of the automatic
//! snapshots taken before every change. The current profiles are snapshotted first, so a
//! restore can itself be undone with `lopi restore`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use zeroize::Zeroizing;

use super::require_terminal;
use crate::atomic;
use crate::backup_bundle::{self, KeyFile};
use crate::commands::backup::summary;
use crate::config::Config;
use crate::config::backup::{self as snapshots, Snapshot};
use crate::config::store::{self, StorePaths};
use crate::error::Abort;
use crate::secrets::{KeyringStore, SecretStore};
use crate::ssh::args::expand_tilde;
use crate::time;
use crate::ui::prompt::{Prompter, TerminalPrompter};
use crate::ui::tty;

/// What is being restored.
struct Incoming {
    /// Where it came from, for messages.
    label: String,
    profiles: String,
    passwords: Vec<(String, Zeroizing<String>)>,
    keys: Vec<KeyFile>,
    /// A `lopi backup` file; snapshots and plain profiles files hold profiles only.
    from_backup: bool,
}

/// Shown wherever a restore can bring back profiles only.
const PROFILES_ONLY: &str =
    "profiles only: passwords and key files come back with `lopi restore <backup file>`";

pub fn run(file: Option<PathBuf>, yes: bool, overwrite_keys: bool) -> Result<i32> {
    let store_paths = StorePaths::from_env()?;
    let incoming = match file {
        Some(file) => read_file(&file)?,
        None => pick_snapshot(&store_paths)?,
    };
    let config = store::parse(&incoming.profiles)
        .with_context(|| format!("{} does not hold a valid profiles file", incoming.label))?;
    config
        .validate()
        .with_context(|| format!("{} holds invalid profiles", incoming.label))?;

    let current = store::load_from(&store_paths.file).ok();
    let home = dirs::home_dir();
    let mut keys = plan_keys(incoming.keys, home.as_deref());

    // Preview.
    let current_count = current.as_ref().map_or(0, |c| c.profiles.len());
    println!("from {}:", incoming.label);
    println!(
        "  {}",
        summary(config.profiles.len(), incoming.passwords.len(), keys.len())
    );
    for key in &keys {
        println!("  key {}: {}", key.target_text(), key.status.describe());
    }
    if !incoming.from_backup {
        println!("  ({PROFILES_ONLY})");
    }

    decide_keys(&mut keys, overwrite_keys)?;
    if !yes {
        require_terminal("-y to restore without confirmation")?;
        let question = format!(
            "Replace your {current_count} profile(s) with the {} from {}?",
            config.profiles.len(),
            incoming.label
        );
        if !TerminalPrompter.confirm(&question, false)? {
            return Err(Abort::Cancelled.into());
        }
    }

    // 1. Profiles (the current file is snapshotted first).
    store::replace_in(&store_paths, &incoming.profiles)?;
    println!("restored {} profile(s)", config.profiles.len());

    // 2. Passwords, for profiles that exist in what was restored.
    if !incoming.passwords.is_empty() {
        restore_passwords(&KeyringStore::new(), &config, &incoming.passwords);
    }

    // 3. Key files.
    for key in &keys {
        match key.write() {
            Ok(Some(message)) => println!("{message}"),
            Ok(None) => {}
            Err(err) => eprintln!("lopi: warning: {err:#}"),
        }
    }

    report_missing_passwords(&config, &KeyringStore::new(), incoming.from_backup);

    // Saved passwords of profiles that are gone stay in the credential store: restoring
    // an earlier snapshot brings those profiles, and so their passwords, back.
    if let Some(current) = &current {
        let kept = kept_passwords(current, &config);
        if kept > 0 {
            println!(
                "{kept} saved password(s) of profiles not in this restore were kept, \
                 in case you restore them later"
            );
        }
    }
    Ok(0)
}

fn read_file(path: &Path) -> Result<Incoming> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let label = path.display().to_string();
    if !backup_bundle::is_encrypted(&bytes) {
        let text = String::from_utf8(bytes)
            .with_context(|| format!("{label} is neither a lopi backup nor a profiles file"))?;
        let text = text
            .strip_prefix('\u{feff}')
            .map(str::to_string)
            .unwrap_or(text);
        return Ok(Incoming {
            label,
            profiles: text,
            passwords: Vec::new(),
            keys: Vec::new(),
            from_backup: false,
        });
    }
    require_terminal(
        "a terminal (the backup passphrase is typed in, never passed as an argument)",
    )?;
    let passphrase = TerminalPrompter.password("Passphrase for this backup:", false)?;
    let plain = backup_bundle::decrypt(&bytes, &passphrase)?;
    let bundle = backup_bundle::unpack(&plain)?;
    Ok(Incoming {
        label: format!("{label} (made {})", bundle.created_at),
        profiles: bundle.profiles,
        passwords: bundle.passwords.into_iter().collect(),
        keys: bundle.keys,
        from_backup: true,
    })
}

fn pick_snapshot(store_paths: &StorePaths) -> Result<Incoming> {
    require_terminal("lopi restore <file>")?;
    let list = snapshots::list_snapshots(&store_paths.backups)?;
    if list.is_empty() {
        bail!("there are no snapshots yet; they are taken automatically before each change");
    }
    let now = time::unix_now();
    let options: Vec<String> = list.iter().map(|s| snapshot_label(s, now)).collect();
    let question = format!("Restore which snapshot? ({PROFILES_ONLY})");
    let index = TerminalPrompter.select(&question, &options, 0)?;
    let chosen = &list[index];
    let profiles = store::read_text(&chosen.path)?
        .with_context(|| format!("{} disappeared", chosen.path.display()))?;
    Ok(Incoming {
        label: format!("the snapshot from {}", options[index]),
        profiles,
        passwords: Vec::new(),
        keys: Vec::new(),
        from_backup: false,
    })
}

fn snapshot_label(snapshot: &Snapshot, now: u64) -> String {
    let secs = snapshot
        .time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let profiles = match snapshot.profiles {
        Some(1) => "1 profile".to_string(),
        Some(n) => format!("{n} profiles"),
        None => "unreadable".to_string(),
    };
    format!(
        "{} — {profiles} ({})",
        time::ago(now, secs),
        time::rfc3339_utc(snapshot.time)
    )
}

fn restore_passwords(
    secrets: &dyn SecretStore,
    config: &Config,
    passwords: &[(String, Zeroizing<String>)],
) {
    if let Err(err) = secrets.status() {
        eprintln!("lopi: warning: passwords not restored: {err:#}");
        return;
    }
    let ids: BTreeSet<&str> = config
        .profiles
        .values()
        .filter_map(|p| p.id.as_deref())
        .collect();
    let mut restored = 0;
    for (id, password) in passwords {
        if !ids.contains(id.as_str()) {
            continue;
        }
        match secrets.set(id, password) {
            Ok(()) => restored += 1,
            Err(err) => eprintln!("lopi: warning: {err:#}"),
        }
    }
    println!("restored {restored} saved password(s)");
}

/// Profiles that log in with a password but have none saved (for example after `rm`
/// and restoring a snapshot, which holds no passwords). Without a usable credential store
/// nothing can be said about them.
fn missing_passwords(config: &Config, secrets: &dyn SecretStore) -> Result<Vec<String>> {
    let users: Vec<(&String, Option<&str>)> = config
        .profiles
        .iter()
        .filter(|(_, profile)| profile.uses_password())
        .map(|(name, profile)| (name, profile.id.as_deref()))
        .collect();
    if users.is_empty() {
        return Ok(Vec::new());
    }
    secrets.status()?;
    let mut missing = Vec::new();
    for (name, id) in users {
        let saved = match id {
            Some(id) => secrets.get(id)?.is_some(),
            None => false,
        };
        if !saved {
            missing.push(name.clone());
        }
    }
    Ok(missing)
}

fn report_missing_passwords(config: &Config, secrets: &dyn SecretStore, from_backup: bool) {
    let missing = match missing_passwords(config, secrets) {
        Ok(missing) => missing,
        Err(err) => {
            eprintln!("lopi: note: cannot check saved passwords: {err:#}");
            return;
        }
    };
    let or_backup = if from_backup {
        ""
    } else {
        ", or restore a backup file: `lopi restore <file>.age`"
    };
    for name in missing {
        eprintln!(
            "lopi: note: '{name}' uses a password but none is saved; \
             set it with `lopi passwd {name}`{or_backup}"
        );
    }
}

/// Password profiles in `current` whose id does not appear in `restored`.
fn kept_passwords(current: &Config, restored: &Config) -> usize {
    let restored_ids: BTreeSet<&str> = restored
        .profiles
        .values()
        .filter_map(|p| p.id.as_deref())
        .collect();
    current
        .profiles
        .values()
        .filter(|p| p.uses_password())
        .filter_map(|p| p.id.as_deref())
        .filter(|id| !restored_ids.contains(id))
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyStatus {
    New,
    Same,
    Different,
    /// The stored path means nothing here (e.g. `D:\keys\id` on Linux).
    Unusable,
}

impl KeyStatus {
    fn describe(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Same => "already there (unchanged)",
            Self::Different => "a DIFFERENT file is already there",
            Self::Unusable => "path not usable on this computer (skipped)",
        }
    }
}

struct KeyAction {
    key: KeyFile,
    target: Option<PathBuf>,
    status: KeyStatus,
    write: bool,
}

impl KeyAction {
    fn target_text(&self) -> String {
        self.target
            .as_ref()
            .map_or_else(|| self.key.path.clone(), |t| t.display().to_string())
    }

    /// Writes the key (and its `.pub` file when missing) if decided so.
    fn write(&self) -> Result<Option<String>> {
        let (Some(target), true) = (&self.target, self.write) else {
            return Ok(None);
        };
        if let Some(dir) = target.parent() {
            create_private_dir(dir)?;
        }
        atomic::write(target, &self.key.private)
            .with_context(|| format!("cannot write {}", target.display()))?;
        if let Some(public) = &self.key.public {
            let mut public_path = target.clone().into_os_string();
            public_path.push(".pub");
            let public_path = PathBuf::from(public_path);
            if !public_path.exists() {
                atomic::write(&public_path, public)
                    .with_context(|| format!("cannot write {}", public_path.display()))?;
            }
        }
        Ok(Some(format!("restored key {}", target.display())))
    }
}

fn plan_keys(keys: Vec<KeyFile>, home: Option<&Path>) -> Vec<KeyAction> {
    keys.into_iter()
        .map(|key| {
            let target = expand_tilde(&key.path, home);
            if !target.is_absolute() {
                return KeyAction {
                    key,
                    target: None,
                    status: KeyStatus::Unusable,
                    write: false,
                };
            }
            let status = match fs::read(&target) {
                Ok(existing) if existing == *key.private => KeyStatus::Same,
                Ok(_) => KeyStatus::Different,
                Err(_) => KeyStatus::New,
            };
            KeyAction {
                key,
                target: Some(target),
                status,
                write: status == KeyStatus::New,
            }
        })
        .collect()
}

/// New keys are written; identical ones skipped; a different existing file is replaced
/// only with `--overwrite-keys` or a "yes" for that file (default no). Without a terminal
/// such files are skipped and reported.
fn decide_keys(keys: &mut [KeyAction], overwrite: bool) -> Result<()> {
    for key in keys.iter_mut().filter(|k| k.status == KeyStatus::Different) {
        key.write = if overwrite {
            true
        } else if tty::interactive() {
            TerminalPrompter.confirm(
                &format!(
                    "Replace the existing key {} with the one from the backup?",
                    key.target_text()
                ),
                false,
            )?
        } else {
            eprintln!(
                "lopi: note: kept the existing {} (use --overwrite-keys to replace it)",
                key.target_text()
            );
            false
        };
    }
    Ok(())
}

/// `~/.ssh` must not be readable by others (ssh refuses keys otherwise).
fn create_private_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Auth, Profile};
    use crate::secrets::memory::MemoryStore;

    fn key(path: &str, private: &[u8]) -> KeyFile {
        KeyFile {
            path: path.into(),
            private: Zeroizing::new(private.to_vec()),
            public: Some(b"PUB".to_vec()),
        }
    }

    #[test]
    fn keys_new_same_different_unusable() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        fs::create_dir_all(&ssh).unwrap();
        fs::write(ssh.join("same"), "S").unwrap();
        fs::write(ssh.join("diff"), "old").unwrap();
        let unusable = if cfg!(windows) {
            "relative/key"
        } else {
            r"D:\keys\id"
        };

        let keys = plan_keys(
            vec![
                key("~/.ssh/new", b"N"),
                key("~/.ssh/same", b"S"),
                key("~/.ssh/diff", b"new"),
                key(unusable, b"X"),
            ],
            Some(home.path()),
        );
        let statuses: Vec<KeyStatus> = keys.iter().map(|k| k.status).collect();
        assert_eq!(
            statuses,
            [
                KeyStatus::New,
                KeyStatus::Same,
                KeyStatus::Different,
                KeyStatus::Unusable
            ]
        );
        let writes: Vec<bool> = keys.iter().map(|k| k.write).collect();
        assert_eq!(writes, [true, false, false, false]);

        for key in &keys {
            key.write().unwrap();
        }
        assert_eq!(fs::read(ssh.join("new")).unwrap(), b"N");
        assert_eq!(fs::read(ssh.join("new.pub")).unwrap(), b"PUB");
        assert_eq!(
            fs::read(ssh.join("diff")).unwrap(),
            b"old",
            "kept without consent"
        );
    }

    #[test]
    fn missing_ssh_folder_is_created() {
        let home = tempfile::tempdir().unwrap();
        let keys = plan_keys(vec![key("~/.ssh/id", b"K")], Some(home.path()));
        keys[0].write().unwrap();
        let dir = home.path().join(".ssh");
        assert_eq!(fs::read(dir.join("id")).unwrap(), b"K");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(dir.join("id")).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    fn config(entries: &[(&str, &str, bool)]) -> Config {
        let mut config = Config::default();
        for (name, id, password) in entries {
            let profile = Profile {
                id: Some(id.to_string()),
                auth: password.then_some(Auth::Password),
                ..Profile::new("h")
            };
            config.profiles.insert(name.to_string(), profile);
        }
        config
    }

    #[test]
    fn passwords_go_only_to_restored_profiles() {
        let store = MemoryStore::default();
        let restored = config(&[("a", "id-a", true)]);
        let passwords = vec![
            ("id-a".to_string(), Zeroizing::new("pa".to_string())),
            ("id-gone".to_string(), Zeroizing::new("pg".to_string())),
        ];
        restore_passwords(&store, &restored, &passwords);
        assert_eq!(
            store.get("id-a").unwrap().as_deref().map(String::as_str),
            Some("pa")
        );
        assert_eq!(store.get("id-gone").unwrap(), None);
    }

    #[test]
    fn finds_password_profiles_without_a_saved_password() {
        let store = MemoryStore::default();
        store.set("id-a", "pa").unwrap();
        let restored = config(&[
            ("a", "id-a", true),
            ("b", "id-b", true),
            ("c", "id-c", false),
        ]);
        assert_eq!(missing_passwords(&restored, &store).unwrap(), ["b"]);

        // no password profiles: the credential store is not even asked
        let unavailable = MemoryStore {
            unavailable: true,
            ..MemoryStore::default()
        };
        let no_passwords = config(&[("c", "id-c", false)]);
        assert!(
            missing_passwords(&no_passwords, &unavailable)
                .unwrap()
                .is_empty()
        );
        assert!(missing_passwords(&restored, &unavailable).is_err());
    }

    #[test]
    fn passwords_of_missing_profiles_are_kept_and_counted() {
        let current = config(&[
            ("a", "id-a", true),
            ("b", "id-b", true),
            ("c", "id-c", false),
        ]);
        let restored = config(&[("a", "id-a", true)]);
        assert_eq!(kept_passwords(&current, &restored), 1);
    }

    #[test]
    fn snapshot_labels() {
        let snapshot = Snapshot {
            path: PathBuf::from("p"),
            time: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
            profiles: Some(3),
        };
        assert_eq!(
            snapshot_label(&snapshot, 1_700_000_000 + 300),
            "5m ago — 3 profiles (2023-11-14T22:13:20Z)"
        );
    }
}
