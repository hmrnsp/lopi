//! Guided `add` and `edit`. The wizards only collect answers: they return the same
//! `AddArgs`/`EditArgs` the command line produces, so saving goes through one tested path.
//! A password travels next to them, never inside a command-line struct.

use std::fs;
use std::path::Path;

use anyhow::Result;
use zeroize::Zeroizing;

use super::prompt::{Prompter, TextQuestion};
use crate::cli::{AddArgs, AuthArg, EditArgs, PortArg};
use crate::config::model::{Target, parse_host_input, parse_target, validate_name, validate_user};
use crate::config::{Auth, Config, Profile};
use crate::error::Abort;
use crate::ssh::args::destination;

const DEFAULT_USER: &str = "root";
const DEFAULT_PORT: u16 = 22;
/// Typed in `edit` to remove an optional value (an empty answer keeps the current one).
const CLEAR: &str = "-";

/// What the `add` wizard collected.
pub struct AddPlan {
    pub args: AddArgs,
    pub password: Option<Zeroizing<String>>,
}

/// What the `edit` wizard collected: only changed fields, plus a new password when the
/// login method was switched to password.
pub struct EditPlan {
    pub args: EditArgs,
    pub password: Option<Zeroizing<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Login {
    Key,
    Password,
    Agent,
}

const LOGIN_CHOICES: [(Login, &str); 3] = [
    (Login::Key, "key file"),
    (
        Login::Password,
        "password (saved in the system credential store)",
    ),
    (Login::Agent, "agent (ssh-agent, or ssh asks each time)"),
];

fn ask_login(prompter: &mut dyn Prompter, current: Login) -> Result<Login> {
    let options: Vec<String> = LOGIN_CHOICES
        .iter()
        .map(|(_, text)| text.to_string())
        .collect();
    let start = LOGIN_CHOICES
        .iter()
        .position(|(login, _)| *login == current)
        .unwrap_or(0);
    let index = prompter.select("Login with:", &options, start)?;
    Ok(LOGIN_CHOICES[index].0)
}

fn current_login(profile: &Profile) -> Login {
    match profile.auth_label() {
        "password" => Login::Password,
        "key" => Login::Key,
        _ => Login::Agent,
    }
}

/// The password to save for `target`, typed twice.
pub fn ask_password(prompter: &mut dyn Prompter, target: &str) -> Result<Zeroizing<String>> {
    prompter.password(&format!("Password for {target}:"), true)
}

fn ask_key(
    prompter: &mut dyn Prompter,
    current: Option<&str>,
    keys: Vec<String>,
) -> Result<String> {
    prompter.text(
        TextQuestion::new("Key file:")
            .default_value(current)
            .help("Tab completes ~/.ssh keys")
            .suggestions(keys)
            .validate(|key| {
                if key.trim().is_empty() {
                    Err("enter the key file, or press Esc and choose another login".into())
                } else {
                    Ok(())
                }
            }),
    )
}

/// Fills in everything `args` is missing. Fields given on the command line are not asked.
pub fn add(
    config: &Config,
    mut args: AddArgs,
    prompter: &mut dyn Prompter,
    keys: Vec<String>,
) -> Result<AddPlan> {
    if args.name.is_none() {
        let taken = config.clone();
        let name = prompter.text(
            TextQuestion::new("Profile name:")
                .help("what you will type after `lopi`, e.g. vps or office")
                .validate(move |name| {
                    validate_name(name)
                        .and_then(|()| taken.check_unique(name, None))
                        .map_err(|err| err.to_string())
                }),
        )?;
        args.name = Some(name);
    }

    if args.target.is_none() {
        let given_port = args.port;
        let answer = prompter.text(
            TextQuestion::new("Host:")
                .help("IP address or host name; user@host and host:port also work")
                .validate(move |target| validate_target(target, given_port)),
        )?;
        let target = parse_target(&answer)?;
        let user = match target.user {
            Some(user) => user,
            None => prompter.text(
                TextQuestion::new("User:")
                    .default_value(Some(DEFAULT_USER))
                    .validate(|user| validate_user(user).map_err(|err| err.to_string())),
            )?,
        };
        // A port in the answer counts as given, so it is not asked again below.
        args.port = args.port.or(target.port);
        args.target = Some(
            Target {
                user: Some(user),
                ..target
            }
            .destination(),
        );
    }

    if args.port.is_none() {
        let port = ask_port(prompter, &DEFAULT_PORT.to_string())?;
        args.port = (port != DEFAULT_PORT).then_some(port);
    }
    let target = args.target.clone().unwrap_or_default();
    let mut password = None;
    if args.password {
        password = Some(ask_password(prompter, &target)?);
    } else if args.key.is_none() {
        match ask_login(prompter, Login::Key)? {
            Login::Key => args.key = Some(ask_key(prompter, None, keys)?),
            Login::Password => {
                args.password = true;
                password = Some(ask_password(prompter, &target)?);
            }
            Login::Agent => args.key = Some(String::new()),
        }
    }
    if args.note.is_none() {
        let note = prompter.text(TextQuestion::new("Note:").help("optional"))?;
        args.note = Some(note);
    }

    let summary = summarize(
        args.name.as_deref().unwrap_or_default(),
        &target,
        args.port,
        args.key.as_deref(),
        args.password,
    );
    if !prompter.confirm(&format!("Save {summary}?"), true)? {
        return Err(Abort::Cancelled.into());
    }
    Ok(AddPlan { args, password })
}

/// Asks every basic field with the current value as default and returns only what the
/// user changed (so fields edited elsewhere meanwhile are not overwritten).
pub fn edit(
    config: &Config,
    name: &str,
    profile: &Profile,
    prompter: &mut dyn Prompter,
    keys: Vec<String>,
) -> Result<EditPlan> {
    let mut args = EditArgs {
        name: Some(name.to_string()),
        ..EditArgs::default()
    };
    let keep = "Enter keeps the current value";
    let keep_or_clear = "Enter keeps the current value, - removes it";

    let taken = config.clone();
    let current = name.to_string();
    let new_name = prompter.text(
        TextQuestion::new("Profile name:")
            .default_value(Some(name))
            .help(keep)
            .validate(move |new| {
                validate_name(new)
                    .and_then(|()| taken.check_unique(new, Some(&current)))
                    .map_err(|err| err.to_string())
            }),
    )?;
    args.rename = (new_name != name).then_some(new_name);

    let host = prompter.text(
        TextQuestion::new("Host:")
            .default_value(Some(&profile.host))
            .help(keep)
            .validate(|host| {
                parse_host_input(host)
                    .map(|_| ())
                    .map_err(|err| err.to_string())
            }),
    )?;
    args.host = (host != profile.host).then_some(host);

    args.user = ask_optional(
        prompter,
        "User:",
        profile.user.as_deref(),
        keep_or_clear,
        |user| validate_user(user).map_err(|err| err.to_string()),
    )?;

    let old_port = profile.port.unwrap_or(DEFAULT_PORT);
    let port = ask_port(prompter, &old_port.to_string())?;
    if port != old_port {
        // The ssh default needs no entry in the file.
        args.port = Some(if port == DEFAULT_PORT {
            PortArg::Clear
        } else {
            PortArg::Set(port)
        });
    }

    let mut password = None;
    let explicit = |auth: Auth| profile.auth.is_some_and(|current| current != auth);
    match ask_login(prompter, current_login(profile))? {
        Login::Key => {
            let key = ask_key(prompter, profile.key.as_deref(), keys)?;
            args.key = changed(profile.key.as_deref(), &key);
            if explicit(Auth::Key) {
                args.auth = Some(AuthArg::Key);
            }
        }
        Login::Password => {
            if !profile.uses_password() {
                args.auth = Some(AuthArg::Password);
                password = Some(ask_password(prompter, &destination(profile))?);
            }
        }
        Login::Agent => {
            if profile.key.is_some() {
                args.key = Some(String::new());
            }
            if explicit(Auth::Agent) {
                args.auth = Some(AuthArg::Agent);
            }
        }
    }

    args.note = ask_optional(
        prompter,
        "Note:",
        profile.note.as_deref(),
        keep_or_clear,
        |_| Ok(()),
    )?;

    if has_changes(&args) {
        let target = super::target(profile);
        if !prompter.confirm(&format!("Save changes to '{name}' ({target})?"), true)? {
            return Err(Abort::Cancelled.into());
        }
    }
    Ok(EditPlan { args, password })
}

pub fn has_changes(args: &EditArgs) -> bool {
    args.host.is_some()
        || args.user.is_some()
        || args.port.is_some()
        || args.key.is_some()
        || args.jump.is_some()
        || args.forwards.is_some()
        || args.note.is_some()
        || args.rename.is_some()
        || args.auth.is_some()
}

/// Private keys in `~/.ssh` (`id_*` without `.pub`), as `~/.ssh/<name>` suggestions.
pub fn local_keys(home: Option<&Path>) -> Vec<String> {
    let Some(entries) = home.and_then(|home| fs::read_dir(home.join(".ssh")).ok()) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("id_") && !name.ends_with(".pub"))
        .map(|name| format!("~/.ssh/{name}"))
        .collect();
    keys.sort();
    keys
}

/// A destination answer, also checked against a port given with `--port`.
fn validate_target(target: &str, given_port: Option<u16>) -> Result<(), String> {
    let mut probe = AddArgs {
        target: Some(target.to_string()),
        port: given_port,
        ..AddArgs::default()
    };
    probe.take_port_from_target().map_err(|err| err.to_string())
}

fn ask_port(prompter: &mut dyn Prompter, default: &str) -> Result<u16> {
    let answer = prompter.text(
        TextQuestion::new("Port:")
            .default_value(Some(default))
            .validate(|port| parse_port(port).map(|_| ())),
    )?;
    Ok(parse_port(&answer).expect("validated above"))
}

fn parse_port(text: &str) -> Result<u16, String> {
    match text.trim().parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err("enter a port between 1 and 65535".into()),
    }
}

/// For optional fields in `edit`: `None` = unchanged, `Some("")` = clear, else new value.
fn ask_optional(
    prompter: &mut dyn Prompter,
    message: &str,
    current: Option<&str>,
    help: &str,
    validate: impl Fn(&str) -> Result<(), String> + 'static,
) -> Result<Option<String>> {
    let answer = prompter.text(
        TextQuestion::new(message)
            .default_value(current)
            .help(help)
            .validate(move |value| {
                if value == CLEAR {
                    Ok(())
                } else {
                    validate(value)
                }
            }),
    )?;
    Ok(changed(current, &answer))
}

/// The edit to send for a text answer: `None` when it equals the current value, `""` for
/// the clear marker (or empty input with no current value), otherwise the new value.
fn changed(current: Option<&str>, answer: &str) -> Option<String> {
    let new = if answer == CLEAR { "" } else { answer };
    (Some(new) != current && !(new.is_empty() && current.is_none())).then(|| new.to_string())
}

fn summarize(
    name: &str,
    target: &str,
    port: Option<u16>,
    key: Option<&str>,
    password: bool,
) -> String {
    let mut text = format!("'{name}' ({target}");
    if let Some(port) = port {
        text.push_str(&format!(":{port}"));
    }
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        text.push_str(&format!(", key {key}"));
    }
    if password {
        text.push_str(", password");
    }
    text.push(')');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::prompt::scripted::{Answer, Answer::*, Scripted};

    fn config_with(name: &str) -> Config {
        let mut config = Config::default();
        config.profiles.insert(name.into(), Profile::new("h"));
        config
    }

    fn run_add(config: &Config, args: AddArgs, script: Vec<Answer>) -> (Result<AddPlan>, Scripted) {
        let mut p = Scripted::new(script);
        let plan = add(config, args, &mut p, vec![]);
        (plan, p)
    }

    #[test]
    fn add_asks_everything_and_uses_defaults() {
        let (plan, p) = run_add(
            &Config::default(),
            AddArgs::default(),
            vec![
                Text("vps"),
                Text("103.1.2.3"),
                Text(""), // user: default root
                Text(""), // port: default 22
                Pick("key"),
                Text("~/.ssh/id_vps"),
                Text("my box"),
                Yes,
            ],
        );
        let AddPlan { args, password } = plan.unwrap();
        assert!(p.finished());
        assert!(password.is_none());
        assert_eq!(args.name.as_deref(), Some("vps"));
        assert_eq!(args.target.as_deref(), Some("root@103.1.2.3"));
        assert_eq!(args.port, None);
        assert_eq!(args.key.as_deref(), Some("~/.ssh/id_vps"));
        assert_eq!(args.note.as_deref(), Some("my box"));
        assert_eq!(
            p.asked.last().unwrap(),
            "Save 'vps' (root@103.1.2.3, key ~/.ssh/id_vps)?"
        );
    }

    #[test]
    fn add_with_password_keeps_it_out_of_the_args() {
        let (plan, p) = run_add(
            &Config::default(),
            AddArgs::default(),
            vec![
                Text("vps"),
                Text("root@h"),
                Text(""),
                Pick("password"),
                Secret("s3cret"),
                Text(""),
                Yes,
            ],
        );
        let AddPlan { args, password } = plan.unwrap();
        assert!(args.password);
        assert_eq!(args.key, None, "no key file");
        assert_eq!(password.as_deref().map(String::as_str), Some("s3cret"));
        assert!(
            p.asked
                .contains(&"Password for root@h: (hidden, twice)".to_string())
        );
        assert_eq!(p.asked.last().unwrap(), "Save 'vps' (root@h, password)?");
        assert!(!format!("{args:?}").contains("s3cret"));
    }

    #[test]
    fn add_password_flag_skips_the_login_question() {
        let given = AddArgs {
            name: Some("vps".into()),
            target: Some("root@h".into()),
            port: Some(22),
            note: Some(String::new()),
            password: true,
            ..AddArgs::default()
        };
        let (plan, p) = run_add(&Config::default(), given, vec![Secret("pw"), Yes]);
        assert_eq!(
            plan.unwrap().password.as_deref().map(String::as_str),
            Some("pw")
        );
        assert!(!p.asked.iter().any(|q| q.starts_with("Login with")));
    }

    #[test]
    fn add_takes_the_port_from_the_host_answer() {
        let (plan, p) = run_add(
            &Config::default(),
            AddArgs::default(),
            vec![
                Text("web"),
                Text("admin@example.com:2222"),
                Pick("agent"),
                Text(""),
                Yes,
            ],
        );
        let AddPlan { args, .. } = plan.unwrap();
        assert!(p.finished());
        assert!(!p.asked.iter().any(|q| q.starts_with("User")));
        assert!(!p.asked.iter().any(|q| q.starts_with("Port")));
        assert_eq!(args.target.as_deref(), Some("admin@example.com"));
        assert_eq!(args.port, Some(2222));
    }

    #[test]
    fn add_refuses_a_host_port_that_differs_from_the_port_flag() {
        let given = AddArgs {
            port: Some(2200),
            ..AddArgs::default()
        };
        let (plan, p) = run_add(
            &Config::default(),
            given,
            vec![
                Text("web"),
                Text("example.com:2222"),
                Text("[::1]"),
                Text(""),
                Pick("agent"),
                Text(""),
                Yes,
            ],
        );
        let AddPlan { args, .. } = plan.unwrap();
        assert!(
            p.rejected[0].contains("port given twice"),
            "{:?}",
            p.rejected
        );
        assert_eq!(args.target.as_deref(), Some("root@::1"));
        assert_eq!(args.port, Some(2200));
    }

    #[test]
    fn add_skips_user_when_host_has_one_and_skips_given_flags() {
        let given = AddArgs {
            name: Some("db".into()),
            port: Some(2222),
            key: Some(String::new()),
            ..AddArgs::default()
        };
        let script = vec![Text("admin@db.example"), Text(""), Yes];
        let (plan, p) = run_add(&Config::default(), given, script);
        assert_eq!(
            p.asked,
            ["Host:", "Note:", "Save 'db' (admin@db.example:2222)?"]
        );
        assert_eq!(
            plan.unwrap().args.target.as_deref(),
            Some("admin@db.example")
        );
    }

    #[test]
    fn add_rejects_bad_answers_until_fixed() {
        let (plan, p) = run_add(
            &config_with("kantor"),
            AddArgs::default(),
            vec![
                Text("Kantor"), // clashes with "kantor" ignoring case
                Text("list"),   // reserved
                Text("office"),
                Text("-oProxyCommand=x"),
                Text("10.0.0.5"),
                Text(""),
                Text("0"),
                Text("2200"),
                Pick("key"),
                Text(""), // a key file is required for key login
                Text("~/.ssh/id"),
                Text(""),
                Yes,
            ],
        );
        let args = plan.unwrap().args;
        assert_eq!(args.name.as_deref(), Some("office"));
        assert_eq!(args.port, Some(2200));
        assert_eq!(p.rejected.len(), 5, "{:?}", p.rejected);
        assert!(p.rejected[0].contains("differ only in letter case"));
    }

    #[test]
    fn add_can_be_cancelled_anywhere() {
        for script in [
            vec![Esc],
            vec![Text("vps"), CtrlC],
            vec![
                Text("vps"),
                Text("h"),
                Text(""),
                Text(""),
                Pick("password"),
                Esc,
            ],
            vec![
                Text("vps"),
                Text("h"),
                Text(""),
                Text(""),
                Pick("agent"),
                Text(""),
                No,
            ],
        ] {
            let (plan, _) = run_add(&Config::default(), AddArgs::default(), script);
            let err = plan.err().expect("cancelled");
            assert!(err.downcast_ref::<Abort>().is_some(), "{err}");
        }
    }

    fn existing() -> Profile {
        Profile {
            user: Some("root".into()),
            port: Some(2222),
            key: Some("~/.ssh/id".into()),
            note: Some("old".into()),
            jump: Some("bastion".into()),
            ..Profile::new("10.0.0.5")
        }
    }

    fn run_edit(profile: &Profile, script: Vec<Answer>) -> (EditPlan, Scripted) {
        let mut p = Scripted::new(script);
        let plan = edit(&config_with("kantor"), "kantor", profile, &mut p, vec![]).unwrap();
        (plan, p)
    }

    /// Name, host, user and port kept.
    fn keep_basics() -> Vec<Answer> {
        vec![Text(""), Text(""), Text(""), Text("")]
    }

    #[test]
    fn edit_keeping_everything_changes_nothing() {
        let mut script = keep_basics();
        script.extend([Pick("key"), Text(""), Text("")]);
        let (plan, p) = run_edit(&existing(), script);
        assert!(!has_changes(&plan.args));
        assert!(plan.password.is_none());
        assert!(p.finished(), "no confirmation when nothing changed");
    }

    #[test]
    fn edit_returns_only_changes() {
        let script = vec![
            Text("office"),
            Text(""),
            Text(CLEAR), // user removed
            Text("22"),
            Pick("key"),
            Text(""),
            Text("new note"),
            Yes,
        ];
        let args = run_edit(&existing(), script).0.args;
        assert_eq!(args.rename.as_deref(), Some("office"));
        assert_eq!(args.host, None);
        assert_eq!(args.user.as_deref(), Some(""));
        assert_eq!(args.port, Some(PortArg::Clear), "22 is ssh's default");
        assert_eq!(args.key, None);
        assert_eq!(args.auth, None);
        assert_eq!(args.note.as_deref(), Some("new note"));
        assert_eq!(
            args.jump, None,
            "fields the wizard does not ask stay untouched"
        );
    }

    #[test]
    fn edit_switching_to_password_asks_for_it() {
        let mut script = keep_basics();
        script.extend([Pick("password"), Secret("pw"), Text(""), Yes]);
        let (plan, p) = run_edit(&existing(), script);
        assert_eq!(plan.args.auth, Some(AuthArg::Password));
        assert_eq!(plan.args.key, None, "the key file is kept");
        assert_eq!(plan.password.as_deref().map(String::as_str), Some("pw"));
        assert!(
            p.asked
                .contains(&"Password for root@10.0.0.5: (hidden, twice)".to_string())
        );
    }

    #[test]
    fn edit_from_password_to_agent_or_key() {
        let with_password = Profile {
            auth: Some(Auth::Password),
            ..existing()
        };

        let mut script = keep_basics();
        script.extend([Pick("agent"), Text(""), Yes]);
        let plan = run_edit(&with_password, script).0;
        assert_eq!(plan.args.auth, Some(AuthArg::Agent));
        assert_eq!(
            plan.args.key.as_deref(),
            Some(""),
            "agent drops the key file"
        );

        let mut script = keep_basics();
        script.extend([Pick("key"), Text(""), Text(""), Yes]);
        let plan = run_edit(&with_password, script).0;
        assert_eq!(plan.args.auth, Some(AuthArg::Key));
        assert!(plan.password.is_none());

        // staying on password does not ask again (`lopi passwd` changes it)
        let mut script = keep_basics();
        script.extend([Pick("password"), Text("")]);
        let (plan, p) = run_edit(&with_password, script);
        assert!(!has_changes(&plan.args));
        assert!(p.finished());
    }

    #[test]
    fn changed_values() {
        assert_eq!(changed(Some("a"), "a"), None);
        assert_eq!(changed(Some("a"), "b"), Some("b".into()));
        assert_eq!(changed(Some("a"), CLEAR), Some(String::new()));
        assert_eq!(changed(None, ""), None);
        assert_eq!(changed(None, CLEAR), None);
        assert_eq!(changed(None, "x"), Some("x".into()));
    }

    #[test]
    fn finds_private_keys_only() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        fs::create_dir_all(&ssh).unwrap();
        for file in [
            "id_ed25519",
            "id_ed25519.pub",
            "id_rsa",
            "known_hosts",
            "config",
        ] {
            fs::write(ssh.join(file), "").unwrap();
        }
        assert_eq!(
            local_keys(Some(home.path())),
            ["~/.ssh/id_ed25519", "~/.ssh/id_rsa"]
        );
        assert!(local_keys(None).is_empty());
    }
}
