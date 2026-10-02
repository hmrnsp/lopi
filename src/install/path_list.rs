//! Editing a Windows `PATH` value (`;`-separated). Only the entry for lopi's folder is
//! added or removed; every other entry is kept exactly as written, including unexpanded
//! `%VARIABLES%`.

/// `path` with `dir` appended, or `None` if `dir` is already in it. Empty entries
/// (`;;`, a trailing `;`) are dropped when the value is rewritten.
pub fn with_dir(path: &str, dir: &str) -> Option<String> {
    if entries(path).any(|entry| same_dir(entry, dir)) {
        return None;
    }
    let mut kept: Vec<&str> = entries(path).collect();
    kept.push(dir);
    Some(kept.join(";"))
}

/// `path` without any entry for `dir`, or `None` if it had none.
pub fn without_dir(path: &str, dir: &str) -> Option<String> {
    if !entries(path).any(|entry| same_dir(entry, dir)) {
        return None;
    }
    let kept: Vec<&str> = entries(path)
        .filter(|entry| !same_dir(entry, dir))
        .collect();
    Some(kept.join(";"))
}

fn entries(path: &str) -> impl Iterator<Item = &str> {
    path.split(';').filter(|entry| !entry.trim().is_empty())
}

/// Windows paths ignore case, and `C:\x` and `C:\x\` are the same folder.
fn same_dir(a: &str, b: &str) -> bool {
    let clean = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_string();
    clean(a).eq_ignore_ascii_case(&clean(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Users\Budi\AppData\Local\Programs\lopi";

    #[test]
    fn adding() {
        let cases: &[(&str, Option<String>)] = &[
            ("", Some(DIR.to_string())),
            (r"C:\a", Some(format!(r"C:\a;{DIR}"))),
            (r"C:\a;", Some(format!(r"C:\a;{DIR}"))),
            (
                r"C:\a;;%USERPROFILE%\bin",
                Some(format!(r"C:\a;%USERPROFILE%\bin;{DIR}")),
            ),
            (&format!(r"C:\a;{DIR}"), None),
            (&format!(r"C:\a;{DIR}\"), None),
            (&format!(r"C:\a;{}", DIR.to_uppercase()), None),
        ];
        for (path, expected) in cases {
            assert_eq!(with_dir(path, DIR), *expected, "{path:?}");
        }
    }

    #[test]
    fn removing() {
        let cases: &[(&str, Option<&str>)] = &[
            ("", None),
            (r"C:\a", None),
            (DIR, Some("")),
            (
                &format!(r"C:\a;{DIR};%USERPROFILE%\bin"),
                Some(r"C:\a;%USERPROFILE%\bin"),
            ),
            (
                &format!(r"{DIR}\;C:\a;{}", DIR.to_lowercase()),
                Some(r"C:\a"),
            ),
        ];
        for (path, expected) in cases {
            assert_eq!(without_dir(path, DIR).as_deref(), *expected, "{path:?}");
        }
    }

    #[test]
    fn add_then_remove_restores_the_entries() {
        let original = r"C:\a;%USERPROFILE%\bin";
        let added = with_dir(original, DIR).unwrap();
        assert_eq!(without_dir(&added, DIR).as_deref(), Some(original));
    }
}
