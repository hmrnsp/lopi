use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::config::Profile;

/// Builds the ssh argument list. Pure: no I/O, no environment lookups.
///
/// `extra` is what the user typed after the profile name. Everything before a `--` in it is
/// ssh options; everything after is the remote command. Result order:
///
/// `<user options> [-p port] [-i key] [-J jump] [-L/-R/-D spec]... -- [user@]host <remote command>`
///
/// User options come first because ssh keeps the first value it sees for options like `-p`
/// and `-o`, so the user can override the profile for one connection.
///
/// The profile must be valid (`Profile::validate`); `jump` must already be resolved to
/// hosts (profile names are translated by the caller).
pub fn build_args(profile: &Profile, extra: &[OsString], home: Option<&Path>) -> Vec<OsString> {
    let (options, command) = split_remote_command(extra);
    let mut args: Vec<OsString> = options.to_vec();
    if let Some(port) = profile.port {
        args.push("-p".into());
        args.push(port.to_string().into());
    }
    if let Some(key) = &profile.key {
        args.push("-i".into());
        args.push(expand_tilde(key, home).into());
    }
    if let Some(jump) = &profile.jump {
        args.push("-J".into());
        args.push(jump.into());
    }
    for forward in &profile.forward {
        // Validated as `L:spec`, `R:spec` or `D:spec`.
        if let Some((kind, spec)) = forward.split_once(':') {
            args.push(format!("-{kind}").into());
            args.push(spec.into());
        }
    }
    // `--` stops ssh from reading the destination as an option.
    args.push("--".into());
    args.push(destination(profile).into());
    args.extend_from_slice(command);
    args
}

/// Adds an option of lopi's own after the user's and the profile's options (so either
/// can override it, ssh keeping the first value), just before the `--` that `build_args`
/// puts in front of the destination.
pub fn add_option(args: &mut Vec<OsString>, option: &[&str]) {
    let at = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    args.splice(at..at, option.iter().map(OsString::from));
}

pub fn destination(profile: &Profile) -> String {
    match &profile.user {
        Some(user) => format!("{user}@{}", profile.host),
        None => profile.host.clone(),
    }
}

fn split_remote_command(extra: &[OsString]) -> (&[OsString], &[OsString]) {
    match extra.iter().position(|arg| arg == "--") {
        Some(i) => (&extra[..i], &extra[i + 1..]),
        None => (extra, &[]),
    }
}

/// Expands a leading `~` (`~`, `~/...`, and `~\...` on Windows). Other paths are unchanged.
pub fn expand_tilde(path: &str, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return PathBuf::from(path);
    };
    if path == "~" {
        return home.to_path_buf();
    }
    let rest = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("~\\").filter(|_| cfg!(windows)));
    let Some(rest) = rest else {
        return PathBuf::from(path);
    };
    let mut expanded = home.to_path_buf();
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    expanded.extend(rest.split(separators).filter(|part| !part.is_empty()));
    expanded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\Budi Santoso")
        } else {
            PathBuf::from("/home/budi")
        }
    }

    fn key_path(rest: &[&str]) -> OsString {
        let mut path = home();
        path.extend(rest);
        path.into_os_string()
    }

    fn base() -> Profile {
        Profile::new("103.1.2.3")
    }

    #[test]
    fn build_args_table() {
        let full = Profile {
            user: Some("root".into()),
            port: Some(2222),
            key: Some("~/.ssh/id_vps".into()),
            jump: Some("admin@bastion:2200,10.0.0.1".into()),
            forward: vec!["L:8080:localhost:80".into(), "D:1080".into()],
            note: Some("not an argument".into()),
            id: Some("abc".into()),
            updated_at: Some("2026-01-01T00:00:00Z".into()),
            ..base()
        };
        let mut full_expected = os(&["-p", "2222", "-i"]);
        full_expected.push(key_path(&[".ssh", "id_vps"]));
        full_expected.extend(os(&[
            "-J",
            "admin@bastion:2200,10.0.0.1",
            "-L",
            "8080:localhost:80",
            "-D",
            "1080",
            "--",
            "root@103.1.2.3",
        ]));

        let cases: Vec<(&str, Profile, Vec<OsString>, Vec<OsString>)> = vec![
            ("host only", base(), vec![], os(&["--", "103.1.2.3"])),
            (
                "user and port",
                Profile {
                    user: Some("admin".into()),
                    port: Some(22),
                    ..base()
                },
                vec![],
                os(&["-p", "22", "--", "admin@103.1.2.3"]),
            ),
            ("all fields", full.clone(), vec![], full_expected),
            (
                "extra options go first",
                Profile {
                    port: Some(2222),
                    ..base()
                },
                os(&["-L", "8080:localhost:80", "-p", "2200"]),
                os(&[
                    "-L",
                    "8080:localhost:80",
                    "-p",
                    "2200",
                    "-p",
                    "2222",
                    "--",
                    "103.1.2.3",
                ]),
            ),
            (
                "remote command after --",
                base(),
                os(&["-t", "--", "uptime", "-a"]),
                os(&["-t", "--", "103.1.2.3", "uptime", "-a"]),
            ),
            (
                "only remote command",
                base(),
                os(&["--", "ls"]),
                os(&["--", "103.1.2.3", "ls"]),
            ),
            (
                "only the first -- splits",
                base(),
                os(&["--", "sh", "--", "x"]),
                os(&["--", "103.1.2.3", "sh", "--", "x"]),
            ),
        ];

        let home = home();
        for (name, profile, extra, expected) in cases {
            assert_eq!(
                build_args(&profile, &extra, Some(&home)),
                expected,
                "case: {name}"
            );
        }
    }

    #[test]
    fn own_options_go_before_the_destination() {
        let profile = Profile {
            port: Some(2222),
            ..base()
        };
        let mut args = build_args(&profile, &os(&["-v", "--", "ls", "--", "x"]), None);
        add_option(&mut args, &["-o", "NumberOfPasswordPrompts=1"]);
        assert_eq!(
            args,
            os(&[
                "-v",
                "-p",
                "2222",
                "-o",
                "NumberOfPasswordPrompts=1",
                "--",
                "103.1.2.3",
                "ls",
                "--",
                "x"
            ])
        );
    }

    #[test]
    fn key_with_spaces_stays_one_argument() {
        let profile = Profile {
            key: Some("~/my keys/id vps".into()),
            ..base()
        };
        let args = build_args(&profile, &[], Some(&home()));
        assert_eq!(args.len(), 4);
        assert_eq!(args[1], key_path(&["my keys", "id vps"]));
    }

    #[test]
    fn tilde_expansion() {
        let home = home();
        assert_eq!(expand_tilde("~", Some(&home)), home);
        assert_eq!(
            expand_tilde("~/.ssh/id", Some(&home)).into_os_string(),
            key_path(&[".ssh", "id"])
        );
        assert_eq!(
            expand_tilde("/etc/key", Some(&home)),
            PathBuf::from("/etc/key")
        );
        assert_eq!(
            expand_tilde("~other/key", Some(&home)),
            PathBuf::from("~other/key")
        );
        assert_eq!(expand_tilde("~/.ssh/id", None), PathBuf::from("~/.ssh/id"));
        if cfg!(windows) {
            assert_eq!(
                expand_tilde(r"~\.ssh\id", Some(&home)).into_os_string(),
                key_path(&[".ssh", "id"])
            );
        }
    }
}
