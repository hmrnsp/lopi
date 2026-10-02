//! `jump` items may name another profile; those are translated to `[user@]host[:port]`
//! for `ssh -J`. Translation is one level deep: a jump profile's own `jump` is not followed.

use crate::config::{Config, Profile};
use crate::resolve::resolve_exact;
use crate::ssh::args::destination;

/// The profile an item refers to, if it is a bare name (no `@` or `:`) of an existing
/// profile (any letter case). Such names win over a host of the same name.
fn profile_for<'a>(config: &'a Config, item: &str) -> Option<(&'a str, &'a Profile)> {
    if item.contains(['@', ':']) {
        return None;
    }
    resolve_exact(config, item).ok()
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
            ("BASTION", "admin@b.example:2200"),
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
    fn finds_jump_profiles_with_keys() {
        let cfg = config();
        assert_eq!(jump_profiles_with_keys(&cfg, "gw,bastion"), ["bastion"]);
        assert!(jump_profiles_with_keys(&cfg, "gw,10.0.0.1").is_empty());
    }
}
