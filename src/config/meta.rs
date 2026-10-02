//! Bookkeeping fields (`id`, `updated_at`) that only `store::mutate` writes. Pure apart
//! from `new_id`'s randomness.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, bail};

use super::model::Config;

const ID_LEN: usize = 10;
const ID_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// 10 random characters from `[a-z0-9]` (about 51 bits), unique within `taken`.
pub fn new_id(taken: &HashSet<String>) -> String {
    loop {
        let id: String = (0..ID_LEN)
            .map(|_| ID_ALPHABET[fastrand::usize(..ID_ALPHABET.len())] as char)
            .collect();
        if !taken.contains(&id) {
            return id;
        }
    }
}

/// Gives every profile a unique id. Missing ids are added; a duplicated id (for example a
/// profile block copied by hand) is kept by the first profile in name order and replaced
/// for the others.
pub fn ensure_ids(config: &mut Config) {
    let mut taken: HashSet<String> = HashSet::new();
    let mut needs_id = Vec::new();
    for (name, profile) in &config.profiles {
        match &profile.id {
            Some(id) if taken.insert(id.clone()) => {}
            _ => needs_id.push(name.clone()),
        }
    }
    for name in needs_id {
        let id = new_id(&taken);
        taken.insert(id.clone());
        config
            .profiles
            .get_mut(&name)
            .expect("name from the map")
            .id = Some(id);
    }
}

/// Fills in metadata after a change. `before` must have gone through [`ensure_ids`].
///
/// - A profile without an id is new: it gets an id and `updated_at = now`.
/// - A profile whose content or name changed gets `updated_at = now`.
/// - An id that did not exist before, or appears twice, means the change edited ids
///   directly, which is a bug: rejected.
pub fn stamp(before: &Config, after: &mut Config, now: &str) -> Result<()> {
    let old: BTreeMap<&str, (&str, &_)> = before
        .profiles
        .iter()
        .filter_map(|(name, p)| Some((p.id.as_deref()?, (name.as_str(), p))))
        .collect();
    let mut taken: HashSet<String> = old.keys().map(|id| id.to_string()).collect();
    let mut seen: HashSet<String> = HashSet::new();

    for (name, profile) in &mut after.profiles {
        let Some(id) = profile.id.clone() else {
            let id = new_id(&taken);
            taken.insert(id.clone());
            seen.insert(id.clone());
            profile.id = Some(id);
            profile.updated_at = Some(now.to_string());
            continue;
        };
        let Some((old_name, old_profile)) = old.get(id.as_str()) else {
            bail!("internal error: profile '{name}' has an unknown id '{id}'");
        };
        if !seen.insert(id.clone()) {
            bail!("internal error: id '{id}' is used by two profiles");
        }
        if old_name != name || !profile.same_content(old_profile) {
            profile.updated_at = Some(now.to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;

    #[test]
    fn ids_have_expected_shape() {
        let id = new_id(&HashSet::new());
        assert_eq!(id.len(), ID_LEN);
        assert!(id.bytes().all(|b| ID_ALPHABET.contains(&b)), "{id}");
    }

    fn config(entries: &[(&str, Option<&str>)]) -> Config {
        let mut config = Config::default();
        for (name, id) in entries {
            let profile = Profile {
                id: id.map(str::to_string),
                ..Profile::new(format!("{name}.example"))
            };
            config.profiles.insert(name.to_string(), profile);
        }
        config
    }

    fn id_of<'a>(config: &'a Config, name: &str) -> &'a str {
        config.profiles[name].id.as_deref().unwrap()
    }

    #[test]
    fn ensure_ids_fills_missing_and_duplicates() {
        let mut cfg = config(&[("a", Some("same")), ("b", Some("same")), ("c", None)]);
        ensure_ids(&mut cfg);
        assert_eq!(id_of(&cfg, "a"), "same");
        assert_ne!(id_of(&cfg, "b"), "same");
        assert_ne!(id_of(&cfg, "c"), id_of(&cfg, "b"));
        // migrated profiles get no invented timestamp
        assert!(cfg.profiles.values().all(|p| p.updated_at.is_none()));
    }

    #[test]
    fn stamp_marks_only_new_changed_and_renamed() {
        let before = config(&[("same", Some("1")), ("edit", Some("2")), ("old", Some("3"))]);
        let mut after = before.clone();
        after.profiles.get_mut("edit").unwrap().port = Some(2222);
        let renamed = after.profiles.remove("old").unwrap();
        after.profiles.insert("new".into(), renamed);
        after
            .profiles
            .insert("added".into(), Profile::new("added.example"));

        stamp(&before, &mut after, "NOW").unwrap();
        let updated = |name: &str| after.profiles[name].updated_at.as_deref();
        assert_eq!(updated("same"), None);
        assert_eq!(updated("edit"), Some("NOW"));
        assert_eq!(updated("new"), Some("NOW"));
        assert_eq!(updated("added"), Some("NOW"));
        assert_eq!(id_of(&after, "new"), "3");
        assert!(!["1", "2", "3"].contains(&id_of(&after, "added")));
    }

    #[test]
    fn stamp_rejects_edited_ids() {
        let before = config(&[("a", Some("1"))]);
        let mut forged = before.clone();
        forged.profiles.get_mut("a").unwrap().id = Some("999".into());
        assert!(stamp(&before, &mut forged, "NOW").is_err());

        let mut copied = before.clone();
        let clone = copied.profiles["a"].clone();
        copied.profiles.insert("b".into(), clone);
        assert!(stamp(&before, &mut copied, "NOW").is_err());
    }
}
