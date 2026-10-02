use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use toml_edit::DocumentMut;

use super::model::{Config, SCHEMA_VERSION};
use super::{backup, document, meta, paths};
use crate::lock::{self, FileLock};
use crate::{atomic, time};

/// Where the profiles file and its backups live.
#[derive(Debug, Clone)]
pub struct StorePaths {
    pub file: PathBuf,
    pub backups: PathBuf,
}

impl StorePaths {
    /// The real locations, honoring `LOPI_CONFIG` and `LOPI_DATA_DIR`.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            file: paths::config_file()?,
            backups: paths::data_dir()?.join("backups"),
        })
    }
}

/// Loads the profiles file and prints warnings about suspicious names once.
/// A missing file is an empty config; a broken file is an error (never treated as empty,
/// so it is never overwritten).
pub fn load() -> Result<Config> {
    let config = load_from(&paths::config_file()?)?;
    warn_about(&config);
    Ok(config)
}

/// Prints warnings for names that work badly: they clash with a subcommand, or differ
/// from another name only in letter case.
pub fn warn_about(config: &Config) {
    for name in config.reserved_names() {
        eprintln!(
            "lopi: warning: profile '{name}' clashes with a subcommand; connect with `lopi connect {name}`"
        );
    }
    for (a, b) in config.case_duplicates() {
        eprintln!(
            "lopi: warning: profiles '{a}' and '{b}' differ only in letter case; rename one with `lopi edit {a} --rename <new>`"
        );
    }
}

/// Changes the profiles file through [`mutate_in`]. Together with [`replace_in`] these are
/// the only ways to write it; both go through `commit`.
///
/// `change` runs before the bookkeeping fields are filled in: a profile it adds has no id
/// yet. Use [`mutate_saved`] when the caller needs the config as saved.
pub fn mutate<T>(change: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
    mutate_in(&StorePaths::from_env()?, change)
}

/// Like [`mutate`], and also returns the config as saved, where every profile (a new one
/// too) has its id and `updated_at`.
pub fn mutate_saved<T>(change: impl FnOnce(&mut Config) -> Result<T>) -> Result<(T, Config)> {
    mutate_saved_in(&StorePaths::from_env()?, change)
}

/// Like [`load`] for an explicit path, without printing anything.
pub fn load_from(path: &Path) -> Result<Config> {
    let text = read_text(path)?.unwrap_or_default();
    parse_file(path, &text)
}

/// Parses profiles file text. Error messages include the line and column.
pub fn parse(text: &str) -> Result<Config> {
    Ok(toml::from_str(text)?)
}

fn parse_file(path: &Path, text: &str) -> Result<Config> {
    parse(text).with_context(|| {
        format!(
            "{} is not valid; fix it by hand (nothing was changed)",
            path.display()
        )
    })
}

/// The file's text, `None` if it does not exist. A UTF-8 byte order mark (added by some
/// Windows editors) is dropped.
pub fn read_text(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(match text.strip_prefix('\u{feff}') {
            Some(rest) => rest.to_string(),
            None => text,
        })),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// Lock → load → `change` → stamp metadata → validate → edit the document → verify →
/// back up → write atomically → unlock. If any step fails, the file is left untouched.
/// When nothing changed, nothing is written (and no backup is made).
pub fn mutate_in<T>(
    store: &StorePaths,
    change: impl FnOnce(&mut Config) -> Result<T>,
) -> Result<T> {
    mutate_saved_in(store, change).map(|(out, _)| out)
}

/// [`mutate_in`], also returning the config as saved (see [`mutate_saved`]).
pub fn mutate_saved_in<T>(
    store: &StorePaths,
    change: impl FnOnce(&mut Config) -> Result<T>,
) -> Result<(T, Config)> {
    let path = store.file.as_path();
    let _lock = FileLock::acquire(path, lock::DEFAULT_TIMEOUT)?;
    let text = read_text(path)?.unwrap_or_default();
    let parsed = parse_file(path, &text)?;
    check_writable(path, &parsed)?;
    let mut doc: DocumentMut = text
        .parse()
        .with_context(|| format!("{} is not valid", path.display()))?;

    let mut before = parsed.clone();
    meta::ensure_ids(&mut before);
    document::write_ids(&mut doc, &parsed, &before)?;

    let mut after = before.clone();
    let out = change(&mut after)?;
    meta::stamp(&before, &mut after, &time::now_rfc3339())?;
    after.schema_version = SCHEMA_VERSION;
    after
        .validate()
        .context("refusing to save an invalid config")?;

    document::apply(&mut doc, &before, &after)?;
    let new_text = match_line_endings(&text, doc.to_string());
    // Safety net: what is written must read back as exactly the intended config.
    if parse(&new_text).ok().as_ref() != Some(&after) {
        bail!(
            "internal error: the updated file would not match the intended change; \
             {} was not changed",
            path.display()
        );
    }
    commit(store, &text, &new_text)?;
    Ok((out, after))
}

/// Replaces the whole profiles file with `new_text` (used by `restore`), byte for byte,
/// comments included. The new text must be a valid profiles file this lopi can write.
/// The current file is not parsed: restoring is also how a broken file gets fixed. It is
/// still backed up first, so the replacement can itself be restored away.
pub fn replace_in(store: &StorePaths, new_text: &str) -> Result<Config> {
    let path = store.file.as_path();
    let _lock = FileLock::acquire(path, lock::DEFAULT_TIMEOUT)?;
    let old_text = read_text(path)?.unwrap_or_default();
    let config = parse(new_text).context("the replacement is not a valid profiles file")?;
    check_writable(path, &config)?;
    config
        .validate()
        .context("refusing to restore an invalid profiles file")?;
    commit(store, &old_text, new_text)?;
    Ok(config)
}

/// The one place the profiles file is written: back up the current file, then replace it
/// atomically. Writes nothing when the text is unchanged. The caller holds the lock.
fn commit(store: &StorePaths, old_text: &str, new_text: &str) -> Result<()> {
    if new_text == old_text {
        return Ok(());
    }
    let path = store.file.as_path();
    backup::snapshot(path, &store.backups, SystemTime::now())
        .context("cannot back up the profiles file, so nothing was changed")?;
    atomic::write(path, new_text.as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))
}

/// A file from a newer lopi may be read, but never written.
fn check_writable(path: &Path, config: &Config) -> Result<()> {
    if config.schema_version > SCHEMA_VERSION {
        bail!(
            "{} uses format version {}, but this lopi only writes version {SCHEMA_VERSION}; \
             upgrade lopi (nothing was changed)",
            path.display(),
            config.schema_version
        );
    }
    Ok(())
}

/// Keeps a file that uses Windows line endings consistent after lines were added.
fn match_line_endings(original: &str, new_text: String) -> String {
    if original.contains("\r\n") {
        new_text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        new_text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;

    fn profile(host: &str) -> Profile {
        Profile {
            user: Some("root".into()),
            port: Some(2222),
            key: Some("~/.ssh/id_vps".into()),
            ..Profile::new(host)
        }
    }

    fn temp_file(text: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        if let Some(text) = text {
            fs::write(&path, text).unwrap();
        }
        (dir, path)
    }

    fn store_for(path: &Path) -> StorePaths {
        StorePaths {
            file: path.to_path_buf(),
            backups: path.with_file_name("backups"),
        }
    }

    fn mutate_at<T>(path: &Path, change: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
        mutate_in(&store_for(path), change)
    }

    fn add(path: &Path, name: &str, profile: Profile) -> Result<()> {
        mutate_at(path, |cfg| {
            cfg.profiles.insert(name.into(), profile);
            Ok(())
        })
    }

    #[test]
    fn parse_profiles() {
        let cfg = parse(
            r#"
            [profiles.kantor]
            host = "10.0.0.5"
            user = "admin"
            port = 2200
            future_field = "kept compatible"

            [profiles.vps]
            host = "example.com"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.profiles.len(), 2);
        assert_eq!(cfg.profiles["kantor"].port, Some(2200));
        assert_eq!(cfg.profiles["vps"].user, None);
    }

    #[test]
    fn broken_file_is_an_error_with_location() {
        let err = parse("[profiles.kantor\nhost = 1").unwrap_err();
        assert!(format!("{err:#}").contains("line 1"), "{err:#}");
    }

    #[test]
    fn missing_file_is_empty() {
        let (_dir, path) = temp_file(None);
        assert!(load_from(&path).unwrap().profiles.is_empty());
    }

    #[test]
    fn mutate_creates_dirs_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("profiles.toml");
        add(&path, "vps", profile("103.1.2.3")).unwrap();
        let cfg = load_from(&path).unwrap();
        let saved = &cfg.profiles["vps"];
        assert!(saved.same_content(&profile("103.1.2.3")));
        assert!(saved.id.is_some() && saved.updated_at.is_some());
        assert_eq!(cfg.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn mutate_saved_returns_the_config_as_saved() {
        let (_dir, path) = temp_file(Some("[profiles.old]\nhost = \"old\"\n"));
        let (seen_by_change, saved) = mutate_saved_in(&store_for(&path), |cfg| {
            cfg.profiles.insert("new".into(), profile("new"));
            Ok(cfg.profiles["new"].id.clone())
        })
        .unwrap();
        // The change runs before ids are given out; the returned config has them.
        assert_eq!(seen_by_change, None);
        assert!(saved.profiles.values().all(|p| p.id.is_some()));
        assert!(saved.profiles["new"].updated_at.is_some());
        assert_eq!(saved, load_from(&path).unwrap());
    }

    #[test]
    fn every_real_change_is_backed_up_first() {
        let (_dir, path) = temp_file(None);
        let backups = store_for(&path).backups;
        add(&path, "a", profile("a")).unwrap(); // no file yet: nothing to back up
        assert_eq!(backup::list(&backups).unwrap().len(), 0);

        let before = fs::read_to_string(&path).unwrap();
        add(&path, "b", profile("b")).unwrap();
        let snapshots = backup::list(&backups).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(fs::read_to_string(&snapshots[0]).unwrap(), before);

        // a change that changes nothing writes nothing
        mutate_at(&path, |_| Ok(())).unwrap();
        assert_eq!(backup::list(&backups).unwrap().len(), 1);
    }

    #[test]
    fn failed_backup_leaves_file_untouched() {
        let (dir, path) = temp_file(Some("[profiles.a]\nhost = \"a\"\n"));
        // a file where the backup directory should be makes the backup fail
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "").unwrap();
        let store = StorePaths {
            file: path.clone(),
            backups: blocker,
        };
        let result = mutate_in(&store, |cfg| {
            cfg.profiles.insert("b".into(), profile("b"));
            Ok(())
        });
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("cannot back up"), "{err}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[profiles.a]\nhost = \"a\"\n"
        );
    }

    #[test]
    fn concurrent_saves_do_not_lose_changes() {
        let (_dir, path) = temp_file(None);
        std::thread::scope(|scope| {
            for i in 0..8 {
                let path = &path;
                scope.spawn(move || add(path, &format!("p{i}"), profile("h")).unwrap());
            }
        });
        assert_eq!(load_from(&path).unwrap().profiles.len(), 8);
    }

    #[test]
    fn comments_and_unknown_fields_survive() {
        let (_dir, path) = temp_file(Some(
            "# my servers\n[profiles.a]\nhost = \"a\" # main\ncolor = \"blue\"\n",
        ));
        add(&path, "b", profile("b")).unwrap();
        mutate_at(&path, |cfg| {
            cfg.profiles.get_mut("a").unwrap().port = Some(22);
            Ok(())
        })
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        for kept in [
            "# my servers\n",
            "host = \"a\" # main\n",
            "color = \"blue\"\n",
        ] {
            assert!(text.contains(kept), "{kept:?} missing in:\n{text}");
        }
    }

    #[test]
    fn v01_file_gets_ids_without_touching_other_timestamps() {
        let (_dir, path) = temp_file(Some(
            "[profiles.a]\nhost = \"a\"\n[profiles.b]\nhost = \"b\"\n",
        ));
        mutate_at(&path, |cfg| {
            cfg.profiles.get_mut("b").unwrap().port = Some(22);
            Ok(())
        })
        .unwrap();
        let cfg = load_from(&path).unwrap();
        assert!(cfg.profiles.values().all(|p| p.id.is_some()));
        assert_eq!(cfg.profiles["a"].updated_at, None);
        assert!(cfg.profiles["b"].updated_at.is_some());
    }

    #[test]
    fn hand_copied_duplicate_id_is_repaired() {
        let (_dir, path) = temp_file(Some(
            "[profiles.a]\nhost = \"a\"\nid = \"x\"\n[profiles.b]\nhost = \"b\"\nid = \"x\"\n",
        ));
        mutate_at(&path, |_| Ok(())).unwrap();
        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.profiles["a"].id.as_deref(), Some("x"));
        assert_ne!(cfg.profiles["b"].id.as_deref(), Some("x"));
    }

    #[test]
    fn bom_and_crlf_are_handled() {
        let (_dir, path) = temp_file(Some("\u{feff}[profiles.a]\r\nhost = \"a\"\r\n"));
        add(&path, "b", profile("b")).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.starts_with('\u{feff}'));
        assert!(
            !text.replace("\r\n", "").contains('\n'),
            "mixed line endings"
        );
        assert_eq!(load_from(&path).unwrap().profiles.len(), 2);
    }

    #[test]
    fn replace_restores_text_exactly_and_backs_up_first() {
        let current = "[profiles.a]\nhost = \"a\"\n";
        let (_dir, path) = temp_file(Some(current));
        let store = store_for(&path);
        let restored = "# kept comment\n[profiles.b]\nhost = \"b\" # note\n";

        let config = replace_in(&store, restored).unwrap();
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), restored);
        let snapshots = backup::list(&store.backups).unwrap();
        assert_eq!(fs::read_to_string(&snapshots[0]).unwrap(), current);
    }

    #[test]
    fn replace_fixes_a_broken_file_but_rejects_bad_replacements() {
        let (_dir, path) = temp_file(Some("this is = = broken"));
        let store = store_for(&path);
        for bad in [
            "not = = toml",
            "[profiles.x]\nhost = \"-oProxyCommand=evil\"\n",
            "schema_version = 99\n",
        ] {
            assert!(replace_in(&store, bad).is_err(), "{bad:?}");
            assert_eq!(fs::read_to_string(&path).unwrap(), "this is = = broken");
        }
        replace_in(&store, "[profiles.ok]\nhost = \"h\"\n").unwrap();
        assert_eq!(load_from(&path).unwrap().profiles.len(), 1);
    }

    #[test]
    fn newer_schema_is_read_but_never_written() {
        let text = "schema_version = 2\n[profiles.a]\nhost = \"a\"\n";
        let (_dir, path) = temp_file(Some(text));
        assert_eq!(load_from(&path).unwrap().profiles.len(), 1);
        let err = mutate_at(&path, |_| Ok(())).unwrap_err();
        assert!(format!("{err:#}").contains("upgrade lopi"), "{err:#}");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn failed_change_or_validation_leaves_file_untouched() {
        let (_dir, path) = temp_file(None);
        add(&path, "vps", profile("h")).unwrap();
        let before = fs::read(&path).unwrap();

        assert!(add(&path, "bad", profile("-oProxyCommand=x")).is_err());
        assert!(mutate_at(&path, |_| -> Result<()> { bail!("nope") }).is_err());

        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn broken_file_is_never_overwritten() {
        let (_dir, path) = temp_file(Some("this is = = not toml"));
        assert!(add(&path, "vps", profile("h")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "this is = = not toml");
    }
}
