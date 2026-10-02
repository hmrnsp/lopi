//! `lopi install` / `uninstall`: put this exe in a per-user folder on `PATH`, no admin
//! rights needed. Pure parts (`path_list`, `profile_block`) are separate so they can be
//! tested without touching the real system.

pub mod path_list;
#[cfg(windows)]
pub mod powershell;
pub mod profile_block;
#[cfg(windows)]
pub mod windows;

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::atomic;

/// Overrides the install folder (used by tests).
pub const INSTALL_DIR_ENV: &str = "LOPI_INSTALL_DIR";

/// Windows: `%LOCALAPPDATA%\Programs\lopi` (a folder of our own).
/// Linux: `~/.local/bin` (shared with other programs, usually already on `PATH`).
pub fn install_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os(INSTALL_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    if cfg!(windows) {
        let local = dirs::data_local_dir().context("cannot find the local app data folder")?;
        Ok(local.join("Programs").join("lopi"))
    } else {
        dirs::executable_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("bin")))
            .context("cannot find ~/.local/bin")
    }
}

pub fn installed_exe() -> Result<PathBuf> {
    Ok(install_dir()?.join(format!("lopi{}", env::consts::EXE_SUFFIX)))
}

/// Whether the running program is the installed copy.
pub fn running_installed_copy() -> bool {
    match (env::current_exe(), installed_exe()) {
        (Ok(running), Ok(installed)) => same_file(&running, &installed),
        _ => false,
    }
}

/// The user's own `PATH` setting, as stored (not the merged, expanded process `PATH`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathValue {
    pub text: String,
    /// Stored as an expandable string (Windows `REG_EXPAND_SZ`), so `%VAR%` keeps working.
    pub expandable: bool,
}

pub trait UserPath {
    fn read(&self) -> Result<Option<PathValue>>;
    fn write(&mut self, value: &PathValue) -> Result<()>;
}

/// Adds `dir` to the user `PATH`. `Ok(false)` when it was already there.
pub fn add_to_path(user_path: &mut dyn UserPath, dir: &Path) -> Result<bool> {
    let current = user_path.read()?.unwrap_or(PathValue {
        text: String::new(),
        expandable: true,
    });
    let Some(text) = path_list::with_dir(&current.text, &path_text(dir)?) else {
        return Ok(false);
    };
    let expandable = current.expandable || text.contains('%');
    user_path.write(&PathValue { text, expandable })?;
    Ok(true)
}

/// Removes `dir` from the user `PATH`. `Ok(false)` when it was not there.
pub fn remove_from_path(user_path: &mut dyn UserPath, dir: &Path) -> Result<bool> {
    let Some(current) = user_path.read()? else {
        return Ok(false);
    };
    let Some(text) = path_list::without_dir(&current.text, &path_text(dir)?) else {
        return Ok(false);
    };
    user_path.write(&PathValue {
        text,
        expandable: current.expandable,
    })?;
    Ok(true)
}

fn path_text(dir: &Path) -> Result<String> {
    dir.to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{} is not valid text", dir.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copied {
    /// No copy existed before.
    Installed,
    /// An older or different copy was replaced.
    Replaced,
    /// The running program already is the installed copy.
    AlreadyThere,
}

/// Copies `source` to `target`. A running `target` (another lopi still open) cannot
/// be overwritten on Windows, but it can be renamed: it is moved to `<target>.old`, which
/// the next install or uninstall deletes.
pub fn copy_exe(source: &Path, target: &Path) -> Result<Copied> {
    if same_file(source, target) {
        return Ok(Copied::AlreadyThere);
    }
    remove_leftover(target);
    let bytes = fs::read(source).with_context(|| format!("cannot read {}", source.display()))?;
    let existed = target.exists();
    if let Err(err) = atomic::write(target, &bytes) {
        if !(existed && err.kind() == io::ErrorKind::PermissionDenied) {
            return Err(err).with_context(|| format!("cannot write {}", target.display()));
        }
        fs::rename(target, old_path(target))
            .and_then(|()| atomic::write(target, &bytes))
            .with_context(|| format!("cannot replace {} (is it in use?)", target.display()))?;
    }
    make_executable(target)?;
    Ok(if existed {
        Copied::Replaced
    } else {
        Copied::Installed
    })
}

/// Deletes the installed exe. When that is the running program, deletion is handed to
/// `self_replace`, which moves it out of the way now and deletes it after exit.
pub fn remove_exe(target: &Path) -> Result<()> {
    remove_leftover(target);
    if !target.exists() {
        return Ok(());
    }
    let running = env::current_exe().is_ok_and(|exe| same_file(&exe, target));
    if running {
        let dir = target.parent().unwrap_or(target);
        self_replace::self_delete_outside_path(dir)
    } else {
        fs::remove_file(target)
    }
    .with_context(|| format!("cannot delete {}", target.display()))?;
    // Windows: the folder is lopi's own; remove it once empty. Never on Linux, where
    // ~/.local/bin is shared.
    if cfg!(windows)
        && let Some(dir) = target.parent()
    {
        let _ = fs::remove_dir(dir);
    }
    Ok(())
}

fn old_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

/// A `.old` copy may still be running; then it stays until a later attempt.
fn remove_leftover(target: &Path) {
    let _ = fs::remove_file(old_path(target));
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("cannot make {} executable", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// In-memory user `PATH` for tests.
    #[derive(Default)]
    pub struct FakePath(pub Option<PathValue>);

    impl UserPath for FakePath {
        fn read(&self) -> Result<Option<PathValue>> {
            Ok(self.0.clone())
        }
        fn write(&mut self, value: &PathValue) -> Result<()> {
            self.0 = Some(value.clone());
            Ok(())
        }
    }

    fn value(text: &str, expandable: bool) -> Option<PathValue> {
        Some(PathValue {
            text: text.into(),
            expandable,
        })
    }

    #[test]
    fn path_add_and_remove_keep_the_value_type() {
        let dir = Path::new(r"C:\Programs\lopi");
        let mut path = FakePath(value(r"C:\a", false));
        assert!(add_to_path(&mut path, dir).unwrap());
        assert_eq!(path.0, value(r"C:\a;C:\Programs\lopi", false));
        assert!(!add_to_path(&mut path, dir).unwrap(), "added once only");

        assert!(remove_from_path(&mut path, dir).unwrap());
        assert_eq!(path.0, value(r"C:\a", false));
        assert!(!remove_from_path(&mut path, dir).unwrap());

        // no user PATH yet: created as an expandable value
        let mut empty = FakePath::default();
        add_to_path(&mut empty, dir).unwrap();
        assert_eq!(empty.0, value(r"C:\Programs\lopi", true));
    }

    #[test]
    fn copy_install_replace_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("download").join("lopi.exe");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"v2").unwrap();
        let target = dir.path().join("programs").join("lopi.exe");

        assert_eq!(copy_exe(&source, &target).unwrap(), Copied::Installed);
        assert_eq!(fs::read(&target).unwrap(), b"v2");
        assert_eq!(copy_exe(&source, &target).unwrap(), Copied::Replaced);
        assert_eq!(copy_exe(&target, &target).unwrap(), Copied::AlreadyThere);

        fs::write(old_path(&target), b"leftover").unwrap();
        remove_exe(&target).unwrap();
        assert!(!target.exists());
        assert!(!old_path(&target).exists());
        // the dedicated folder goes away on Windows only
        assert_eq!(target.parent().unwrap().exists(), !cfg!(windows));
    }

    #[cfg(unix)]
    #[test]
    fn installed_copy_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src");
        fs::write(&source, b"x").unwrap();
        let target = dir.path().join("bin").join("lopi");
        copy_exe(&source, &target).unwrap();
        let mode = fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}
