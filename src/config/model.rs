use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Names that would clash with subcommands (current or planned).
pub const RESERVED_NAMES: &[&str] = &[
    "list",
    "add",
    "rm",
    "edit",
    "path",
    "help",
    "version",
    "completion",
    "connect",
    "doctor",
    "ping",
    "import",
    "export",
    "backup",
    "restore",
    "undo",
    "config",
    "passwd",
    "setup-key",
    "sync",
    "install",
    "uninstall",
    "__complete",
];

/// The profiles file format version this build reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Missing in files written by v0.1, which are version 1.
    #[serde(default = "current_schema")]
    pub schema_version: u32,
    /// ssh executable to use instead of `ssh` from PATH (`LOPI_SSH_BIN` wins over it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_bin: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

fn current_schema() -> u32 {
    SCHEMA_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ssh_bin: None,
            profiles: BTreeMap::new(),
        }
    }
}

/// The login method recorded in a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    Key,
    Password,
    Agent,
    /// A value from a newer lopi. Kept in the file as written (it is never rewritten
    /// unless changed) and treated like `agent`.
    #[serde(other)]
    Other,
}

impl Auth {
    /// The text written to the file; `None` for [`Auth::Other`], which is never written.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Key => Some("key"),
            Self::Password => Some("password"),
            Self::Agent => Some("agent"),
            Self::Other => None,
        }
    }
}

/// Field order here is the order in the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Stored as `~/...` when inside home; expanded only when connecting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// ProxyJump: comma-separated `[user@]host[:port]` items or profile names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<String>,
    /// Saved port forwards such as `L:8080:localhost:80`, `R:...`, `D:1080`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forward: Vec<String>,
    /// How to log in. Only `password` changes what lopi does (it fills in the password
    /// saved in the OS keyring); otherwise ssh decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Stable identity (survives renames). Set only by `store::mutate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// RFC 3339 UTC time of the last change. Set only by `store::mutate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl Config {
    /// Checks every profile. Reserved names are allowed here (they only warn on load),
    /// so a hand-edited file with such a name does not block other changes.
    pub fn validate(&self) -> Result<()> {
        if self
            .ssh_bin
            .as_deref()
            .is_some_and(|bin| bin.trim().is_empty())
        {
            bail!("ssh_bin is empty");
        }
        for (name, profile) in &self.profiles {
            check_name_syntax(name).with_context(|| format!("profile '{name}'"))?;
            profile
                .validate()
                .with_context(|| format!("profile '{name}'"))?;
        }
        Ok(())
    }

    pub fn reserved_names(&self) -> impl Iterator<Item = &str> {
        self.profiles
            .keys()
            .map(String::as_str)
            .filter(|name| is_reserved(name))
    }

    /// Pairs of names that differ only in letter case (possible in hand-edited files).
    /// Each is reachable by its exact name, but they are easy to confuse, so lopi warns.
    pub fn case_duplicates(&self) -> Vec<(&str, &str)> {
        let names: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
        let mut pairs = Vec::new();
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                if a.eq_ignore_ascii_case(b) {
                    pairs.push((*a, *b));
                }
            }
        }
        pairs
    }

    /// Names are case-sensitive, but two names that differ only in letter case are too easy
    /// to confuse, so a new name must not equal another profile's name ignoring case.
    /// `except` is the profile being renamed (it may change its own case).
    pub fn check_unique(&self, name: &str, except: Option<&str>) -> Result<()> {
        let clash = self.profiles.keys().find(|existing| {
            Some(existing.as_str()) != except && existing.eq_ignore_ascii_case(name)
        });
        match clash {
            Some(existing) if existing == name => {
                bail!("profile '{name}' already exists; change it with `lopi edit {name} ...`")
            }
            Some(existing) => bail!(
                "profile '{existing}' already exists; names that differ only in letter case are not allowed"
            ),
            None => Ok(()),
        }
    }
}

impl Profile {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            user: None,
            port: None,
            key: None,
            jump: None,
            forward: Vec::new(),
            auth: None,
            note: None,
            id: None,
            updated_at: None,
        }
    }

    /// What `list` shows: `password` when set, `key` when a key file is given, else
    /// `agent` (ssh uses its agent or asks).
    pub fn auth_label(&self) -> &'static str {
        match self.auth {
            Some(Auth::Password) => "password",
            Some(Auth::Key) => "key",
            _ if self.key.is_some() => "key",
            _ => "agent",
        }
    }

    pub fn uses_password(&self) -> bool {
        self.auth == Some(Auth::Password)
    }

    /// Equal apart from `id` and `updated_at`.
    pub fn same_content(&self, other: &Profile) -> bool {
        let strip = |p: &Profile| Profile {
            id: None,
            updated_at: None,
            ..p.clone()
        };
        strip(self) == strip(other)
    }

    pub fn validate(&self) -> Result<()> {
        validate_host(&self.host)?;
        if let Some(user) = &self.user {
            validate_user(user)?;
        }
        if self.port == Some(0) {
            bail!("port must be between 1 and 65535");
        }
        if self.key.as_deref().is_some_and(|key| key.trim().is_empty()) {
            bail!("key path is empty");
        }
        if let Some(jump) = &self.jump {
            validate_jump(jump)?;
        }
        for forward in &self.forward {
            validate_forward(forward)?;
        }
        if self.id.as_deref().is_some_and(|id| id.trim().is_empty()) {
            bail!("id is empty");
        }
        Ok(())
    }
}

/// Splits `[user@]host` at the first `@`. Odd input (`@h`, `u@`, `a@b@c`) is left for
/// validation to reject.
pub fn split_target(target: &str) -> (Option<&str>, &str) {
    match target.split_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, target),
    }
}

/// `jump` is a comma-separated list; each item is `[user@]host[:port]` or a profile name.
pub fn validate_jump(jump: &str) -> Result<()> {
    if jump.is_empty() {
        bail!("jump is empty");
    }
    for item in jump.split(',') {
        let (user, host) = match item.split_once('@') {
            Some((user, host)) => (Some(user), host),
            None => (None, item),
        };
        if let Some(user) = user {
            validate_user(user).with_context(|| format!("jump '{item}'"))?;
        }
        validate_host(host).with_context(|| format!("jump '{item}'"))?;
    }
    Ok(())
}

/// A saved forward: `L:<spec>`, `R:<spec>` or `D:<spec>`, passed to ssh as `-L <spec>` etc.
pub fn validate_forward(forward: &str) -> Result<()> {
    let spec = ["L:", "R:", "D:"]
        .iter()
        .find_map(|kind| forward.strip_prefix(kind))
        .with_context(|| {
            format!("forward '{forward}' must start with L:, R: or D: (e.g. L:8080:localhost:80)")
        })?;
    if spec.is_empty() || spec.starts_with('-') {
        bail!("forward '{forward}' needs a spec after the kind, e.g. L:8080:localhost:80");
    }
    if spec.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("forward '{forward}' must not contain spaces");
    }
    Ok(())
}

/// Case-insensitive, because profile lookup is case-insensitive too.
pub fn is_reserved(name: &str) -> bool {
    RESERVED_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(name))
}

/// Full check for a new name (`add`, `edit --rename`).
pub fn validate_name(name: &str) -> Result<()> {
    check_name_syntax(name)?;
    if is_reserved(name) {
        bail!("'{name}' is reserved because it clashes with a subcommand; choose another name");
    }
    Ok(())
}

fn check_name_syntax(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("name is empty");
    }
    if name.starts_with('-') {
        bail!("name '{name}' must not start with '-'");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
    {
        bail!("name '{name}' contains '{bad}'; use only letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

pub fn validate_host(host: &str) -> Result<()> {
    check_ssh_word("host", host)
}

pub fn validate_user(user: &str) -> Result<()> {
    check_ssh_word("user", user)
}

fn check_ssh_word(what: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{what} is empty");
    }
    if value.starts_with('-') {
        bail!("{what} '{value}' must not start with '-'");
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("{what} '{value}' must not contain spaces or control characters");
    }
    if value.contains('@') {
        bail!("{what} '{value}' must not contain '@'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(host: &str) -> Profile {
        Profile::new(host)
    }

    #[test]
    fn names() {
        for ok in ["kantor", "vps-1", "a.b_c", "X9"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-x",
            "my server",
            "a/b",
            "kantör",
            "list",
            "List",
            "setup-key",
            "__complete",
        ] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hosts_and_users() {
        for ok in ["103.1.2.3", "example.com", "::1", "my_host"] {
            assert!(validate_host(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-oProxyCommand=x", "a b", "a@b", "a\tb"] {
            assert!(validate_host(bad).is_err(), "{bad:?}");
        }
        assert!(validate_user("root").is_ok());
        assert!(validate_user("-l").is_err());
    }

    #[test]
    fn profile_validation() {
        assert!(profile("h").validate().is_ok());
        assert!(
            Profile {
                port: Some(0),
                ..profile("h")
            }
            .validate()
            .is_err()
        );
        assert!(
            Profile {
                key: Some(" ".into()),
                ..profile("h")
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn jump_and_forward() {
        for ok in ["bastion", "root@10.0.0.1:2222", "a,b@c:22", "::1"] {
            assert!(validate_jump(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a,,b", "-oProxyCommand=x", "a b", "@h", "u@", "a@b@c"] {
            assert!(validate_jump(bad).is_err(), "{bad:?}");
        }
        for ok in ["L:8080:localhost:80", "R:9000:localhost:9000", "D:1080"] {
            assert!(validate_forward(ok).is_ok(), "{ok}");
        }
        for bad in [
            "8080:localhost:80",
            "X:1",
            "L:",
            "L:-x",
            "L:80 :h:80",
            "l:1",
        ] {
            assert!(validate_forward(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn auth_labels() {
        let with = |auth, key: Option<&str>| Profile {
            auth,
            key: key.map(str::to_string),
            ..profile("h")
        };
        assert_eq!(with(None, None).auth_label(), "agent");
        assert_eq!(with(None, Some("~/k")).auth_label(), "key");
        assert_eq!(
            with(Some(Auth::Password), Some("~/k")).auth_label(),
            "password"
        );
        assert_eq!(with(Some(Auth::Agent), None).auth_label(), "agent");
        assert_eq!(with(Some(Auth::Other), None).auth_label(), "agent");
        assert!(with(Some(Auth::Password), None).uses_password());
        assert!(!with(Some(Auth::Other), None).uses_password());
    }

    #[test]
    fn same_content_ignores_meta() {
        let a = Profile {
            id: Some("a".into()),
            updated_at: Some("2026-01-01T00:00:00Z".into()),
            ..profile("h")
        };
        assert!(a.same_content(&profile("h")));
        assert!(!a.same_content(&profile("h2")));
    }

    #[test]
    fn schema_version_defaults_to_current() {
        assert_eq!(Config::default().schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn unique_ignoring_case() {
        let mut cfg = Config::default();
        cfg.profiles.insert("kantor".into(), profile("h"));
        cfg.profiles.insert("vps".into(), profile("h"));
        assert!(cfg.check_unique("db", None).is_ok());
        let err = cfg.check_unique("kantor", None).unwrap_err().to_string();
        assert!(err.contains("already exists; change it"), "{err}");
        let err = cfg.check_unique("KANTOR", None).unwrap_err().to_string();
        assert!(
            err.contains("'kantor' already exists; names that differ only in letter case"),
            "{err}"
        );
        // renaming a profile to a different case of its own name is fine
        assert!(cfg.check_unique("Kantor", Some("kantor")).is_ok());
        assert!(cfg.check_unique("VPS", Some("kantor")).is_err());
    }

    #[test]
    fn case_duplicates() {
        let mut cfg = Config::default();
        for name in ["Kantor", "kantor", "vps"] {
            cfg.profiles.insert(name.into(), profile("h"));
        }
        assert_eq!(cfg.case_duplicates(), [("Kantor", "kantor")]);
    }

    #[test]
    fn reserved_name_in_config_only_warns() {
        let mut cfg = Config::default();
        cfg.profiles.insert("list".into(), profile("h"));
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.reserved_names().collect::<Vec<_>>(), ["list"]);
    }
}
