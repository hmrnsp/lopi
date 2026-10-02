//! Profile lookup. A name must be typed in full and in the same letter case, so a short or
//! mistyped name can never reach the wrong server. Near misses (another letter case, a
//! prefix, a typo) are suggested, never used.

use crate::config::{Config, Profile};
use crate::error::{NearMiss, ResolveError};

pub type Found<'a> = (&'a str, &'a Profile);

/// The profile called exactly `query`, for every command that takes a profile name.
pub fn resolve<'a>(config: &'a Config, query: &str) -> Result<Found<'a>, ResolveError> {
    if let Some((name, profile)) = config.profiles.get_key_value(query) {
        return Ok((name, profile));
    }
    let (near_miss, suggestions) = near_misses(config, query);
    Err(ResolveError::NotFound {
        query: query.into(),
        suggestions,
        near_miss,
    })
}

/// What to suggest for a name that is not a profile, closest kind first: the same name in
/// another letter case, then names it is a prefix of, then likely typos.
fn near_misses(config: &Config, query: &str) -> (NearMiss, Vec<String>) {
    if query.is_empty() {
        return (NearMiss::None, Vec::new());
    }
    let case = matching(config, |name| name.eq_ignore_ascii_case(query));
    if !case.is_empty() {
        return (NearMiss::Case, case);
    }
    let prefixes = matching(config, |name| starts_with_ignore_case(name, query));
    if !prefixes.is_empty() {
        return (NearMiss::Prefix, prefixes);
    }
    let typos = typo_suggestions(config, query);
    if !typos.is_empty() {
        return (NearMiss::Typo, typos);
    }
    (NearMiss::None, Vec::new())
}

fn matching(config: &Config, accept: impl Fn(&str) -> bool) -> Vec<String> {
    config
        .profiles
        .keys()
        .filter(|name| accept(name))
        .cloned()
        .collect()
}

fn starts_with_ignore_case(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len()
        && name.is_char_boundary(prefix.len())
        && name[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Up to three names that look like a typo of `query`, closest first. A name qualifies when
/// it is at most one edit away per three characters (at least one): `kantro` → `kantor`,
/// `vsp` → `vps`. Transpositions count as one edit.
fn typo_suggestions(config: &Config, query: &str) -> Vec<String> {
    let query = query.to_ascii_lowercase();
    let allowed = (query.chars().count() / 3).max(1);
    let mut scored: Vec<(usize, &str)> = config
        .profiles
        .keys()
        .map(|name| {
            let distance = strsim::damerau_levenshtein(&query, &name.to_ascii_lowercase());
            (distance, name.as_str())
        })
        .filter(|(distance, _)| *distance <= allowed)
        .collect();
    scored.sort();
    scored
        .into_iter()
        .take(3)
        .map(|(_, name)| name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(names: &[&str]) -> Config {
        let mut config = Config::default();
        for name in names {
            config
                .profiles
                .insert(name.to_string(), Profile::new(format!("{name}.example")));
        }
        config
    }

    fn not_found(query: &str, near_miss: NearMiss, suggestions: &[&str]) -> ResolveError {
        ResolveError::NotFound {
            query: query.into(),
            suggestions: suggestions.iter().map(|s| s.to_string()).collect(),
            near_miss,
        }
    }

    #[test]
    fn only_the_exact_name_resolves() {
        let cfg = config(&["kantor", "kantin", "vps", "vps2", "Web"]);
        let found = |q: &str| resolve(&cfg, q).map(|(name, _)| name.to_string());

        for name in ["kantor", "kantin", "vps", "vps2", "Web"] {
            assert_eq!(found(name), Ok(name.to_string()), "query: {name:?}");
        }
        let misses: &[(&str, NearMiss, &[&str])] = &[
            ("KANTOR", NearMiss::Case, &["kantor"]),
            ("web", NearMiss::Case, &["Web"]),
            ("VPS", NearMiss::Case, &["vps"]),
            ("kanto", NearMiss::Prefix, &["kantor"]),
            ("Kan", NearMiss::Prefix, &["kantin", "kantor"]),
            ("kantro", NearMiss::Typo, &["kantor", "kantin"]),
            ("vsp", NearMiss::Typo, &["vps"]),
            ("db", NearMiss::None, &[]),
            ("", NearMiss::None, &[]),
        ];
        for (query, near_miss, suggestions) in misses {
            assert_eq!(
                found(query),
                Err(not_found(query, *near_miss, suggestions)),
                "query: {query:?}"
            );
        }
    }

    #[test]
    fn hand_edited_case_duplicates_are_both_reachable() {
        let cfg = config(&["Kantor", "kantor"]);
        assert_eq!(resolve(&cfg, "kantor").unwrap().0, "kantor");
        assert_eq!(resolve(&cfg, "Kantor").unwrap().0, "Kantor");
        assert_eq!(
            resolve(&cfg, "KANTOR"),
            Err(not_found("KANTOR", NearMiss::Case, &["Kantor", "kantor"]))
        );
    }

    #[test]
    fn messages() {
        let cases = [
            (
                not_found("db", NearMiss::None, &[]),
                "no profile named 'db' (see `lopi list`)",
            ),
            (
                not_found("KANTOR", NearMiss::Case, &["kantor"]),
                "no profile named 'KANTOR'; names are case-sensitive: did you mean 'kantor'?",
            ),
            (
                not_found("kan", NearMiss::Prefix, &["kantin", "kantor"]),
                "no profile named 'kan'; type the full name: did you mean one of: kantin, kantor?",
            ),
            (
                not_found("kantro", NearMiss::Typo, &["kantor"]),
                "no profile named 'kantro'; did you mean 'kantor'?",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn empty_config() {
        let cfg = config(&[]);
        assert_eq!(resolve(&cfg, "x"), Err(not_found("x", NearMiss::None, &[])));
    }
}
