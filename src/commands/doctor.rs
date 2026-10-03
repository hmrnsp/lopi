//! `lopi doctor`: checks that everything lopi relies on is in place and says how to
//! fix what is not. Exit code 1 when any check fails.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use crate::config::model::{Target, edit_flags, parse_target};
use crate::config::{Config, paths, store};
use crate::install;
use crate::secrets::{KeyringStore, SecretStore};
use crate::ssh::args::{destination, expand_tilde};
use crate::ssh::jump::jump_case_mismatches;
use crate::ssh::{self};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Ok,
    Warn,
    Fail,
}

struct Check {
    status: Status,
    text: String,
    fix: Option<String>,
}

impl Check {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            status: Status::Ok,
            text: text.into(),
            fix: None,
        }
    }
    fn warn(text: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            status: Status::Warn,
            text: text.into(),
            fix: Some(fix.into()),
        }
    }
    fn fail(text: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            status: Status::Fail,
            text: text.into(),
            fix: Some(fix.into()),
        }
    }
}

pub fn run() -> Result<i32> {
    let mut checks = vec![check_install()];
    let config = check_profiles(&mut checks);
    let ssh_version = check_ssh(config.as_ref(), &mut checks);
    if let Some(config) = &config {
        check_destinations(config, &mut checks);
        check_keys(config, &mut checks);
        check_passwords(config, ssh_version, &mut checks);
    }

    for check in &checks {
        let label = match check.status {
            Status::Ok => "ok  ",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
        };
        println!("{label}  {}", check.text);
        if let Some(fix) = &check.fix {
            println!("      fix: {fix}");
        }
    }
    let worst = checks.iter().map(|c| c.status).max().unwrap_or(Status::Ok);
    Ok(if worst == Status::Fail { 1 } else { 0 })
}

fn check_install() -> Check {
    let version = env!("CARGO_PKG_VERSION");
    if install::running_installed_copy() {
        return Check::ok(format!("lopi {version}, installed"));
    }
    let on_path = env::var_os("PATH")
        .and_then(|path| find_program("lopi", &path))
        .is_some();
    if on_path {
        Check::ok(format!("lopi {version}, on PATH"))
    } else {
        Check::warn(
            format!("lopi {version} is not installed and not on PATH"),
            "run `lopi install` so it works from any terminal",
        )
    }
}

fn check_profiles(checks: &mut Vec<Check>) -> Option<Config> {
    let path = match paths::config_file() {
        Ok(path) => path,
        Err(err) => {
            checks.push(Check::fail(
                format!("profiles file: {err:#}"),
                "check HOME/APPDATA",
            ));
            return None;
        }
    };
    match store::load_from(&path) {
        Ok(config) => {
            let count = config.profiles.len();
            let noun = if count == 1 { "profile" } else { "profiles" };
            checks.push(Check::ok(format!(
                "profiles: {} ({count} {noun})",
                path.display()
            )));
            for name in config.reserved_names() {
                checks.push(Check::warn(
                    format!("profile '{name}' clashes with a command"),
                    format!("rename it: `lopi edit {name} --rename <new>`"),
                ));
            }
            for (a, b) in config.case_duplicates() {
                checks.push(Check::warn(
                    format!("profiles '{a}' and '{b}' differ only in letter case"),
                    format!("rename one: `lopi edit {a} --rename <new>`"),
                ));
            }
            check_jump_names(&config, checks);
            Some(config)
        }
        Err(err) => {
            checks.push(Check::fail(
                format!("{err:#}"),
                "fix the file by hand, or `lopi restore` to go back to a snapshot",
            ));
            None
        }
    }
}

/// Returns ssh's (major, minor) version when it could be read.
fn check_ssh(config: Option<&Config>, checks: &mut Vec<Check>) -> Option<(u32, u32)> {
    let bin = ssh::ssh_bin(config.and_then(|c| c.ssh_bin.as_deref()));
    let Some(path) = locate(&bin) else {
        checks.push(Check::fail(
            format!("ssh not found ({})", bin.display()),
            "install the OpenSSH client, or set ssh_bin in the profiles file",
        ));
        return None;
    };
    let output = Command::new(&path).arg("-V").output();
    let text = output
        .map(|out| {
            let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stdout));
            text
        })
        .unwrap_or_default();
    let shown = path.display();
    let git = if is_git_ssh(&path) { ", Git's ssh" } else { "" };
    match parse_ssh_version(&text) {
        Some(version) => {
            checks.push(Check::ok(format!("ssh: {shown} ({}{git})", version.name)));
            Some((version.major, version.minor))
        }
        None => {
            checks.push(Check::warn(
                format!("ssh: {shown} (version unknown{git})"),
                "make sure it is OpenSSH",
            ));
            None
        }
    }
}

/// A jump item in another letter case than a profile is used as a host name.
fn check_jump_names(config: &Config, checks: &mut Vec<Check>) {
    for (name, profile) in &config.profiles {
        let Some(jump) = &profile.jump else { continue };
        let mismatches = jump_case_mismatches(config, jump);
        if mismatches.is_empty() {
            continue;
        }
        let fixed: Vec<&str> = jump
            .split(',')
            .map(|item| {
                mismatches
                    .iter()
                    .find(|(wrong, _)| *wrong == item)
                    .map_or(item, |(_, profile)| *profile)
            })
            .collect();
        for (item, other) in &mismatches {
            checks.push(Check::warn(
                format!(
                    "profile '{name}' jumps through '{item}', which is not a profile \
                     (did you mean '{other}'?)"
                ),
                format!(
                    "if you meant the profile: `lopi edit {name} --jump {}`",
                    fixed.join(",")
                ),
            ));
        }
    }
}

/// A port or `ssh://` inside the saved host or user, which `lopi add` used to accept
/// (`lopi add web example.com:2222`) and ssh cannot use as a host name.
fn check_destinations(config: &Config, checks: &mut Vec<Check>) {
    for (name, profile) in &config.profiles {
        let saved = destination(profile);
        let Ok(target) = parse_target(&saved) else {
            continue;
        };
        if target.user == profile.user && target.host == profile.host && target.port.is_none() {
            continue;
        }
        // Only what has to change: a user that is already right is left out of the fix.
        let fix = Target {
            user: target
                .user
                .clone()
                .filter(|user| profile.user.as_ref() != Some(user)),
            ..target
        };
        checks.push(Check::warn(
            format!("profile '{name}' has more than a host name in its address: {saved}"),
            format!("`lopi edit {name} {}`", edit_flags(&fix)),
        ));
    }
}

fn check_keys(config: &Config, checks: &mut Vec<Check>) {
    let home = dirs::home_dir();
    for (name, profile) in &config.profiles {
        let Some(key) = &profile.key else { continue };
        let path = expand_tilde(key, home.as_deref());
        if !path.is_file() {
            checks.push(Check::warn(
                format!("key for '{name}' not found: {}", path.display()),
                format!("copy the key there, or `lopi edit {name} --key <file>`"),
            ));
        } else if let Some(problem) = key_permission_problem(&path) {
            checks.push(Check::warn(
                format!("key for '{name}' {problem}: {}", path.display()),
                format!("chmod 600 {}", path.display()),
            ));
        }
    }
}

fn check_passwords(config: &Config, ssh_version: Option<(u32, u32)>, checks: &mut Vec<Check>) {
    let users: Vec<(&String, Option<&str>)> = config
        .profiles
        .iter()
        .filter(|(_, profile)| profile.uses_password())
        .map(|(name, profile)| (name, profile.id.as_deref()))
        .collect();
    if users.is_empty() {
        return;
    }
    if let Some(version) = ssh_version
        && !supports_askpass_require(version)
    {
        checks.push(Check::fail(
            format!(
                "saved passwords need OpenSSH 8.4 or newer (found {}.{})",
                version.0, version.1
            ),
            "update OpenSSH, or set ssh_bin to a newer ssh (Git for Windows ships one)",
        ));
    }
    let secrets = KeyringStore::new();
    if let Err(err) = secrets.status() {
        checks.push(Check::fail(
            format!("{err:#}"),
            "without it ssh asks for passwords every time",
        ));
        return;
    }
    let mut saved = 0;
    for (name, id) in &users {
        match id.map(|id| secrets.get(id)) {
            Some(Ok(Some(_))) => saved += 1,
            Some(Err(err)) => checks.push(Check::warn(
                format!("cannot read the password for '{name}': {err:#}"),
                format!("save it again: `lopi passwd {name}`"),
            )),
            _ => checks.push(Check::warn(
                format!("'{name}' uses password login but has no saved password"),
                format!("`lopi passwd {name}`"),
            )),
        }
    }
    checks.push(Check::ok(format!(
        "system credential store: {saved} of {} passwords saved",
        users.len()
    )));
}

/// `OpenSSH_10.2p1, OpenSSL …` or `OpenSSH_for_Windows_9.5p2, LibreSSL …`.
#[derive(Debug, PartialEq, Eq)]
struct SshVersion {
    name: String,
    major: u32,
    minor: u32,
}

fn parse_ssh_version(text: &str) -> Option<SshVersion> {
    let name = text
        .split([',', ' ', '\n', '\r'])
        .find(|word| word.starts_with("OpenSSH_"))?;
    let digits = name
        .trim_start_matches("OpenSSH_")
        .trim_start_matches("for_Windows_");
    let (major, rest) = digits.split_once('.')?;
    let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some(SshVersion {
        name: name.to_string(),
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
    })
}

/// `SSH_ASKPASS_REQUIRE`, which saved passwords rely on, arrived in OpenSSH 8.4.
fn supports_askpass_require((major, minor): (u32, u32)) -> bool {
    (major, minor) >= (8, 4)
}

/// Splits on both separators, so a Windows path is also recognized on Linux (tests).
fn is_git_ssh(path: &Path) -> bool {
    path.to_string_lossy()
        .split(['\\', '/'])
        .any(|part| part.eq_ignore_ascii_case("Git"))
}

/// A path to an existing file: `bin` itself when it has a folder, else searched on PATH.
fn locate(bin: &OsStr) -> Option<PathBuf> {
    let path = Path::new(bin);
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    find_program(bin.to_str()?, &env::var_os("PATH")?)
}

fn find_program(name: &str, path: &OsStr) -> Option<PathBuf> {
    let names = [
        name.to_string(),
        format!("{name}{}", env::consts::EXE_SUFFIX),
    ];
    env::split_paths(path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

#[cfg(unix)]
fn key_permission_problem(path: &Path) -> Option<&'static str> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    (mode & 0o077 != 0).then_some("is readable by other users (ssh will refuse it)")
}

#[cfg(not(unix))]
fn key_permission_problem(_path: &Path) -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_versions() {
        let cases = [
            (
                "OpenSSH_10.2p1, OpenSSL 3.5.4 30 Sep 2025",
                Some(("OpenSSH_10.2p1", 10, 2)),
            ),
            (
                "OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2",
                Some(("OpenSSH_for_Windows_9.5p2", 9, 5)),
            ),
            (
                "OpenSSH_8.2p1 Ubuntu-4ubuntu0.11, OpenSSL 1.1.1f",
                Some(("OpenSSH_8.2p1", 8, 2)),
            ),
            ("usage: ssh [-46AaCfGgKkMNnqsTtVvXxYy]", None),
            ("", None),
        ];
        for (text, expected) in cases {
            let parsed = parse_ssh_version(text);
            let expected = expected.map(|(name, major, minor)| SshVersion {
                name: name.into(),
                major,
                minor,
            });
            assert_eq!(parsed, expected, "{text:?}");
        }
    }

    #[test]
    fn askpass_require_needs_8_4() {
        assert!(!supports_askpass_require((8, 3)));
        assert!(supports_askpass_require((8, 4)));
        assert!(supports_askpass_require((9, 5)));
        assert!(supports_askpass_require((10, 0)));
        assert!(!supports_askpass_require((7, 9)));
    }

    #[test]
    fn finds_programs_on_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join(format!("fakessh{}", env::consts::EXE_SUFFIX));
        std::fs::write(&file, "").unwrap();
        let path = env::join_paths([Path::new("/nonexistent"), dir.path()]).unwrap();
        assert_eq!(find_program("fakessh", &path), Some(file));
        assert_eq!(find_program("other", &path), None);
    }

    #[test]
    fn recognizes_gits_ssh() {
        assert!(is_git_ssh(Path::new(
            r"C:\Program Files\Git\usr\bin\ssh.exe"
        )));
        assert!(!is_git_ssh(Path::new(
            r"C:\Windows\System32\OpenSSH\ssh.exe"
        )));
    }
}
