use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use zeroize::Zeroizing;

use super::require_terminal;
use crate::atomic;
use crate::backup_bundle::{self, Bundle, KeyFile};
use crate::config::{Config, paths, store};
use crate::error::Abort;
use crate::secrets::{KeyringStore, SecretStore};
use crate::ssh::args::expand_tilde;
use crate::time;
use crate::ui::prompt::{Prompter, TerminalPrompter};

/// Passphrases shorter than this get a warning (they are allowed after confirming).
const SHORT_PASSPHRASE: usize = 12;

/// `lopi backup [FILE]`: profiles, saved passwords and private key files in one
/// passphrase-encrypted file, for moving to another PC or recovering this one.
pub fn run(file: Option<PathBuf>, no_keys: bool, force: bool) -> Result<i32> {
    require_terminal(
        "a terminal (the backup passphrase is typed in, never passed as an argument)",
    )?;
    let config_path = paths::config_file()?;
    let Some(profiles) = store::read_text(&config_path)? else {
        bail!(
            "nothing to back up yet: {} does not exist",
            config_path.display()
        );
    };
    let config = store::parse(&profiles).with_context(|| {
        format!(
            "{} is not valid, so it cannot be backed up; fix it first (`lopi doctor`)",
            config_path.display()
        )
    })?;

    let target = file.unwrap_or_else(default_file_name);
    if target.exists()
        && !force
        && !TerminalPrompter.confirm(&format!("{} exists. Replace it?", target.display()), false)?
    {
        return Err(Abort::Cancelled.into());
    }

    let passwords = collect_passwords(&config);
    let keys = if no_keys {
        Vec::new()
    } else {
        collect_keys(&config)
    };
    let passphrase = ask_passphrase(&mut TerminalPrompter)?;

    let bundle = Bundle {
        created_at: time::now_rfc3339(),
        lopi_version: env!("CARGO_PKG_VERSION").to_string(),
        profiles,
        passwords,
        keys,
    };
    let plain = backup_bundle::pack(&bundle)?;
    let encrypted = backup_bundle::encrypt(&plain, &passphrase, None)?;
    atomic::write(&target, &encrypted)
        .with_context(|| format!("cannot write {}", target.display()))?;

    println!(
        "backed up {} to {}",
        summary(
            config.profiles.len(),
            bundle.passwords.len(),
            bundle.keys.len()
        ),
        target.display()
    );
    println!("keep the passphrase safe: without it this file cannot be opened, not even by lopi");
    Ok(0)
}

fn default_file_name() -> PathBuf {
    let stamp = time::compact_utc(SystemTime::now());
    PathBuf::from(format!("lopi-backup-{}.age", &stamp[..8]))
}

pub fn summary(profiles: usize, passwords: usize, keys: usize) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "{}, {} and {}",
        plural(profiles, "profile", "profiles"),
        plural(passwords, "password", "passwords"),
        plural(keys, "key file", "key files")
    )
}

/// Saved passwords of profiles that use them. The credential store is only opened when
/// there is something to read; problems are warnings, the rest is still backed up.
fn collect_passwords(config: &Config) -> BTreeMap<String, Zeroizing<String>> {
    let mut passwords = BTreeMap::new();
    let users: Vec<(&String, Option<&String>)> = config
        .profiles
        .iter()
        .filter(|(_, profile)| profile.uses_password())
        .map(|(name, profile)| (name, profile.id.as_ref()))
        .collect();
    if users.is_empty() {
        return passwords;
    }
    let secrets = KeyringStore::new();
    if let Err(err) = secrets.status() {
        eprintln!("lopi: warning: saved passwords are not included: {err:#}");
        return passwords;
    }
    for (name, id) in users {
        match id.map(|id| (id, secrets.get(id))) {
            Some((id, Ok(Some(password)))) => {
                passwords.insert(id.clone(), password);
            }
            Some((_, Err(err))) => {
                eprintln!("lopi: warning: password for '{name}' not included: {err:#}")
            }
            _ => eprintln!("lopi: warning: '{name}' has no saved password to include"),
        }
    }
    passwords
}

/// Every key file the profiles use, once each, with its `.pub` file when present.
fn collect_keys(config: &Config) -> Vec<KeyFile> {
    let home = dirs::home_dir();
    let stored: BTreeSet<&String> = config
        .profiles
        .values()
        .filter_map(|p| p.key.as_ref())
        .collect();
    let mut keys = Vec::new();
    for path in stored {
        let on_disk = expand_tilde(path, home.as_deref());
        match fs::read(&on_disk) {
            Ok(private) => {
                let mut public_path = on_disk.clone().into_os_string();
                public_path.push(".pub");
                keys.push(KeyFile {
                    path: path.clone(),
                    private: Zeroizing::new(private),
                    public: fs::read(public_path).ok(),
                });
            }
            Err(err) => eprintln!(
                "lopi: warning: key file {} not included: {err}",
                on_disk.display()
            ),
        }
    }
    keys
}

/// Asked twice. A short one is allowed only after a clear warning.
fn ask_passphrase(prompter: &mut dyn Prompter) -> Result<Zeroizing<String>> {
    loop {
        let passphrase = prompter.password("Passphrase for this backup:", true)?;
        if passphrase.chars().count() >= SHORT_PASSPHRASE
            || prompter.confirm(
                &format!(
                    "That passphrase is shorter than {SHORT_PASSPHRASE} characters, so the file is easier to break into. Use it anyway?"
                ),
                false,
            )?
        {
            return Ok(passphrase);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::prompt::scripted::{Answer::*, Scripted};

    #[test]
    fn short_passphrases_need_confirmation() {
        let mut p = Scripted::new([Secret("short"), No, Secret("a much longer passphrase")]);
        assert_eq!(
            ask_passphrase(&mut p).unwrap().as_str(),
            "a much longer passphrase"
        );

        let mut p = Scripted::new([Secret("short"), Yes]);
        assert_eq!(ask_passphrase(&mut p).unwrap().as_str(), "short");
    }

    #[test]
    fn summaries() {
        assert_eq!(summary(1, 0, 2), "1 profile, 0 passwords and 2 key files");
        assert_eq!(summary(3, 1, 1), "3 profiles, 1 password and 1 key file");
    }

    #[test]
    fn default_name_is_dated() {
        let name = default_file_name().to_string_lossy().into_owned();
        assert!(
            name.starts_with("lopi-backup-20") && name.ends_with(".age"),
            "{name}"
        );
        assert_eq!(name.len(), "lopi-backup-YYYYMMDD.age".len());
    }

    #[test]
    fn keys_are_collected_once_with_public_halves() {
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("id_vps");
        fs::write(&private, "PRIVATE").unwrap();
        fs::write(dir.path().join("id_vps.pub"), "PUBLIC").unwrap();
        let path = private.to_string_lossy().into_owned();
        let mut config = Config::default();
        for name in ["a", "b"] {
            let profile = crate::config::Profile {
                key: Some(path.clone()),
                ..crate::config::Profile::new("h")
            };
            config.profiles.insert(name.into(), profile);
        }
        let missing = crate::config::Profile {
            key: Some(dir.path().join("gone").to_string_lossy().into_owned()),
            ..crate::config::Profile::new("h")
        };
        config.profiles.insert("c".into(), missing);

        let keys = collect_keys(&config);
        assert_eq!(keys.len(), 1, "same key once; missing one skipped");
        assert_eq!(keys[0].private.as_slice(), b"PRIVATE");
        assert_eq!(keys[0].public.as_deref(), Some(&b"PUBLIC"[..]));
    }
}
