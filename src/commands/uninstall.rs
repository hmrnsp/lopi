use anyhow::{Context, Result};

use super::require_terminal;
use crate::config::paths;
use crate::error::Abort;
use crate::install;
use crate::ui::prompt::{Prompter, TerminalPrompter};

/// `lopi uninstall`: removes the installed exe, its PATH entry and the PowerShell
/// completion block. Profiles, backups and history are kept.
pub fn run(yes: bool) -> Result<i32> {
    let target = install::installed_exe()?;
    let dir = target.parent().context("install path has no folder")?;
    if !yes {
        require_terminal("-y to uninstall without confirmation")?;
        let question = format!("Remove lopi from {}?", dir.display());
        if !TerminalPrompter.confirm(&question, false)? {
            return Err(Abort::Cancelled.into());
        }
    }

    let mut changed = remove_from_path(dir)?;
    changed |= disable_completion();
    if target.exists() {
        install::remove_exe(&target)?;
        println!("deleted {}", target.display());
        changed = true;
    }

    if changed {
        println!(
            "lopi is uninstalled; your profiles are kept in {} (backups and history in {})",
            paths::config_file()?.display(),
            paths::data_dir()?.display()
        );
    } else {
        println!("lopi is not installed; nothing to remove");
    }
    Ok(0)
}

#[cfg(windows)]
fn remove_from_path(dir: &std::path::Path) -> Result<bool> {
    let removed = install::remove_from_path(&mut install::windows::RegistryPath, dir)?;
    if removed {
        println!("removed {} from your PATH", dir.display());
    }
    Ok(removed)
}

#[cfg(not(windows))]
fn remove_from_path(_dir: &std::path::Path) -> Result<bool> {
    Ok(false)
}

/// Both PowerShell profiles are checked, whether or not PowerShell 7 is still installed.
/// A problem with one profile is reported but does not stop the uninstall.
#[cfg(windows)]
fn disable_completion() -> bool {
    use crate::install::powershell::{self, Outcome};

    let Some(documents) = dirs::document_dir() else {
        return false;
    };
    let mut changed = false;
    for folder in ["WindowsPowerShell", "PowerShell"] {
        let profile = documents.join(folder).join("profile.ps1");
        match powershell::disable(&profile) {
            Ok(Outcome::Changed) => {
                println!("removed Tab completion from {}", profile.display());
                changed = true;
            }
            Ok(Outcome::Unchanged) => {}
            Err(err) => eprintln!("lopi: warning: {err:#}"),
        }
    }
    changed
}

#[cfg(not(windows))]
fn disable_completion() -> bool {
    false
}
