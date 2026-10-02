//! Key paths are stored portably: `~/...` with `/` when inside the home directory,
//! otherwise as an absolute path. Expansion back happens in `ssh::args::expand_tilde`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};

/// Turns what the user typed into the stored form. Pure: `cwd` and `home` are passed in.
///
/// - `~`, `~/x` (and `~\x` on Windows) stay home-relative, with `/` separators.
/// - Relative paths are made absolute against `cwd` (`.` and `..` resolved lexically,
///   without touching the file system, so a key that does not exist yet still works).
/// - Absolute paths inside `home` become `~/...` (shells such as bash expand `~` before
///   lopi sees it, so this restores the portable form).
pub fn normalize_key(raw: &str, cwd: &Path, home: Option<&Path>) -> Result<String> {
    if let Some(rest) = tilde_rest(raw) {
        return Ok(tilde_form(rest.split(is_separator)));
    }
    let absolute = lexical_clean(&cwd.join(raw));
    if let Some(home) = home
        && let Some(rest) = strip_home(&absolute, &lexical_clean(home))
    {
        return Ok(tilde_form(rest.iter().map(String::as_str)));
    }
    absolute
        .into_os_string()
        .into_string()
        .map_err(|path| anyhow!("key path {} is not valid UTF-8", path.display()))
}

fn tilde_rest(raw: &str) -> Option<&str> {
    if raw == "~" {
        return Some("");
    }
    raw.strip_prefix("~/")
        .or_else(|| raw.strip_prefix("~\\").filter(|_| cfg!(windows)))
}

fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

fn tilde_form<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let parts: Vec<&str> = parts.filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        "~".into()
    } else {
        format!("~/{}", parts.join("/"))
    }
}

/// Resolves `.` and `..` without following symlinks or requiring the path to exist.
fn lexical_clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never pop past the root/prefix.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The components of `path` below `home`, if `path` is inside it. Windows paths are
/// compared ignoring ASCII case, like the file system does.
fn strip_home(path: &Path, home: &Path) -> Option<Vec<String>> {
    let same = |a: &Component, b: &Component| {
        let (a, b) = (
            a.as_os_str().to_string_lossy(),
            b.as_os_str().to_string_lossy(),
        );
        if cfg!(windows) {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    };
    let mut path_parts = path.components();
    for home_part in home.components() {
        let path_part = path_parts.next()?;
        if !same(&path_part, &home_part) {
            return None;
        }
    }
    path_parts
        .map(|part| part.as_os_str().to_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unix_paths() {
        let home = Path::new("/home/budi");
        let cwd = Path::new("/home/budi/.ssh");
        let norm = |raw: &str| normalize_key(raw, cwd, Some(home)).unwrap();

        assert_eq!(norm("~/.ssh/id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm("~//.ssh/id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm("~"), "~");
        assert_eq!(norm("/home/budi/.ssh/id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm("id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm("./keys/../id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm("/etc/ssh/key"), "/etc/ssh/key");
        assert_eq!(norm("/home/budiman/key"), "/home/budiman/key");
        assert_eq!(norm("/../../etc/key"), "/etc/key");
        // `~user` is not expanded by lopi; it is just a relative name here
        assert_eq!(norm("~other/key"), "~/.ssh/~other/key");
        assert_eq!(
            normalize_key("/home/budi/k", cwd, None).unwrap(),
            "/home/budi/k"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths() {
        let home = Path::new(r"C:\Users\Budi Santoso");
        let cwd = Path::new(r"C:\Users\Budi Santoso\.ssh");
        let norm = |raw: &str| normalize_key(raw, cwd, Some(home)).unwrap();

        assert_eq!(norm("~/.ssh/id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm(r"~\.ssh\id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm(r"C:\Users\Budi Santoso\.ssh\id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm(r"c:\users\budi santoso\.ssh\id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm("id_vps"), "~/.ssh/id_vps");
        assert_eq!(norm(r"..\keys\id"), "~/keys/id");
        assert_eq!(norm(r"D:\keys\id"), r"D:\keys\id");
    }
}
