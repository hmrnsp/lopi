//! `lopi ping [name]`: checks that a profile's ssh server answers, without logging in.
//! The system's ssh does the work (so ports, jump hosts and `~/.ssh/config` apply); it is
//! told to offer no login method at all, so the server's "Permission denied" is the
//! answer that proves it is there. Nothing is ever asked, and `known_hosts` is never
//! changed.

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use super::connect::prepare;
use super::require_terminal;
use crate::askpass;
use crate::config::{Profile, store};
use crate::resolve::resolve;
use crate::ssh::{self, args::build_args};
use crate::state;
use crate::ui::picker::pick_profile;
use crate::ui::prompt::TerminalPrompter;

/// Given before the profile's own options, so they win (ssh keeps the first value).
/// `PreferredAuthentications=none` offers no login method; `LogLevel=INFO` makes ssh
/// report the closed connection after a jump host fails (see [`through_jump`]); a server that lets anyone in
/// without authentication would run `true`, which changes nothing. `ControlPath=none`
/// keeps an open multiplexed connection from answering in the server's place, and
/// `ClearAllForwardings` keeps forwards from `~/.ssh/config` from opening local ports.
const OPTIONS: &[&str] = &[
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "PreferredAuthentications=none",
    "-o",
    "ConnectTimeout=5",
    "-o",
    "LogLevel=INFO",
    "-o",
    "ControlPath=none",
    "-o",
    "ClearAllForwardings=yes",
];

/// ssh is stopped after this, also when a jump host hangs (`ConnectTimeout` is per hop).
const LIMIT: Duration = Duration::from_secs(15);

pub fn run(name: Option<String>) -> Result<i32> {
    let config = store::load()?;
    let name = match &name {
        Some(name) => resolve(&config, name)?.0,
        None => {
            require_terminal("lopi ping <name>")?;
            pick_profile(&config, &state::load(), &mut TerminalPrompter, "Ping")?
        }
    };
    let profile = prepare(&config, name)?;
    let args = ping_args(&profile, dirs::home_dir().as_deref());
    let exe = env::current_exe().context("cannot find this program's own file")?;
    // A jump host's ssh does not inherit BatchMode: lopi answers its prompts with "no".
    let env: Vec<(OsString, OsString)> = vec![
        ("SSH_ASKPASS".into(), exe.into_os_string()),
        ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
        (askpass::REFUSE_ENV.into(), "1".into()),
    ];
    let bin = ssh::ssh_bin(config.ssh_bin.as_deref());
    let captured = ssh::run_captured(&bin, &args, &env, LIMIT)?;
    let outcome = if captured.timed_out {
        Outcome::Unreachable(format!("no answer within {} s", LIMIT.as_secs()))
    } else {
        classify(captured.success, &captured.stderr)
    };

    let via = profile
        .jump
        .as_deref()
        .map(|jump| format!(" (via {jump})"))
        .unwrap_or_default();
    let millis = captured.elapsed.as_millis();
    let answered = format!("{name}: ok, the ssh server answered in {millis} ms{via}");
    let (line, code) = match &outcome {
        Outcome::Answered { methods } => {
            let login = methods
                .as_deref()
                .map(|methods| format!(" (login: {methods})"))
                .unwrap_or_default();
            (format!("{answered}{login}"), 0)
        }
        Outcome::LetIn => (
            format!("{answered}, and let lopi in without any authentication"),
            0,
        ),
        Outcome::UnknownHostKey => (
            format!(
                "{answered}; its host key is not known yet: connect once with `lopi {name}` \
                 to check it and save it"
            ),
            0,
        ),
        Outcome::HostKeyChanged => (
            format!(
                "{name}: FAIL, a host key has CHANGED since you last connected{via} (see \
                 ssh's message below). Someone may be intercepting the connection: do not \
                 connect until you have checked the new key with the server's admin"
            ),
            1,
        ),
        Outcome::Unreachable(reason) => (format!("{name}: FAIL, {reason}{via}"), 1),
    };
    println!("{line}");
    // What ssh said, when lopi's summary may not be the whole story.
    if code != 0 {
        for said in captured.stderr.lines().filter(|l| !l.trim().is_empty()) {
            eprintln!("  ssh: {said}");
        }
    }
    Ok(code)
}

/// The ssh arguments for a ping: the profile's own (port, key, jump), no forwards, and
/// [`OPTIONS`]. `profile` comes from [`prepare`].
fn ping_args(profile: &Profile, home: Option<&Path>) -> Vec<OsString> {
    let profile = Profile {
        forward: Vec::new(),
        ..profile.clone()
    };
    let mut extra: Vec<OsString> = OPTIONS.iter().map(OsString::from).collect();
    extra.push("--".into());
    extra.push("true".into());
    build_args(&profile, &extra, home)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// The server refused the empty login: it is there. `methods` are the login methods
    /// it offers, as ssh reports them.
    Answered {
        methods: Option<String>,
    },
    /// The server accepted no authentication at all (ssh exited with 0).
    LetIn,
    /// The server answered, but its host key is not in `known_hosts` yet.
    UnknownHostKey,
    HostKeyChanged,
    Unreachable(String),
}

/// What ssh's exit status and messages mean. ssh exits with 255 for every failure, so
/// the messages decide, read from the last line back.
fn classify(success: bool, stderr: &str) -> Outcome {
    // Checked anywhere in the output, for the destination and for jump hosts alike.
    if stderr
        .to_lowercase()
        .contains("remote host identification has changed")
    {
        return Outcome::HostKeyChanged;
    }
    if success {
        return Outcome::LetIn;
    }
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    classify_lines(&lines)
}

fn classify_lines(lines: &[&str]) -> Outcome {
    let Some((last, before)) = lines.split_last() else {
        return Outcome::Unreachable("ssh failed without saying why".into());
    };
    let lower = last.to_lowercase();
    let closed = [
        "connection closed",
        "connection reset",
        "kex_exchange_identification",
    ]
    .iter()
    .any(|word| lower.contains(word));
    if closed && !before.is_empty() {
        return through_jump(before);
    }
    if lower.contains("permission denied") {
        let methods = last
            .rsplit_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .map(|(methods, _)| methods.to_string())
            .filter(|methods| !methods.is_empty());
        return Outcome::Answered { methods };
    }
    if lower.contains("host key verification failed") {
        return Outcome::UnknownHostKey;
    }
    let reason = if [
        "could not resolve",
        "name or service not known",
        "name resolution",
    ]
    .iter()
    .any(|word| lower.contains(word))
    {
        "cannot resolve the host name".to_string()
    } else if lower.contains("connection refused") {
        "connection refused: nothing listens on that port".to_string()
    } else if lower.contains("timed out") {
        "connection timed out".to_string()
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        "no route to the host".to_string()
    } else if lower.contains("administratively prohibited") {
        "the jump host does not allow forwarding (AllowTcpForwarding)".to_string()
    } else if closed {
        "the connection was closed before the ssh server answered".to_string()
    } else {
        format!("ssh failed: {last}")
    };
    Outcome::Unreachable(reason)
}

/// ssh reported a closed connection after other messages: a jump host failed. Its ssh
/// prints its own error first (with `LogLevel=INFO`, ssh then adds the closed line), or,
/// when the jump host cannot reach the destination, `channel 0: open failed: ...`.
fn through_jump(before: &[&str]) -> Outcome {
    if let Some(open_failed) = before.iter().rfind(|line| line.contains("open failed")) {
        return match classify_lines(&[open_failed]) {
            Outcome::Unreachable(reason) => {
                Outcome::Unreachable(format!("the jump host cannot reach it: {reason}"))
            }
            other => other,
        };
    }
    let reason = match classify_lines(before) {
        Outcome::Answered { .. } => {
            "the jump host refused the login (lopi ping never types a password: use a key \
             or ssh-agent for the jump host)"
                .to_string()
        }
        Outcome::UnknownHostKey => {
            "the jump host's host key is not known yet; connect once to check it and save it"
                .to_string()
        }
        Outcome::Unreachable(reason) => format!("jump host: {reason}"),
        other => return other,
    };
    Outcome::Unreachable(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreachable(reason: &str) -> Outcome {
        Outcome::Unreachable(reason.to_string())
    }

    #[test]
    fn classifies_what_ssh_says() {
        let answered = |methods: &str| Outcome::Answered {
            methods: Some(methods.to_string()),
        };
        let cases = [
            (
                "admin@10.0.0.5: Permission denied (publickey,password).\n",
                answered("publickey,password"),
            ),
            ("Permission denied (publickey).", answered("publickey")),
            ("Host key verification failed.\n", Outcome::UnknownHostKey),
            (
                "@@@@@@@@@@@\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
                 @@@@@@@@@@@\nHost key verification failed.\n",
                Outcome::HostKeyChanged,
            ),
            (
                "ssh: Could not resolve hostname nohost.invalid: Name or service not known\n",
                unreachable("cannot resolve the host name"),
            ),
            (
                "ssh: connect to host 127.0.0.1 port 22: Connection refused\n",
                unreachable("connection refused: nothing listens on that port"),
            ),
            (
                "ssh: connect to host 10.0.0.9 port 22: Connection timed out\n",
                unreachable("connection timed out"),
            ),
            (
                "ssh: connect to host 10.0.0.9 port 22: No route to host\n",
                unreachable("no route to the host"),
            ),
            ("", unreachable("ssh failed without saying why")),
            (
                "Bad configuration option: x\n",
                unreachable("ssh failed: Bad configuration option: x"),
            ),
        ];
        for (stderr, expected) in cases {
            assert_eq!(classify(false, stderr), expected, "{stderr:?}");
        }
        assert_eq!(classify(true, ""), Outcome::LetIn);
    }

    #[test]
    fn a_failing_jump_host_is_not_an_answer_from_the_destination() {
        let closed = "Connection closed by UNKNOWN port 65535";
        let cases = [
            (
                format!("admin@zayd.example.com: Permission denied (publickey).\n{closed}\n"),
                "the jump host refused the login (lopi ping never types a password: use a \
                 key or ssh-agent for the jump host)",
            ),
            (
                format!("Host key verification failed.\n{closed}\n"),
                "the jump host's host key is not known yet; connect once to check it and save it",
            ),
            (
                format!("ssh: connect to host 127.0.0.1 port 1: Connection refused\n{closed}\n"),
                "jump host: connection refused: nothing listens on that port",
            ),
            (
                format!(
                    "channel 0: open failed: connect failed: Connection refused\n\
                     stdio forwarding failed\n{closed}\n"
                ),
                "the jump host cannot reach it: connection refused: nothing listens on that port",
            ),
            (
                format!(
                    "channel 0: open failed: connect failed: Name or service not known\n\
                     stdio forwarding failed\n{closed}\n"
                ),
                "the jump host cannot reach it: cannot resolve the host name",
            ),
            (
                "kex_exchange_identification: Connection closed by remote host\n".to_string(),
                "the connection was closed before the ssh server answered",
            ),
        ];
        for (stderr, expected) in cases {
            assert_eq!(
                classify(false, &stderr),
                unreachable(expected),
                "{stderr:?}"
            );
        }
    }

    #[test]
    fn args_never_log_in_or_forward() {
        let profile = Profile {
            user: Some("admin".into()),
            port: Some(2222),
            jump: Some("admin@[2001:db8::1]:22".into()),
            forward: vec!["L:8080:localhost:80".into()],
            ..Profile::new("10.0.0.5")
        };
        let args: Vec<String> = ping_args(&profile, None)
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect();
        let mut expected: Vec<&str> = OPTIONS.to_vec();
        expected.extend([
            "-p",
            "2222",
            "-J",
            "admin@[2001:db8::1]:22",
            "--",
            "admin@10.0.0.5",
            "true",
        ]);
        assert_eq!(args, expected);
    }
}
