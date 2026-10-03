//! `jump` items may name another profile; those are translated to `[user@]host[:port]`
//! for `ssh -J`. Translation is one level deep: a jump profile's own `jump` is not followed.

use crate::config::{Config, Profile};
use crate::resolve::resolve;
use crate::ssh::args::destination;

/// The profile an item refers to, if it is a bare name (no `@` or `:`) of an existing
/// profile, in the same letter case. Such names win over a host of the same name.
fn profile_for<'a>(config: &'a Config, item: &str) -> Option<(&'a str, &'a Profile)> {
    if !is_bare_name(item) {
        return None;
    }
    resolve(config, item).ok()
}

fn is_bare_name(item: &str) -> bool {
    !item.contains(['@', ':'])
}

/// Bare items that are not a profile but equal one when letter case is ignored, as
/// (item, profile name). They are used as host names, which is probably not what was
/// meant, so callers warn.
pub fn jump_case_mismatches<'j, 'c>(config: &'c Config, jump: &'j str) -> Vec<(&'j str, &'c str)> {
    jump.split(',')
        .filter(|item| is_bare_name(item) && !config.profiles.contains_key(*item))
        .filter_map(|item| {
            config
                .profiles
                .keys()
                .find(|name| name.eq_ignore_ascii_case(item))
                .map(|name| (item, name.as_str()))
        })
        .collect()
}

/// The warning for one [`jump_case_mismatches`] entry.
pub fn case_mismatch_warning(item: &str, profile: &str) -> String {
    format!("'{item}' is not a profile (did you mean '{profile}'?); using it as a host name")
}

/// `jump` with profile names replaced by their address, ready for `ssh -J`.
pub fn resolve_jump(config: &Config, jump: &str) -> String {
    jump.split(',')
        .map(|item| match profile_for(config, item) {
            Some((_, profile)) => match profile.port {
                Some(port) => format!("{}:{port}", destination(profile)),
                None => destination(profile),
            },
            None => item.to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Other profiles whose `jump` names `name` as a bare item, so they connect through it.
pub fn jump_dependents<'a>(config: &'a Config, name: &str) -> Vec<&'a str> {
    config
        .profiles
        .iter()
        .filter(|(other, _)| other.as_str() != name)
        .filter(|(_, profile)| {
            profile
                .jump
                .as_deref()
                .is_some_and(|jump| jump.split(',').any(|item| item == name))
        })
        .map(|(other, _)| other.as_str())
        .collect()
}

/// `jump` with every bare item that is exactly `old` replaced by `new`; `None` when there
/// is none. Items with a user or port (`root@old`, `old:22`) are host names, not profiles,
/// and stay as they are.
pub fn rename_in_jump(jump: &str, old: &str, new: &str) -> Option<String> {
    if !jump.split(',').any(|item| item == old) {
        return None;
    }
    let items: Vec<&str> = jump
        .split(',')
        .map(|item| if item == old { new } else { item })
        .collect();
    Some(items.join(","))
}

/// Profiles used as jump hosts whose key ssh will not use for the jump (ssh applies `-i`
/// to the destination only).
pub fn jump_profiles_with_keys<'a>(config: &'a Config, jump: &str) -> Vec<&'a str> {
    jump.split(',')
        .filter_map(|item| profile_for(config, item))
        .filter(|(_, profile)| profile.key.is_some())
        .map(|(name, _)| name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut config = Config::default();
        config.profiles.insert(
            "bastion".into(),
            Profile {
                user: Some("admin".into()),
                port: Some(2200),
                key: Some("~/.ssh/id_bastion".into()),
                ..Profile::new("b.example")
            },
        );
        config
            .profiles
            .insert("gw".into(), Profile::new("gw.example"));
        config
    }

    #[test]
    fn translates_profile_names_only() {
        let cfg = config();
        let cases = [
            ("bastion", "admin@b.example:2200"),
            // another letter case is not the profile: it stays a host name
            ("BASTION", "BASTION"),
            ("gw,bastion", "gw.example,admin@b.example:2200"),
            ("10.0.0.1", "10.0.0.1"),
            ("root@bastion", "root@bastion"),
            ("bastion:22", "bastion:22"),
            ("bast", "bast"),
        ];
        for (jump, expected) in cases {
            assert_eq!(resolve_jump(&cfg, jump), expected, "{jump}");
        }
    }

    #[test]
    fn finds_names_in_another_letter_case() {
        let cfg = config();
        assert_eq!(
            jump_case_mismatches(&cfg, "gw,BASTION"),
            [("BASTION", "bastion")]
        );
        assert_eq!(jump_case_mismatches(&cfg, "Gw"), [("Gw", "gw")]);
        for jump in [
            "bastion",
            "gw,bastion",
            "root@BASTION",
            "BASTION:22",
            "bast",
            "",
        ] {
            assert!(jump_case_mismatches(&cfg, jump).is_empty(), "{jump}");
        }
        assert_eq!(
            case_mismatch_warning("Bastion", "bastion"),
            "'Bastion' is not a profile (did you mean 'bastion'?); using it as a host name"
        );
    }

    #[test]
    fn renames_bare_items_only() {
        let cases = [
            ("zayd", Some("bastion")),
            ("gw,zayd", Some("gw,bastion")),
            ("zayd,zayd", Some("bastion,bastion")),
            ("root@zayd", None),
            ("zayd:22", None),
            ("Zayd", None),
            ("zayd2", None),
            ("", None),
        ];
        for (jump, expected) in cases {
            assert_eq!(
                rename_in_jump(jump, "zayd", "bastion").as_deref(),
                expected,
                "{jump}"
            );
        }
    }

    #[test]
    fn finds_profiles_that_jump_through_a_name() {
        let mut cfg = config();
        for (name, jump) in [
            ("db", "bastion"),
            ("app", "gw,bastion"),
            ("web", "root@bastion"),
            ("api", "Bastion"),
        ] {
            let profile = Profile {
                jump: Some(jump.into()),
                ..Profile::new("10.0.0.9")
            };
            cfg.profiles.insert(name.into(), profile);
        }
        // a profile that jumps through itself is not a dependent of itself
        cfg.profiles.get_mut("bastion").unwrap().jump = Some("bastion".into());
        assert_eq!(jump_dependents(&cfg, "bastion"), ["app", "db"]);
        assert!(jump_dependents(&cfg, "nothing").is_empty());
    }

    #[test]
    fn finds_jump_profiles_with_keys() {
        let cfg = config();
        assert_eq!(jump_profiles_with_keys(&cfg, "gw,bastion"), ["bastion"]);
        assert!(jump_profiles_with_keys(&cfg, "gw,10.0.0.1").is_empty());
    }
}
