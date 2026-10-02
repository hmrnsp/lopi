//! Profile lookup. Names are not case-sensitive: an exact match (same case) always wins,
//! then an exact match ignoring case, then (for connecting only) a unique prefix.

use crate::config::{Config, Profile};
use crate::error::ResolveError;

pub type Found<'a> = (&'a str, &'a Profile);

/// For connecting: exact name, or a prefix that matches exactly one profile.
pub fn resolve<'a>(config: &'a Config, query: &str) -> Result<Found<'a>, ResolveError> {
    if let Some(found) = exact(config, query)? {
        return Ok(found);
    }
    let mut matches = prefix_matches(config, query);
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(ResolveError::NotFound {
            query: query.into(),
            suggestions: typo_suggestions(config, query),
            full_name_required: false,
        }),
        _ => Err(ambiguous(query, &matches)),
    }
}

/// For destructive commands (`rm`, `edit`): only the full name (in any case) is accepted.
pub fn resolve_exact<'a>(config: &'a Config, query: &str) -> Result<Found<'a>, ResolveError> {
    if let Some(found) = exact(config, query)? {
        return Ok(found);
    }
    let prefixes = names(&prefix_matches(config, query));
    let suggestions = if prefixes.is_empty() {
        typo_suggestions(config, query)
    } else {
        prefixes
    };
    Err(ResolveError::NotFound {
        query: query.into(),
        suggestions,
        full_name_required: true,
    })
}

fn exact<'a>(config: &'a Config, query: &str) -> Result<Option<Found<'a>>, ResolveError> {
    if let Some((name, profile)) = config.profiles.get_key_value(query) {
        return Ok(Some((name, profile)));
    }
    let mut matches = matching(config, |name| name.eq_ignore_ascii_case(query));
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.remove(0))),
        // Only possible in a hand-edited file (`add` rejects such names).
        _ => Err(ambiguous(query, &matches)),
    }
}

fn prefix_matches<'a>(config: &'a Config, query: &str) -> Vec<Found<'a>> {
    if query.is_empty() {
        return Vec::new();
    }
    matching(config, |name| starts_with_ignore_case(name, query))
}

fn matching<'a>(config: &'a Config, accept: impl Fn(&str) -> bool) -> Vec<Found<'a>> {
    config
        .profiles
        .iter()
        .filter(|(name, _)| accept(name))
        .map(|(name, profile)| (name.as_str(), profile))
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
    if query.is_empty() {
        return Vec::new();
    }
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

fn ambiguous(query: &str, matches: &[Found<'_>]) -> ResolveError {
    ResolveError::Ambiguous {
        query: query.into(),
        candidates: names(matches),
    }
}

fn names(found: &[Found<'_>]) -> Vec<String> {
    found.iter().map(|(name, _)| name.to_string()).collect()
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

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn not_found(query: &str, suggestions: &[&str], full: bool) -> ResolveError {
        ResolveError::NotFound {
            query: query.into(),
            suggestions: list(suggestions),
            full_name_required: full,
        }
    }

    #[test]
    fn resolve_cases() {
        let cfg = config(&["kantor", "kantin", "vps", "vps2", "Web"]);
        let found = |q: &str| resolve(&cfg, q).map(|(name, _)| name.to_string());

        let cases: &[(&str, Result<&str, ResolveError>)] = &[
            ("kantor", Ok("kantor")),
            ("kanto", Ok("kantor")),
            ("kanti", Ok("kantin")),
            // exact match wins even though "vps" is also a prefix of "vps2"
            ("vps", Ok("vps")),
            ("vps2", Ok("vps2")),
            // case-insensitive exact and prefix
            ("KANTOR", Ok("kantor")),
            ("Kanto", Ok("kantor")),
            ("web", Ok("Web")),
            ("VPS", Ok("vps")),
            (
                "kan",
                Err(ResolveError::Ambiguous {
                    query: "kan".into(),
                    candidates: list(&["kantin", "kantor"]),
                }),
            ),
            ("db", Err(not_found("db", &[], false))),
            ("", Err(not_found("", &[], false))),
        ];
        for (query, expected) in cases {
            let expected = expected.clone().map(str::to_string);
            assert_eq!(found(query), expected, "query: {query:?}");
        }
    }

    #[test]
    fn typos_get_suggestions() {
        let cfg = config(&["kantor", "kantin", "vps"]);
        assert_eq!(
            resolve(&cfg, "kantro"),
            Err(not_found("kantro", &["kantor", "kantin"], false))
        );
        assert_eq!(resolve(&cfg, "vsp"), Err(not_found("vsp", &["vps"], false)));
        assert_eq!(resolve(&cfg, "zzz"), Err(not_found("zzz", &[], false)));
    }

    #[test]
    fn resolve_exact_rejects_prefix() {
        let cfg = config(&["kantor", "kantin"]);
        assert_eq!(resolve_exact(&cfg, "kantor").unwrap().0, "kantor");
        assert_eq!(resolve_exact(&cfg, "KANTOR").unwrap().0, "kantor");
        assert_eq!(
            resolve_exact(&cfg, "kanto"),
            Err(not_found("kanto", &["kantor"], true))
        );
        assert_eq!(
            resolve_exact(&cfg, "kantro"),
            Err(not_found("kantro", &["kantor", "kantin"], true))
        );
    }

    #[test]
    fn hand_edited_case_duplicates_are_ambiguous() {
        let cfg = config(&["Kantor", "kantor"]);
        // exact case still works
        assert_eq!(resolve(&cfg, "kantor").unwrap().0, "kantor");
        assert_eq!(resolve_exact(&cfg, "Kantor").unwrap().0, "Kantor");
        assert!(matches!(
            resolve(&cfg, "KANTOR"),
            Err(ResolveError::Ambiguous { .. })
        ));
        assert!(matches!(
            resolve_exact(&cfg, "KANTOR"),
            Err(ResolveError::Ambiguous { .. })
        ));
    }

    #[test]
    fn messages() {
        assert_eq!(
            not_found("db", &[], false).to_string(),
            "no profile named 'db' (see `lopi list`)"
        );
        assert_eq!(
            not_found("kantro", &["kantor"], false).to_string(),
            "no profile named 'kantro'; did you mean 'kantor'?"
        );
        assert_eq!(
            not_found("kan", &["kantor", "kantin"], true).to_string(),
            "no profile named exactly 'kan'; this command needs the full name; did you mean one of: kantor, kantin?"
        );
    }

    #[test]
    fn empty_config() {
        let cfg = config(&[]);
        assert!(resolve(&cfg, "x").is_err());
        assert!(resolve_exact(&cfg, "x").is_err());
    }
}
