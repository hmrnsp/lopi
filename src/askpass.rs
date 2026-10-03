//! lopi as ssh's askpass program (`SSH_ASKPASS`). When connecting to a profile with a
//! saved password, ssh runs lopi again with the prompt as its argument and reads the
//! answer from stdout. The saved password is only given to a password prompt for the
//! profile's own `user@host`; every other prompt (a jump host's password, a host key
//! confirmation, a key passphrase, a 2FA code) is asked on the terminal, so a password
//! never goes to the wrong server and host keys are never confirmed automatically.

use std::env;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

use anyhow::{Context, Result};
use zeroize::Zeroizing;

use crate::secrets::{KeyringStore, SecretStore};

/// Profile id whose saved password may be used; its presence switches on askpass mode.
pub const ID_ENV: &str = "LOPI_ASKPASS_ID";
/// `user@host` (or `host`) the saved password belongs to.
pub const TARGET_ENV: &str = "LOPI_ASKPASS_TARGET";
/// Set by `lopi ping`: every prompt is refused, so a jump host's ssh (which does not
/// inherit `BatchMode`) never waits for an answer.
pub const REFUSE_ENV: &str = "LOPI_ASKPASS_REFUSE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Password,
    /// Secret, but not the saved password (key passphrase, one-time code, PIN).
    Hidden,
    /// Shown while typing (host key confirmation and anything unknown).
    Visible,
}

pub fn classify(prompt: &str) -> PromptKind {
    let lower = prompt.trim().to_lowercase();
    if lower.ends_with("password:") {
        PromptKind::Password
    } else if ["passphrase", "code", "token", "otp", "pin"]
        .iter()
        .any(|word| lower.contains(word))
    {
        PromptKind::Hidden
    } else {
        PromptKind::Visible
    }
}

/// Whether a password prompt is for `target` (`user@host` or `host`). ssh writes
/// `user@host's password:` or, for keyboard-interactive, `(user@host) Password:`.
pub fn prompt_is_for(prompt: &str, target: &str) -> bool {
    let prompt = prompt.to_lowercase();
    let target = target.to_lowercase();
    let (user, host) = match target.split_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, target.as_str()),
    };
    let names_host = prompt.contains(&format!("@{host}'s password"))
        || prompt.contains(&format!("@{host}) password"));
    let names_user = user.is_none_or(|user| {
        prompt.contains(&format!("{user}@{host}'s")) || prompt.contains(&format!("({user}@{host})"))
    });
    names_host && names_user
}

/// Reading from the user's terminal directly: in askpass mode stdin and stdout belong to
/// ssh.
pub trait Terminal {
    fn hidden(&mut self, prompt: &str) -> Result<Zeroizing<String>>;
    fn visible(&mut self, prompt: &str) -> Result<String>;
}

/// The answer to give ssh for `prompt`.
pub fn answer(
    prompt: &str,
    id: &str,
    target: &str,
    store: &dyn SecretStore,
    terminal: &mut dyn Terminal,
) -> Result<Zeroizing<String>> {
    match classify(prompt) {
        PromptKind::Password if prompt_is_for(prompt, target) => match store.get(id) {
            Ok(Some(password)) => Ok(password),
            // Not saved (or the store failed): let the user type it.
            _ => terminal.hidden(prompt),
        },
        PromptKind::Password | PromptKind::Hidden => terminal.hidden(prompt),
        PromptKind::Visible => terminal.visible(prompt).map(Zeroizing::new),
    }
}

/// Called first thing in `main`: runs askpass mode when ssh started us as `SSH_ASKPASS`,
/// returning the exit code. `None` means a normal run.
pub fn run_if_requested() -> Option<i32> {
    if env::var_os(REFUSE_ENV).is_some_and(|value| !value.is_empty()) {
        return Some(1);
    }
    let id = env::var(ID_ENV).ok().filter(|id| !id.is_empty())?;
    let target = env::var(TARGET_ENV).unwrap_or_default();
    let prompt = env::args().nth(1).unwrap_or_default();
    let reply = answer(
        &prompt,
        &id,
        &target,
        &KeyringStore::new(),
        &mut ConsoleTerminal,
    );
    Some(match reply {
        Ok(text) => {
            let mut out = std::io::stdout().lock();
            let written = out
                .write_all(text.as_bytes())
                .and_then(|()| out.write_all(b"\n"))
                .and_then(|()| out.flush());
            if written.is_ok() { 0 } else { 1 }
        }
        // Cancelled or no terminal: ssh treats a failing askpass as "no answer".
        Err(_) => 1,
    })
}

struct ConsoleTerminal;

#[cfg(windows)]
const CONSOLE_IN: &str = "CONIN$";
#[cfg(windows)]
const CONSOLE_OUT: &str = "CONOUT$";
#[cfg(not(windows))]
const CONSOLE_IN: &str = "/dev/tty";
#[cfg(not(windows))]
const CONSOLE_OUT: &str = "/dev/tty";

impl Terminal for ConsoleTerminal {
    fn hidden(&mut self, prompt: &str) -> Result<Zeroizing<String>> {
        rpassword::prompt_password(prompt)
            .map(Zeroizing::new)
            .context("cannot read from the terminal")
    }

    fn visible(&mut self, prompt: &str) -> Result<String> {
        let mut out = OpenOptions::new()
            .write(true)
            .open(CONSOLE_OUT)
            .context("cannot open the terminal")?;
        out.write_all(prompt.as_bytes())?;
        out.flush()?;
        let input = OpenOptions::new()
            .read(true)
            .open(CONSOLE_IN)
            .context("cannot open the terminal")?;
        let mut line = String::new();
        BufReader::new(input).read_line(&mut line)?;
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::MemoryStore;

    #[test]
    fn prompt_kinds() {
        let cases = [
            ("root@10.0.0.5's password: ", PromptKind::Password),
            ("(root@10.0.0.5) Password: ", PromptKind::Password),
            ("Password:", PromptKind::Password),
            (
                "Enter passphrase for key '/home/b/.ssh/id': ",
                PromptKind::Hidden,
            ),
            ("Verification code: ", PromptKind::Hidden),
            ("Enter PIN for ECDSA-SK key: ", PromptKind::Hidden),
            (
                "The authenticity of host 'h (1.2.3.4)' can't be established.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ",
                PromptKind::Visible,
            ),
        ];
        for (prompt, kind) in cases {
            assert_eq!(classify(prompt), kind, "{prompt:?}");
        }
    }

    #[test]
    fn password_prompts_are_matched_to_the_profile() {
        assert!(prompt_is_for("root@10.0.0.5's password: ", "root@10.0.0.5"));
        assert!(prompt_is_for(
            "(Root@Example.COM) Password: ",
            "root@example.com"
        ));
        assert!(
            prompt_is_for("budi@vps's password: ", "vps"),
            "no user in the profile"
        );
        // a jump host, another user, a look-alike host
        assert!(!prompt_is_for(
            "admin@bastion's password: ",
            "root@10.0.0.5"
        ));
        assert!(!prompt_is_for(
            "admin@10.0.0.5's password: ",
            "root@10.0.0.5"
        ));
        assert!(!prompt_is_for(
            "root@10.0.0.55's password: ",
            "root@10.0.0.5"
        ));
        assert!(
            !prompt_is_for("Password: ", "root@10.0.0.5"),
            "names no host"
        );
    }

    /// Records what was asked and replies with fixed text.
    #[derive(Default)]
    struct FakeTerminal {
        asked: Vec<(bool, String)>,
    }

    impl Terminal for FakeTerminal {
        fn hidden(&mut self, prompt: &str) -> Result<Zeroizing<String>> {
            self.asked.push((true, prompt.into()));
            Ok(Zeroizing::new("typed".into()))
        }
        fn visible(&mut self, prompt: &str) -> Result<String> {
            self.asked.push((false, prompt.into()));
            Ok("no".into())
        }
    }

    #[test]
    fn saved_password_goes_only_to_its_own_server() {
        let store = MemoryStore::default();
        store.set("id1", "s3cret").unwrap();
        let mut terminal = FakeTerminal::default();
        let ask = |prompt: &str, terminal: &mut FakeTerminal| {
            answer(prompt, "id1", "root@10.0.0.5", &store, terminal)
                .unwrap()
                .to_string()
        };

        assert_eq!(ask("root@10.0.0.5's password: ", &mut terminal), "s3cret");
        assert!(terminal.asked.is_empty());

        assert_eq!(ask("admin@bastion's password: ", &mut terminal), "typed");
        assert_eq!(
            ask(
                "Are you sure you want to continue connecting (yes/no)? ",
                &mut terminal
            ),
            "no"
        );
        assert_eq!(terminal.asked.len(), 2);
        assert!(terminal.asked[0].0, "other passwords are typed hidden");
        assert!(!terminal.asked[1].0, "host key answers are visible");
    }

    #[test]
    fn missing_or_unavailable_password_falls_back_to_typing() {
        let empty = MemoryStore::default();
        let broken = MemoryStore {
            unavailable: true,
            ..MemoryStore::default()
        };
        for store in [&empty, &broken] {
            let mut terminal = FakeTerminal::default();
            let reply =
                answer("root@h's password: ", "id1", "root@h", store, &mut terminal).unwrap();
            assert_eq!(reply.as_str(), "typed");
        }
    }
}
