//! How the running lopi was installed, which decides whether `lopi update` may replace
//! it. [`detect`] is pure; [`Facts::gather`] reads the files it needs.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::version::Version;
use crate::atomic;
use crate::install;

/// The app name cargo-dist and cargo record lopi under.
const APP_NAME: &str = "lopi-ssh";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// Copied by `lopi install`.
    LopiInstall,
    /// The install script; `receipt` is the file it left behind.
    Installer { receipt: PathBuf },
    /// `cargo install`: cargo keeps its own record, so only cargo should replace it.
    Cargo,
    /// Anywhere else: a build folder, a package manager, a copy run where it was unpacked.
    Unknown,
}

impl Channel {
    /// Whether `lopi update` replaces the binary itself.
    pub fn self_updates(&self) -> bool {
        matches!(self, Self::LopiInstall | Self::Installer { .. })
    }

    /// How to update this install, for messages.
    pub fn how_to_update(&self) -> &'static str {
        match self {
            Self::LopiInstall | Self::Installer { .. } => "run `lopi update`",
            Self::Cargo => "run `cargo install lopi-ssh --locked`",
            Self::Unknown => {
                "download it from https://github.com/hmrnsp/lopi/releases/latest and run \
                 `lopi install` from there, or run the install script again"
            }
        }
    }
}

/// Everything [`detect`] looks at. Paths are canonical (symlinks resolved), so plain
/// comparison means "the same file".
#[derive(Debug, Default)]
pub struct Facts {
    /// The running program.
    pub exe: PathBuf,
    /// Where `lopi install` puts lopi, if that file exists.
    pub installed_exe: Option<PathBuf>,
    /// `$CARGO_HOME/bin`, if it exists.
    pub cargo_bin: Option<PathBuf>,
    /// cargo's install records list lopi-ssh with the `lopi` binary.
    pub cargo_owns_lopi: bool,
    /// The install script's receipt, with the binary paths it stands for.
    pub receipt: Option<(PathBuf, Vec<PathBuf>)>,
}

/// Rules in order: `lopi install`'s copy; a cargo install (cargo's record wins over a
/// receipt, because replacing a file cargo tracks would confuse it); the install script's
/// copy; anything else.
pub fn detect(facts: &Facts) -> Channel {
    if facts.installed_exe.as_ref() == Some(&facts.exe) {
        return Channel::LopiInstall;
    }
    if facts.cargo_owns_lopi
        && facts.cargo_bin.is_some()
        && facts.exe.parent() == facts.cargo_bin.as_deref()
    {
        return Channel::Cargo;
    }
    if let Some((receipt, exes)) = &facts.receipt
        && exes.contains(&facts.exe)
    {
        return Channel::Installer {
            receipt: receipt.clone(),
        };
    }
    Channel::Unknown
}

impl Facts {
    /// Reads the receipt and cargo's records. Missing or unreadable files count as absent.
    pub fn gather(exe: &Path) -> Self {
        let canonical = |path: &Path| fs::canonicalize(path).ok();
        let cargo_home = cargo_home();
        let read = |path: Option<PathBuf>| path.and_then(|path| fs::read_to_string(path).ok());
        let receipt = receipt_path().and_then(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let exes = receipt_exes(&text)?
                .iter()
                .filter_map(|exe| canonical(exe))
                .collect();
            Some((path, exes))
        });
        Self {
            exe: canonical(exe).unwrap_or_else(|| exe.to_path_buf()),
            installed_exe: install::installed_exe().ok().and_then(|p| canonical(&p)),
            cargo_bin: cargo_home
                .as_ref()
                .and_then(|home| canonical(&home.join("bin"))),
            cargo_owns_lopi: cargo_owns_lopi(
                read(cargo_home.as_ref().map(|home| home.join(".crates2.json"))).as_deref(),
                read(cargo_home.as_ref().map(|home| home.join(".crates.toml"))).as_deref(),
            ),
            receipt,
        }
    }
}

fn cargo_home() -> Option<PathBuf> {
    env::var_os("CARGO_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".cargo")))
}

fn is_lopi_bin(name: &str) -> bool {
    name == "lopi" || name == "lopi.exe"
}

/// Whether cargo's records (`.crates2.json`, or the older `.crates.toml`) say that cargo
/// installed lopi-ssh's `lopi` binary. Keys look like `lopi-ssh 0.4.0 (registry+…)`.
pub fn cargo_owns_lopi(crates2_json: Option<&str>, crates_toml: Option<&str>) -> bool {
    let ours = |key: &str| key.split(' ').next() == Some(APP_NAME);

    #[derive(Deserialize)]
    struct Crates2 {
        installs: std::collections::BTreeMap<String, Install>,
    }
    #[derive(Deserialize)]
    struct Install {
        #[serde(default)]
        bins: Vec<String>,
    }
    if let Some(records) = crates2_json.and_then(|text| serde_json::from_str::<Crates2>(text).ok())
    {
        return records
            .installs
            .iter()
            .any(|(key, install)| ours(key) && install.bins.iter().any(|b| is_lopi_bin(b)));
    }

    #[derive(Deserialize)]
    struct CratesToml {
        v1: std::collections::BTreeMap<String, Vec<String>>,
    }
    crates_toml
        .and_then(|text| toml::from_str::<CratesToml>(text).ok())
        .is_some_and(|records| {
            records
                .v1
                .iter()
                .any(|(key, bins)| ours(key) && bins.iter().any(|b| is_lopi_bin(b)))
        })
}

/// Where the install script writes its receipt: `$XDG_CONFIG_HOME/lopi-ssh` or
/// `~/.config/lopi-ssh`; on Windows `%LOCALAPPDATA%\lopi-ssh` (also used by the shell script
/// under Git Bash), or `$XDG_CONFIG_HOME\lopi-ssh` when PowerShell's installer saw it set.
/// The first existing one is returned.
pub fn receipt_path() -> Option<PathBuf> {
    let xdg = env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from);
    let candidates: Vec<PathBuf> = if cfg!(windows) {
        xdg.into_iter().chain(dirs::data_local_dir()).collect()
    } else {
        xdg.or_else(|| dirs::home_dir().map(|home| home.join(".config")))
            .into_iter()
            .collect()
    };
    candidates
        .into_iter()
        .map(|dir| dir.join(APP_NAME).join(format!("{APP_NAME}-receipt.json")))
        .find(|path| path.is_file())
}

#[derive(Deserialize)]
struct Receipt {
    install_prefix: String,
    #[serde(default)]
    install_layout: String,
    source: ReceiptSource,
}

#[derive(Deserialize)]
struct ReceiptSource {
    app_name: String,
}

/// The paths a receipt's lopi binary may have: `<prefix>/bin/lopi` for the `cargo-home` and
/// `hierarchical` layouts, `<prefix>/lopi` for `flat`; both when the layout is not known.
/// `None` when the receipt is not readable or not lopi's.
pub fn receipt_exes(text: &str) -> Option<Vec<PathBuf>> {
    let receipt: Receipt = serde_json::from_str(text).ok()?;
    if receipt.source.app_name != APP_NAME || receipt.install_prefix.is_empty() {
        return None;
    }
    let prefix = PathBuf::from(&receipt.install_prefix);
    let exe = format!("lopi{}", env::consts::EXE_SUFFIX);
    let in_bin = prefix.join("bin").join(&exe);
    let flat = prefix.join(&exe);
    Some(match receipt.install_layout.as_str() {
        "cargo-home" | "hierarchical" => vec![in_bin],
        "flat" => vec![flat],
        _ => vec![in_bin, flat],
    })
}

/// Records the new version in the receipt, keeping everything else as it was.
pub fn set_receipt_version(path: &Path, version: Version) -> Result<()> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut receipt: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    let fields = receipt
        .as_object_mut()
        .with_context(|| format!("{} is not a receipt", path.display()))?;
    fields.insert("version".into(), version.to_string().into());
    let mut bytes = serde_json::to_vec(&receipt)?;
    bytes.push(b'\n');
    atomic::write(path, &bytes).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    fn facts(exe: &str) -> Facts {
        Facts {
            exe: p(exe),
            installed_exe: Some(p("/home/u/.local/bin/lopi")),
            cargo_bin: Some(p("/home/u/.cargo/bin")),
            cargo_owns_lopi: false,
            receipt: Some((p("/r.json"), vec![p("/home/u/.cargo/bin/lopi")])),
        }
    }

    #[test]
    fn detects_each_channel() {
        assert_eq!(
            detect(&facts("/home/u/.local/bin/lopi")),
            Channel::LopiInstall
        );
        assert_eq!(
            detect(&facts("/home/u/.cargo/bin/lopi")),
            Channel::Installer {
                receipt: p("/r.json")
            }
        );
        assert_eq!(
            detect(&facts("/home/u/src/lopi/target/debug/lopi")),
            Channel::Unknown
        );
        assert_eq!(detect(&facts("/usr/bin/lopi")), Channel::Unknown);

        let mut cargo = facts("/home/u/.cargo/bin/lopi");
        cargo.cargo_owns_lopi = true;
        assert_eq!(
            detect(&cargo),
            Channel::Cargo,
            "cargo's record wins over the receipt"
        );
        cargo.receipt = None;
        assert_eq!(detect(&cargo), Channel::Cargo);

        // cargo owns a lopi, but this one runs from elsewhere
        let mut elsewhere = facts("/home/u/.local/bin/lopi");
        elsewhere.cargo_owns_lopi = true;
        assert_eq!(detect(&elsewhere), Channel::LopiInstall);

        // no receipt (e.g. LOPI_SSH_DISABLE_UPDATE=1): not ours to replace
        let mut no_receipt = facts("/home/u/.cargo/bin/lopi");
        no_receipt.receipt = None;
        assert_eq!(detect(&no_receipt), Channel::Unknown);
        assert_eq!(detect(&Facts::default()), Channel::Unknown);
    }

    #[test]
    fn only_own_channels_self_update() {
        assert!(Channel::LopiInstall.self_updates());
        assert!(Channel::Installer { receipt: p("/r") }.self_updates());
        assert!(!Channel::Cargo.self_updates());
        assert!(!Channel::Unknown.self_updates());
        assert!(
            Channel::Cargo
                .how_to_update()
                .contains("cargo install lopi-ssh --locked")
        );
    }

    #[test]
    fn reads_cargo_records() {
        let crates2 = |key: &str, bins: &str| {
            format!(r#"{{"installs":{{"{key}":{{"version_req":null,"bins":{bins}}}}}}}"#)
        };
        let ours = "lopi-ssh 0.4.0 (registry+https://github.com/rust-lang/crates.io-index)";
        let git = "lopi-ssh 0.4.0 (git+https://github.com/hmrnsp/lopi#fed4b2d)";
        assert!(cargo_owns_lopi(Some(&crates2(ours, r#"["lopi"]"#)), None));
        assert!(cargo_owns_lopi(
            Some(&crates2(git, r#"["lopi.exe"]"#)),
            None
        ));
        assert!(!cargo_owns_lopi(
            Some(&crates2("lopi 1.0.0 (registry+x)", r#"["lopi"]"#)),
            None
        ));
        assert!(!cargo_owns_lopi(
            Some(&crates2("lopi-ssh-x 1.0.0 (x)", r#"["lopi"]"#)),
            None
        ));
        assert!(!cargo_owns_lopi(Some(&crates2(ours, "[]")), None));
        assert!(!cargo_owns_lopi(Some(r#"{"installs":{}}"#), None));
        assert!(!cargo_owns_lopi(None, None));

        let toml = format!("[v1]\n\"{ours}\" = [\"lopi\"]\n");
        assert!(cargo_owns_lopi(None, Some(&toml)));
        assert!(
            cargo_owns_lopi(Some("not json"), Some(&toml)),
            "falls back to .crates.toml"
        );
        assert!(!cargo_owns_lopi(
            None,
            Some("[v1]\n\"ripgrep 14.0.0 (x)\" = [\"rg\"]\n")
        ));
        assert!(!cargo_owns_lopi(None, Some("garbage")));
    }

    #[test]
    fn reads_receipts() {
        let exe = format!("lopi{}", env::consts::EXE_SUFFIX);
        let receipt = |layout: &str| {
            format!(
                r#"{{"binaries":["lopi"],"install_layout":"{layout}","install_prefix":"/home/u/.cargo","source":{{"app_name":"lopi-ssh","name":"lopi","owner":"hmrnsp","release_type":"github"}},"version":"0.4.0"}}"#
            )
        };
        let in_bin = p("/home/u/.cargo/bin").join(&exe);
        let flat = p("/home/u/.cargo").join(&exe);
        let one = std::slice::from_ref;
        assert_eq!(receipt_exes(&receipt("cargo-home")).unwrap(), one(&in_bin));
        assert_eq!(
            receipt_exes(&receipt("hierarchical")).unwrap(),
            one(&in_bin)
        );
        assert_eq!(receipt_exes(&receipt("flat")).unwrap(), one(&flat));
        assert_eq!(
            receipt_exes(&receipt("unspecified")).unwrap(),
            [in_bin, flat]
        );

        assert!(receipt_exes(&receipt("flat").replace("lopi-ssh", "other")).is_none());
        assert!(
            receipt_exes(r#"{"install_prefix":"","source":{"app_name":"lopi-ssh"}}"#).is_none()
        );
        assert!(receipt_exes("not json").is_none());
    }

    #[test]
    fn updates_only_the_receipt_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lopi-ssh-receipt.json");
        fs::write(
            &path,
            r#"{"binaries":["lopi"],"install_prefix":"/x","modify_path":true,"version":"0.4.0"}"#,
        )
        .unwrap();
        set_receipt_version(&path, Version(0, 5, 0)).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after["version"], "0.5.0");
        assert_eq!(after["install_prefix"], "/x");
        assert_eq!(after["modify_path"], true);
        assert_eq!(after["binaries"][0], "lopi");

        fs::write(&path, "[]").unwrap();
        assert!(set_receipt_version(&path, Version(0, 5, 0)).is_err());
    }
}
