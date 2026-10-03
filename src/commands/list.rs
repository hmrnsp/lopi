use std::time::{Duration, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;

use crate::config::{Config, paths, store};
use crate::state::{self, State};
use crate::ui::{self, KeyColumn, table};
use crate::{output, time};

pub fn run(recent: bool, json: bool) -> Result<i32> {
    let config = store::load()?;
    if config.profiles.is_empty() && !json {
        eprintln!(
            "no profiles yet; add one with `lopi add <name> [user@]host` (file: {})",
            paths::config_file()?.display()
        );
        return Ok(0);
    }
    let history = state::load();
    let names: Vec<&str> = if recent {
        history.recent_first(&config)
    } else {
        config.profiles.keys().map(String::as_str).collect()
    };
    if json {
        let mut text = serde_json::to_string_pretty(&json_rows(&config, &history, &names))?;
        text.push('\n');
        output::print(&text)?;
        return Ok(0);
    }
    let (header, rows) =
        ui::profile_table(&config, &history, &names, time::unix_now(), KeyColumn::Show);
    output::print(&table::render(&header, &rows))?;
    Ok(0)
}

/// One profile in `list --json`. Every key is always present (`null` when unset), so
/// scripts can rely on the shape. Passwords are never part of it.
#[derive(Debug, Serialize)]
struct JsonProfile<'a> {
    name: &'a str,
    host: &'a str,
    user: Option<&'a str>,
    port: Option<u16>,
    /// As stored, e.g. `~/.ssh/id_vps`.
    key: Option<&'a str>,
    jump: Option<&'a str>,
    forward: &'a [String],
    /// `key`, `password` or `agent`, as in the AUTH column.
    auth: &'static str,
    note: Option<&'a str>,
    /// RFC 3339 UTC time of the last connection.
    last_used: Option<String>,
}

fn json_rows<'a>(config: &'a Config, history: &State, names: &[&'a str]) -> Vec<JsonProfile<'a>> {
    names
        .iter()
        .map(|&name| {
            let profile = &config.profiles[name];
            JsonProfile {
                name,
                host: &profile.host,
                user: profile.user.as_deref(),
                port: profile.port,
                key: profile.key.as_deref(),
                jump: profile.jump.as_deref(),
                forward: &profile.forward,
                auth: profile.auth_label(),
                note: profile.note.as_deref(),
                last_used: history
                    .last_used(name, profile)
                    .map(|secs| time::rfc3339_utc(UNIX_EPOCH + Duration::from_secs(secs))),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Auth, Profile};

    #[test]
    fn json_has_every_key_and_no_secrets() {
        let mut config = Config::default();
        config.profiles.insert(
            "vps".into(),
            Profile {
                user: Some("root".into()),
                port: Some(2222),
                forward: vec!["D:1080".into()],
                auth: Some(Auth::Password),
                note: Some("say \"hi\"".into()),
                id: Some("abc".into()),
                ..Profile::new("203.0.113.7")
            },
        );
        config.profiles.insert("bare".into(), Profile::new("h"));
        let mut history = State::default();
        history.last_used.insert("abc".into(), 1_790_000_000);

        let rows = json_rows(&config, &history, &["vps", "bare"]);
        let value = serde_json::to_value(&rows).unwrap();
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "name": "vps", "host": "203.0.113.7", "user": "root", "port": 2222,
                    "key": null, "jump": null, "forward": ["D:1080"], "auth": "password",
                    "note": "say \"hi\"", "last_used": "2026-09-21T14:13:20Z"
                },
                {
                    "name": "bare", "host": "h", "user": null, "port": null, "key": null,
                    "jump": null, "forward": [], "auth": "agent", "note": null,
                    "last_used": null
                }
            ])
        );
        // bookkeeping stays out of the public shape
        assert!(!value.to_string().contains("abc"));
    }
}
