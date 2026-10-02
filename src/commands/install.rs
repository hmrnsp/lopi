use std::env;
use std::path::Path;

use anyhow::{Context, Result};

use crate::install::{self, Copied};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `lopi install`: copies this exe to the per-user folder, puts that folder on PATH
/// and optionally enables PowerShell Tab completion (`completion`: `None` = ask).
/// Each step reports as it finishes, so a later failure never hides what already changed.
pub fn run(completion: Option<bool>) -> Result<i32> {
    let source = env::current_exe().context("cannot find this program's own file")?;
    let target = install::installed_exe()?;
    let dir = target.parent().context("install path has no folder")?;

    match install::copy_exe(&source, &target)? {
        Copied::Installed => println!("installed lopi {VERSION} to {}", target.display()),
        Copied::Replaced => println!("updated {} to lopi {VERSION}", target.display()),
        Copied::AlreadyThere => {
            println!("lopi {VERSION} is installed at {}", target.display())
        }
    }
    let path_changed = add_to_path(dir)?;
    enable_completion(completion)?;

    if path_changed {
        println!("done: open a new terminal (PowerShell, cmd or Git Bash) and run `lopi`");
    } else {
        println!("done: run `lopi` from any terminal");
    }
    Ok(0)
}

#[cfg(windows)]
fn add_to_path(dir: &Path) -> Result<bool> {
    let added = install::add_to_path(&mut install::windows::RegistryPath, dir)?;
    if added {
        println!("added {} to your PATH", dir.display());
    }
    Ok(added)
}

/// Linux and macOS: shell startup files are not edited; when `~/.local/bin` is not on PATH
/// (the macOS default), the line to add is printed.
#[cfg(not(windows))]
fn add_to_path(dir: &Path) -> Result<bool> {
    let on_path =
        env::var_os("PATH").is_some_and(|path| env::split_paths(&path).any(|entry| entry == dir));
    if !on_path {
        println!(
            "note: {} is not on your PATH; add this line to ~/.bashrc or ~/.zshrc:\n  export PATH=\"{}:$PATH\"",
            dir.display(),
            dir.display()
        );
    }
    Ok(false)
}

#[cfg(windows)]
fn enable_completion(wanted: Option<bool>) -> Result<()> {
    use crate::install::powershell::{self, Outcome};
    use crate::ui::prompt::{Prompter, TerminalPrompter};
    use crate::ui::tty;

    let wanted = match wanted {
        Some(wanted) => wanted,
        None => {
            tty::interactive()
                && TerminalPrompter.confirm("Enable Tab completion in PowerShell?", true)?
        }
    };
    if !wanted {
        return Ok(());
    }
    if let Some(policy) = install::windows::execution_policy()
        && powershell::policy_blocks_profiles(&policy)
    {
        println!(
            "note: Tab completion not enabled: PowerShell's execution policy is {policy}, so \
             profile scripts do not run. To allow them, run in PowerShell:\n  \
             Set-ExecutionPolicy -Scope CurrentUser RemoteSigned\nthen `lopi install --completion`"
        );
        return Ok(());
    }
    let Some(documents) = dirs::document_dir() else {
        println!("note: Tab completion not enabled: cannot find your Documents folder");
        return Ok(());
    };
    for profile in powershell::profile_paths(&documents) {
        match powershell::enable(&profile) {
            Ok(Outcome::Changed) => {
                println!("enabled Tab completion in {}", profile.display())
            }
            Ok(Outcome::Unchanged) => {
                println!("Tab completion is already enabled in {}", profile.display())
            }
            // The exe and PATH are done; a profile problem should not undo that.
            Err(err) => eprintln!("lopi: warning: {err:#}"),
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn enable_completion(wanted: Option<bool>) -> Result<()> {
    if wanted == Some(true) {
        println!("note: set up Tab completion as shown by `lopi completion --help`");
    }
    Ok(())
}
