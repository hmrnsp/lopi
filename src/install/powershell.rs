//! Tab completion for PowerShell via the per-user profile scripts ("CurrentUserAllHosts").

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::profile_block::{add_block, remove_block};
use crate::atomic;

/// Does nothing if lopi was uninstalled without cleaning the profile.
pub const SETUP_LINE: &str = "if (Get-Command lopi -ErrorAction SilentlyContinue) { lopi completion powershell | Out-String | Invoke-Expression }";

/// Policies under which a local, unsigned profile script does not run (it would print
/// an error every time PowerShell starts).
pub fn policy_blocks_profiles(policy: &str) -> bool {
    policy.eq_ignore_ascii_case("Restricted") || policy.eq_ignore_ascii_case("AllSigned")
}

/// Windows PowerShell 5.1 always; PowerShell 7 when it is installed (its folder exists
/// or `pwsh.exe` is on PATH). `documents` is the real Documents folder (it may be
/// redirected to OneDrive).
pub fn profile_paths(documents: &Path) -> Vec<PathBuf> {
    let mut paths = vec![documents.join("WindowsPowerShell").join("profile.ps1")];
    let pwsh_dir = documents.join("PowerShell");
    if pwsh_dir.is_dir() || pwsh_on_path() {
        paths.push(pwsh_dir.join("profile.ps1"));
    }
    paths
}

fn pwsh_on_path() -> bool {
    env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|dir| dir.join("pwsh.exe").is_file()))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Changed,
    Unchanged,
}

/// Adds the completion block to `profile` (creating it if needed).
pub fn enable(profile: &Path) -> Result<Outcome> {
    edit(profile, |text| Ok(add_block(text, SETUP_LINE)))
}

/// Removes the completion block from `profile`, if present.
pub fn disable(profile: &Path) -> Result<Outcome> {
    if !profile.exists() {
        return Ok(Outcome::Unchanged);
    }
    edit(profile, |text| {
        remove_block(text).map_err(|err| anyhow::anyhow!("{err}; remove it by hand"))
    })
}

fn edit(profile: &Path, change: impl FnOnce(&str) -> Result<Option<String>>) -> Result<Outcome> {
    let bytes = match fs::read(profile) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", profile.display())),
    };
    // Writing UTF-8 into a UTF-16 file would corrupt it.
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    let Some(text) = String::from_utf8(bytes).ok().filter(|_| !utf16) else {
        anyhow::bail!(
            "{} is not UTF-8 text (maybe UTF-16), which lopi does not edit; \
             add or remove this line yourself:\n  {SETUP_LINE}",
            profile.display()
        );
    };
    let Some(new_text) = change(&text).with_context(|| profile.display().to_string())? else {
        return Ok(Outcome::Unchanged);
    };
    // A profile that only held lopi's block (usually one install created) goes away.
    if new_text.trim_start_matches('\u{feff}').trim().is_empty() {
        fs::remove_file(profile).with_context(|| format!("cannot delete {}", profile.display()))?;
    } else {
        atomic::write(profile, new_text.as_bytes())
            .with_context(|| format!("cannot write {}", profile.display()))?;
    }
    Ok(Outcome::Changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_and_disable_keep_the_users_lines() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("WindowsPowerShell").join("profile.ps1");

        assert_eq!(enable(&profile).unwrap(), Outcome::Changed);
        assert_eq!(enable(&profile).unwrap(), Outcome::Unchanged);
        assert!(fs::read_to_string(&profile).unwrap().contains(SETUP_LINE));

        fs::write(
            &profile,
            format!(
                "Set-Alias ll ls\r\n{}",
                fs::read_to_string(&profile).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(disable(&profile).unwrap(), Outcome::Changed);
        assert_eq!(fs::read_to_string(&profile).unwrap(), "Set-Alias ll ls\r\n");
        assert_eq!(disable(&profile).unwrap(), Outcome::Unchanged);
        assert_eq!(
            disable(&dir.path().join("missing.ps1")).unwrap(),
            Outcome::Unchanged
        );
    }

    #[test]
    fn a_profile_created_by_install_is_deleted_by_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("PowerShell").join("profile.ps1");
        enable(&profile).unwrap();
        assert_eq!(disable(&profile).unwrap(), Outcome::Changed);
        assert!(!profile.exists());
    }

    #[test]
    fn utf16_profiles_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile.ps1");
        fs::write(&profile, [0xFF, 0xFE, b'a', 0]).unwrap();
        let err = enable(&profile).unwrap_err().to_string();
        assert!(err.contains("UTF-16"), "{err}");
        assert_eq!(fs::read(&profile).unwrap(), [0xFF, 0xFE, b'a', 0]);
    }

    #[test]
    fn policies() {
        assert!(policy_blocks_profiles("Restricted"));
        assert!(policy_blocks_profiles("AllSigned"));
        assert!(!policy_blocks_profiles("RemoteSigned"));
        assert!(!policy_blocks_profiles("Unrestricted"));
        assert!(!policy_blocks_profiles("Bypass"));
    }

    #[test]
    fn windows_powershell_profile_is_always_included() {
        let dir = tempfile::tempdir().unwrap();
        let paths = profile_paths(dir.path());
        assert_eq!(
            paths[0],
            dir.path().join("WindowsPowerShell").join("profile.ps1")
        );
        fs::create_dir_all(dir.path().join("PowerShell")).unwrap();
        assert_eq!(profile_paths(dir.path()).len(), 2);
    }
}
