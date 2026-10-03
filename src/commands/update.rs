//! `lopi update [--check] [-y]`: replaces lopi with the latest release, when the install
//! script or `lopi install` put it there. Other installs only get told how to update.

use std::env;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use super::require_terminal;
use crate::askpass;
use crate::error::Abort;
use crate::install;
use crate::ui::prompt::{Prompter, TerminalPrompter};
use crate::update::channel::{self, Channel, Facts};
use crate::update::http::{ARCHIVE_LIMIT, CHECKSUM_LIMIT, Releases};
use crate::update::manifest::Manifest;
use crate::update::version::Version;
use crate::update::{ARCHIVE_SUFFIX, RELEASE_TARGET, archive, checksum};

/// Exit code 1 from `--check` means a newer release exists (like `doctor` finding a problem).
pub fn run(check: bool, yes: bool) -> Result<i32> {
    let exe = env::current_exe().context("cannot find this program's own file")?;
    let facts = Facts::gather(&exe);
    let channel = channel::detect(&facts);

    let releases = Releases::from_env()?;
    let manifest = Manifest::parse(&releases.latest_manifest()?)?;
    let latest = manifest.version()?;
    let current = Version::current();
    if latest == current {
        println!("lopi {current} is up to date");
        return Ok(0);
    }
    if latest < current {
        println!("lopi {current} is newer than the latest release ({latest}); nothing to do");
        return Ok(0);
    }
    println!(
        "lopi {latest} is available (you have {current}): {}",
        releases.page(latest)
    );
    if check {
        println!("to update, {}", channel.how_to_update());
        return Ok(1);
    }

    let target = &facts.exe;
    match channel {
        Channel::Cargo => bail!(
            "this lopi was installed with cargo; to update it, run \
             `cargo install lopi-ssh --locked`"
        ),
        Channel::Unknown => bail!(
            "lopi update only replaces a lopi put in place by the install script or \
             `lopi install`, and this one runs from {}; to update it, {}",
            target.display(),
            channel.how_to_update()
        ),
        Channel::LopiInstall | Channel::Installer { .. } => {}
    }
    let Some(triple) = RELEASE_TARGET else {
        bail!(
            "no prebuilt lopi is released for this system; update with `cargo install lopi-ssh --locked`"
        );
    };
    let archive = manifest.archive_for(triple, ARCHIVE_SUFFIX)?;

    if !yes {
        require_terminal("-y to update without confirmation")?;
        let question = format!("Update lopi at {} to {latest}?", target.display());
        if !TerminalPrompter.confirm(&question, true)? {
            return Err(Abort::Cancelled.into());
        }
    }

    println!("downloading {}", archive.name);
    let bytes = releases.asset(latest, &archive.name, ARCHIVE_LIMIT)?;
    let sums = releases.asset(latest, &archive.checksum_file, CHECKSUM_LIMIT)?;
    let expected = checksum::parse_sha256_file(&String::from_utf8_lossy(&sums), &archive.name)?;
    if archive
        .sha256
        .as_ref()
        .is_some_and(|listed| *listed != expected)
    {
        bail!(
            "the release lists two different checksums for {}; nothing was changed",
            archive.name
        );
    }
    checksum::verify(&bytes, &expected, &archive.name)?;
    let new_exe = archive::extract_exe(&bytes)?;
    install::replace_exe(target, &new_exe, |staged| reports_version(staged, latest))
        .context("nothing was changed")?;

    if let Channel::Installer { receipt } = &channel
        && let Err(err) = channel::set_receipt_version(receipt, latest)
    {
        eprintln!("lopi: warning: {err:#}");
    }
    println!("updated {} to lopi {latest}", target.display());
    Ok(0)
}

/// Runs the downloaded exe before it replaces this one: it must start on this system and
/// call itself the expected version.
fn reports_version(exe: &Path, expected: Version) -> Result<()> {
    let output = Command::new(exe)
        .arg("--version")
        .env_remove(askpass::ID_ENV)
        .env_remove(askpass::TARGET_ENV)
        .env_remove(askpass::REFUSE_ENV)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("the downloaded lopi does not start on this system")?;
    let printed = String::from_utf8_lossy(&output.stdout);
    let printed = printed.trim();
    if !output.status.success() || printed != format!("lopi {expected}") {
        bail!("the downloaded lopi says '{printed}' instead of 'lopi {expected}'");
    }
    Ok(())
}
