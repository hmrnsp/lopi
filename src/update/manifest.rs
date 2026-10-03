//! `dist-manifest.json`, published by cargo-dist with every release: the version and the
//! archive to download for each target. Pure; only the fields lopi needs are read.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use super::version::Version;

/// The crate (and app) name the releases are published under.
const APP_NAME: &str = "lopi-ssh";

#[derive(Debug, Deserialize)]
pub struct Manifest {
    releases: Vec<ReleaseEntry>,
    #[serde(default)]
    artifacts: BTreeMap<String, Artifact>,
}

#[derive(Debug, Deserialize)]
struct ReleaseEntry {
    app_name: String,
    app_version: String,
    #[serde(default)]
    artifacts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    kind: String,
    #[serde(default)]
    target_triples: Vec<String>,
    /// Name of the artifact holding this one's checksum file.
    checksum: Option<String>,
    #[serde(default)]
    checksums: BTreeMap<String, String>,
}

/// The archive for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// File name in the release, e.g. `lopi-ssh-x86_64-unknown-linux-musl.tar.xz`.
    pub name: String,
    /// File name of its `.sha256` file.
    pub checksum_file: String,
    /// The SHA-256 written in the manifest itself, when there is one (lowercase hex).
    pub sha256: Option<String>,
}

impl Manifest {
    pub fn parse(json: &[u8]) -> Result<Self> {
        serde_json::from_slice(json).context("the release manifest is not valid")
    }

    fn release(&self) -> Result<&ReleaseEntry> {
        let mut matching = self.releases.iter().filter(|r| r.app_name == APP_NAME);
        match (matching.next(), matching.next()) {
            (Some(release), None) => Ok(release),
            (None, _) => bail!("the release manifest has no {APP_NAME} release"),
            (Some(_), Some(_)) => {
                bail!("the release manifest has more than one {APP_NAME} release")
            }
        }
    }

    pub fn version(&self) -> Result<Version> {
        let text = &self.release()?.app_version;
        text.parse()
            .with_context(|| format!("the release manifest has a bad version '{text}'"))
    }

    /// The archive of the release built for `target`, ending in `suffix` (`.tar.xz` or `.zip`).
    pub fn archive_for(&self, target: &str, suffix: &str) -> Result<Archive> {
        let release = self.release()?;
        let listed = |name: &str| release.artifacts.iter().any(|a| a == name);
        let mut found = self.artifacts.iter().filter(|(name, artifact)| {
            artifact.kind == "executable-zip"
                && artifact.target_triples.iter().any(|t| t == target)
                && name.ends_with(suffix)
                && listed(name)
        });
        let (name, artifact) = match (found.next(), found.next()) {
            (Some(one), None) => one,
            (None, _) => bail!("the latest release has no download for this system ({target})"),
            (Some(_), Some(_)) => bail!("the latest release has several downloads for {target}"),
        };
        check_file_name(name)?;
        let checksum_file = artifact
            .checksum
            .clone()
            .ok_or_else(|| anyhow!("the latest release has no checksum for {name}"))?;
        check_file_name(&checksum_file)?;
        if !listed(&checksum_file) {
            bail!("the latest release does not include {checksum_file}");
        }
        Ok(Archive {
            name: name.clone(),
            checksum_file,
            sha256: artifact
                .checksums
                .get("sha256")
                .map(|hex| hex.to_ascii_lowercase()),
        })
    }
}

/// Names go into download URLs: plain file names only.
fn check_file_name(name: &str) -> Result<()> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !plain {
        bail!("the release manifest has an unexpected file name '{name}'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the real v0.4.0 manifest.
    const MANIFEST: &str = r#"{
      "dist_version": "0.32.0",
      "announcement_tag": "v0.4.0",
      "releases": [{
        "app_name": "lopi-ssh",
        "app_version": "0.4.0",
        "artifacts": [
          "lopi-ssh-installer.sh",
          "lopi-ssh-x86_64-unknown-linux-musl.tar.xz",
          "lopi-ssh-x86_64-unknown-linux-musl.tar.xz.sha256",
          "lopi-ssh-x86_64-pc-windows-msvc.zip",
          "lopi-ssh-x86_64-pc-windows-msvc.zip.sha256"
        ]
      }],
      "artifacts": {
        "lopi-ssh-installer.sh": {
          "name": "lopi-ssh-installer.sh",
          "kind": "installer",
          "target_triples": ["x86_64-unknown-linux-musl-static", "x86_64-pc-windows-gnu"]
        },
        "lopi-ssh-x86_64-unknown-linux-musl.tar.xz": {
          "name": "lopi-ssh-x86_64-unknown-linux-musl.tar.xz",
          "kind": "executable-zip",
          "target_triples": ["x86_64-unknown-linux-musl"],
          "assets": [{"name": "lopi", "path": "lopi", "kind": "executable"}],
          "checksum": "lopi-ssh-x86_64-unknown-linux-musl.tar.xz.sha256",
          "checksums": {"sha256": "ABCDEF"}
        },
        "lopi-ssh-x86_64-unknown-linux-musl.tar.xz.sha256": {
          "name": "lopi-ssh-x86_64-unknown-linux-musl.tar.xz.sha256",
          "kind": "checksum",
          "target_triples": ["x86_64-unknown-linux-musl"]
        },
        "lopi-ssh-x86_64-pc-windows-msvc.zip": {
          "kind": "executable-zip",
          "target_triples": ["x86_64-pc-windows-msvc"],
          "checksum": "lopi-ssh-x86_64-pc-windows-msvc.zip.sha256"
        }
      }
    }"#;

    fn manifest(json: &str) -> Manifest {
        Manifest::parse(json.as_bytes()).unwrap()
    }

    #[test]
    fn reads_version_and_archive() {
        let m = manifest(MANIFEST);
        assert_eq!(m.version().unwrap(), Version(0, 4, 0));
        assert_eq!(
            m.archive_for("x86_64-unknown-linux-musl", ".tar.xz")
                .unwrap(),
            Archive {
                name: "lopi-ssh-x86_64-unknown-linux-musl.tar.xz".into(),
                checksum_file: "lopi-ssh-x86_64-unknown-linux-musl.tar.xz.sha256".into(),
                sha256: Some("abcdef".into()),
            }
        );
        let zip = m.archive_for("x86_64-pc-windows-msvc", ".zip").unwrap();
        assert_eq!(zip.name, "lopi-ssh-x86_64-pc-windows-msvc.zip");
        assert_eq!(zip.sha256, None);
    }

    #[test]
    fn refuses_what_does_not_fit() {
        let m = manifest(MANIFEST);
        let err =
            |target: &str, suffix: &str| m.archive_for(target, suffix).unwrap_err().to_string();
        assert!(
            err("riscv64gc-unknown-linux-gnu", ".tar.xz").contains("no download for this system")
        );
        // the installer lists this target, but it is not an archive
        assert!(err("x86_64-pc-windows-gnu", ".zip").contains("no download"));
        assert!(err("x86_64-unknown-linux-musl", ".zip").contains("no download"));
        // an archive that the release does not list is not used
        let unlisted = MANIFEST.replace("\"lopi-ssh-x86_64-pc-windows-msvc.zip\",", "");
        assert!(
            manifest(&unlisted)
                .archive_for("x86_64-pc-windows-msvc", ".zip")
                .is_err()
        );
    }

    #[test]
    fn refuses_unsafe_names_and_missing_checksums() {
        let evil = MANIFEST.replace(
            "\"checksum\": \"lopi-ssh-x86_64-unknown-linux-musl.tar.xz.sha256\"",
            "\"checksum\": \"../x.sha256\"",
        );
        let err = manifest(&evil)
            .archive_for("x86_64-unknown-linux-musl", ".tar.xz")
            .unwrap_err();
        assert!(err.to_string().contains("unexpected file name"), "{err}");

        let none = MANIFEST.replace(
            "\"checksum\": \"lopi-ssh-x86_64-pc-windows-msvc.zip.sha256\"",
            "\"checksum\": null",
        );
        let err = manifest(&none)
            .archive_for("x86_64-pc-windows-msvc", ".zip")
            .unwrap_err();
        assert!(err.to_string().contains("no checksum"), "{err}");

        for name in ["", ".hidden", "a/b", "a b", "a\\b", "ä"] {
            assert!(check_file_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn needs_exactly_one_lopi_release_with_a_plain_version() {
        let other = MANIFEST.replace("\"app_name\": \"lopi-ssh\"", "\"app_name\": \"other\"");
        assert!(manifest(&other).version().is_err());
        let bad = MANIFEST.replace(
            "\"app_version\": \"0.4.0\"",
            "\"app_version\": \"0.4.0-rc.1\"",
        );
        assert!(manifest(&bad).version().is_err());
        assert!(Manifest::parse(b"not json").is_err());
        assert!(Manifest::parse(b"{}").is_err());
    }
}
