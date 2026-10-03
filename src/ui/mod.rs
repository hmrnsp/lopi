pub mod picker;
pub mod prompt;
pub mod reveal;
pub mod screen;
pub mod table;
pub mod table_picker;
pub mod tty;
pub mod wizard;

use crate::config::{Config, Profile};
use crate::ssh::args::{bracket_ipv6, destination};
use crate::state::State;
use crate::time;

/// `user@host:port via jump`, as shown in `list` and the picker.
pub fn target(profile: &Profile) -> String {
    let mut target = destination(profile);
    if let Some(port) = profile.port {
        // `user@[2001:db8::1]:22`, not the ambiguous `user@2001:db8::1:22`
        target = match &profile.user {
            Some(user) => format!("{user}@{}:{port}", bracket_ipv6(&profile.host)),
            None => format!("{}:{port}", bracket_ipv6(&profile.host)),
        };
    }
    if let Some(jump) = &profile.jump {
        target.push_str(&format!(" via {jump}"));
    }
    target
}

/// Whether [`profile_table`] has a KEY column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyColumn {
    Show,
    Hide,
}

/// The profile table of `list` and the picker: a header, then one row per name in the
/// given order. The LAST USED column only appears once there is something to show.
pub fn profile_table(
    config: &Config,
    history: &State,
    names: &[&str],
    now: u64,
    key: KeyColumn,
) -> (Vec<&'static str>, Vec<Vec<String>>) {
    let show_key = key == KeyColumn::Show;
    let show_last_used = config
        .profiles
        .iter()
        .any(|(name, profile)| history.last_used(name, profile).is_some());

    let mut header = vec!["NAME", "TARGET"];
    if show_key {
        header.push("KEY");
    }
    header.push("AUTH");
    if show_last_used {
        header.push("LAST USED");
    }
    header.push("NOTE");

    let rows = names
        .iter()
        .map(|&name| {
            let profile = &config.profiles[name];
            let mut row = vec![name.to_string(), target(profile)];
            if show_key {
                row.push(profile.key.clone().unwrap_or_default());
            }
            row.push(profile.auth_label().to_string());
            if show_last_used {
                let last_used = history.last_used(name, profile);
                row.push(
                    last_used
                        .map(|then| time::ago(now, then))
                        .unwrap_or_default(),
                );
            }
            row.push(profile.note.clone().unwrap_or_default());
            row
        })
        .collect();
    (header, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut config = Config::default();
        let kantor = Profile {
            id: Some("a".into()),
            user: Some("root".into()),
            port: Some(2222),
            key: Some("~/.ssh/id_kantor".into()),
            note: Some("office".into()),
            ..Profile::new("10.0.0.5")
        };
        let vps = Profile {
            id: Some("b".into()),
            ..Profile::new("vps.example")
        };
        config.profiles.insert("kantor".into(), kantor);
        config.profiles.insert("vps".into(), vps);
        config
    }

    #[test]
    fn ipv6_targets_with_a_port_are_bracketed() {
        let v6 = |user: Option<&str>, port| Profile {
            user: user.map(str::to_string),
            port,
            ..Profile::new("2001:db8::1")
        };
        assert_eq!(target(&v6(Some("root"), Some(22))), "root@[2001:db8::1]:22");
        assert_eq!(target(&v6(None, Some(22))), "[2001:db8::1]:22");
        assert_eq!(target(&v6(Some("root"), None)), "root@2001:db8::1");
    }

    #[test]
    fn table_with_key_and_without_history() {
        let (header, rows) = profile_table(
            &config(),
            &State::default(),
            &["vps", "kantor"],
            1000,
            KeyColumn::Show,
        );
        assert_eq!(header, ["NAME", "TARGET", "KEY", "AUTH", "NOTE"]);
        assert_eq!(
            rows,
            [
                vec!["vps", "vps.example", "", "agent", ""],
                vec![
                    "kantor",
                    "root@10.0.0.5:2222",
                    "~/.ssh/id_kantor",
                    "key",
                    "office"
                ],
            ]
        );
    }

    #[test]
    fn table_without_key_shows_last_used_once_there_is_history() {
        let mut history = State::default();
        history.last_used.insert("a".into(), 1000 - 120);
        let (header, rows) = profile_table(
            &config(),
            &history,
            &["kantor", "vps"],
            1000,
            KeyColumn::Hide,
        );
        assert_eq!(header, ["NAME", "TARGET", "AUTH", "LAST USED", "NOTE"]);
        assert_eq!(
            rows,
            [
                vec!["kantor", "root@10.0.0.5:2222", "key", "2m ago", "office"],
                vec!["vps", "vps.example", "agent", "", ""],
            ]
        );
    }
}
