//! The `lopi backup` file: a tar archive inside an `age` passphrase-encrypted file.
//! Both are open formats, so a backup can also be opened without lopi:
//! `age -d lopi-backup-….age | tar x`.
//!
//! ```text
//! lopi-backup/manifest.toml     format, creation time, which key file is which
//! lopi-backup/profiles.toml     the profiles file, byte for byte
//! lopi-backup/secrets.toml      saved passwords by profile id (only if any)
//! lopi-backup/keys/<n>-<name>   private keys (and `.pub` files)
//! ```
//!
//! Everything here works on bytes in memory: no files, no prompts. Secret-holding buffers
//! are wiped when dropped.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use age::secrecy::SecretString;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The bundle layout this build writes; newer ones are refused rather than half-restored.
pub const FORMAT: u32 = 1;
const ROOT: &str = "lopi-backup";
/// No real backup comes near this; it stops a damaged or hostile file from using up memory.
const MAX_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub created_at: String,
    pub lopi_version: String,
    #[serde(default)]
    pub keys: Vec<KeyEntry>,
}

/// A key file: where profiles expect it, and where it sits in the archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEntry {
    /// As written in the profiles file, e.g. `~/.ssh/id_vps`.
    pub path: String,
    /// Archive path of the private key, relative to the bundle root.
    pub file: String,
    /// Archive path of the matching `.pub` file, if there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<String>,
}

pub struct KeyFile {
    pub path: String,
    pub private: Zeroizing<Vec<u8>>,
    pub public: Option<Vec<u8>>,
}

pub struct Bundle {
    pub created_at: String,
    pub lopi_version: String,
    pub profiles: String,
    /// Profile id → password.
    pub passwords: BTreeMap<String, Zeroizing<String>>,
    pub keys: Vec<KeyFile>,
}

#[derive(Serialize, Deserialize, Default)]
struct Secrets {
    #[serde(default)]
    passwords: BTreeMap<String, String>,
}

/// Bundle → tar bytes.
pub fn pack(bundle: &Bundle) -> Result<Zeroizing<Vec<u8>>> {
    let mut entries = Vec::new();
    for (n, key) in bundle.keys.iter().enumerate() {
        let name = file_name(&key.path);
        let file = format!("keys/{}-{name}", n + 1);
        let public = key.public.as_ref().map(|_| format!("{file}.pub"));
        entries.push(KeyEntry {
            path: key.path.clone(),
            file,
            public,
        });
    }
    let manifest = Manifest {
        format: FORMAT,
        created_at: bundle.created_at.clone(),
        lopi_version: bundle.lopi_version.clone(),
        keys: entries.clone(),
    };

    // Sized up front so the buffer is not reallocated (which would leave unwiped copies).
    let data: usize = bundle.profiles.len()
        + bundle
            .passwords
            .values()
            .map(|p| p.len() + 64)
            .sum::<usize>()
        + bundle
            .keys
            .iter()
            .map(|k| k.private.len() + k.public.as_ref().map_or(0, Vec::len))
            .sum::<usize>();
    let headers = (4 + 2 * bundle.keys.len()) * 1024;
    let mut tar = tar::Builder::new(Vec::with_capacity(data + headers + 64 * 1024));
    add(
        &mut tar,
        "manifest.toml",
        toml::to_string(&manifest)?.as_bytes(),
    )?;
    add(&mut tar, "profiles.toml", bundle.profiles.as_bytes())?;
    if !bundle.passwords.is_empty() {
        let secrets = Secrets {
            passwords: bundle
                .passwords
                .iter()
                .map(|(id, password)| (id.clone(), password.to_string()))
                .collect(),
        };
        let text = Zeroizing::new(toml::to_string(&secrets)?);
        // The plain copies inside `secrets` cannot be wiped by Zeroizing; clear them by hand.
        let Secrets { passwords } = secrets;
        for mut password in passwords.into_values() {
            zeroize::Zeroize::zeroize(&mut password);
        }
        add(&mut tar, "secrets.toml", text.as_bytes())?;
    }
    for (key, entry) in bundle.keys.iter().zip(&entries) {
        add(&mut tar, &entry.file, &key.private)?;
        if let (Some(public), Some(name)) = (&key.public, &entry.public) {
            add(&mut tar, name, public)?;
        }
    }
    Ok(Zeroizing::new(tar.into_inner()?))
}

fn add(tar: &mut tar::Builder<Vec<u8>>, name: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    tar.append_data(&mut header, format!("{ROOT}/{name}"), data)
        .with_context(|| format!("cannot add {name} to the backup"))
}

/// The last path component of a stored key path, made safe for an archive name.
fn file_name(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or("key");
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        "key".into()
    } else {
        clean
    }
}

/// Tar bytes → bundle. Only the files listed above are read; anything else (or a path that
/// tries to leave the bundle) is rejected.
pub fn unpack(tar_bytes: &[u8]) -> Result<Bundle> {
    let mut files: BTreeMap<String, Zeroizing<Vec<u8>>> = BTreeMap::new();
    let mut archive = tar::Archive::new(tar_bytes);
    for entry in archive
        .entries()
        .context("the backup's contents are damaged")?
    {
        let mut entry = entry.context("the backup's contents are damaged")?;
        let path = entry.path()?.to_string_lossy().replace('\\', "/");
        let Some(name) = path.strip_prefix(&format!("{ROOT}/")) else {
            bail!("unexpected file in the backup: {path}");
        };
        if name.is_empty() || name.split('/').any(|part| part == ".." || part.is_empty()) {
            bail!("unsafe file name in the backup: {path}");
        }
        let mut data = Zeroizing::new(Vec::new());
        entry.read_to_end(&mut data)?;
        files.insert(name.to_string(), data);
    }

    let text = |name: &str| -> Result<String> {
        let bytes = files
            .get(name)
            .with_context(|| format!("the backup has no {name}"))?;
        String::from_utf8(bytes.to_vec())
            .with_context(|| format!("{name} in the backup is not text"))
    };
    let manifest: Manifest =
        toml::from_str(&text("manifest.toml")?).context("the backup's manifest is damaged")?;
    if manifest.format > FORMAT {
        bail!(
            "this backup was made by a newer lopi (format {}); upgrade lopi to restore it",
            manifest.format
        );
    }
    let profiles = text("profiles.toml")?;

    let mut passwords = BTreeMap::new();
    if files.contains_key("secrets.toml") {
        let secrets_text = Zeroizing::new(text("secrets.toml")?);
        let secrets: Secrets =
            toml::from_str(&secrets_text).context("the backup's saved passwords are damaged")?;
        for (id, password) in secrets.passwords {
            passwords.insert(id, Zeroizing::new(password));
        }
    }

    let mut keys = Vec::new();
    for entry in &manifest.keys {
        let private = files
            .get(&entry.file)
            .with_context(|| format!("the backup lists {} but does not contain it", entry.file))?
            .clone();
        let public = match &entry.public {
            Some(name) => Some(
                files
                    .get(name)
                    .with_context(|| format!("the backup lists {name} but does not contain it"))?
                    .to_vec(),
            ),
            None => None,
        };
        keys.push(KeyFile {
            path: entry.path.clone(),
            private,
            public,
        });
    }

    Ok(Bundle {
        created_at: manifest.created_at,
        lopi_version: manifest.lopi_version,
        profiles,
        passwords,
        keys,
    })
}

/// Whether `bytes` is an age file (as opposed to a plain profiles file).
pub fn is_encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(b"age-encryption.org/v1")
}

/// Encrypts with a passphrase. `work_factor` (scrypt log2 N) lowers the cost for tests;
/// `None` uses age's default, tuned to take about a second.
pub fn encrypt(plain: &[u8], passphrase: &str, work_factor: Option<u8>) -> Result<Vec<u8>> {
    let mut recipient = age::scrypt::Recipient::new(SecretString::from(passphrase.to_owned()));
    if let Some(factor) = work_factor {
        recipient.set_work_factor(factor);
    }
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .context("cannot set up encryption")?;
    let mut out = Vec::new();
    let mut writer = encryptor.wrap_output(&mut out)?;
    writer.write_all(plain)?;
    writer.finish()?;
    Ok(out)
}

/// Decrypts a passphrase-encrypted file. age itself refuses files whose key derivation
/// would take unreasonably long.
pub fn decrypt(cipher: &[u8], passphrase: &str) -> Result<Zeroizing<Vec<u8>>> {
    let decryptor = age::Decryptor::new_buffered(cipher).context("this is not a lopi backup")?;
    if !decryptor.is_scrypt() {
        bail!("this backup is not protected by a passphrase, so lopi cannot open it");
    }
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| anyhow::anyhow!("wrong passphrase, or the backup file is damaged"))?;
    let mut plain = Zeroizing::new(Vec::new());
    (&mut reader)
        .take(MAX_SIZE)
        .read_to_end(&mut plain)
        .context("the backup file is damaged")?;
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Low scrypt cost so tests run fast.
    const FAST: Option<u8> = Some(10);

    fn sample() -> Bundle {
        let mut passwords = BTreeMap::new();
        passwords.insert(
            "id1".to_string(),
            Zeroizing::new("p@ss \"word\"".to_string()),
        );
        Bundle {
            created_at: "2026-10-01T10:00:00Z".into(),
            lopi_version: "0.2.0".into(),
            profiles: "# comment\n[profiles.vps]\nhost = \"h\"\n".into(),
            passwords,
            keys: vec![
                KeyFile {
                    path: "~/.ssh/id_vps".into(),
                    private: Zeroizing::new(b"PRIVATE".to_vec()),
                    public: Some(b"PUBLIC".to_vec()),
                },
                KeyFile {
                    path: r"D:\keys\my key".into(),
                    private: Zeroizing::new(b"OTHER".to_vec()),
                    public: None,
                },
            ],
        }
    }

    #[test]
    fn pack_unpack_round_trip() {
        let tar = pack(&sample()).unwrap();
        let back = unpack(&tar).unwrap();
        assert_eq!(back.profiles, sample().profiles);
        assert_eq!(back.passwords["id1"].as_str(), "p@ss \"word\"");
        assert_eq!(back.keys.len(), 2);
        assert_eq!(back.keys[0].path, "~/.ssh/id_vps");
        assert_eq!(back.keys[0].private.as_slice(), b"PRIVATE");
        assert_eq!(back.keys[0].public.as_deref(), Some(&b"PUBLIC"[..]));
        assert_eq!(back.keys[1].path, r"D:\keys\my key");
        assert_eq!(back.keys[1].public, None);
    }

    #[test]
    fn no_secrets_file_without_passwords() {
        let mut bundle = sample();
        bundle.passwords.clear();
        let tar = pack(&bundle).unwrap();
        let names: Vec<String> = tar::Archive::new(tar.as_slice())
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            !names.iter().any(|n| n.ends_with("secrets.toml")),
            "{names:?}"
        );
        assert!(
            names.contains(&"lopi-backup/keys/2-my_key".to_string()),
            "{names:?}"
        );
        assert!(unpack(&tar).unwrap().passwords.is_empty());
    }

    #[test]
    fn encrypt_decrypt_and_wrong_passphrase() {
        let tar = pack(&sample()).unwrap();
        let cipher = encrypt(&tar, "correct horse battery", FAST).unwrap();
        assert!(is_encrypted(&cipher));
        assert!(
            !cipher.windows(7).any(|w| w == b"PRIVATE"),
            "plaintext leaked"
        );

        let plain = decrypt(&cipher, "correct horse battery").unwrap();
        assert_eq!(unpack(&plain).unwrap().profiles, sample().profiles);

        let err = decrypt(&cipher, "wrong").unwrap_err().to_string();
        assert!(err.contains("wrong passphrase"), "{err}");
        assert!(decrypt(b"[profiles.a]\nhost = \"h\"\n", "x").is_err());
        assert!(!is_encrypted(b"[profiles.a]"));
    }

    #[test]
    fn newer_or_tampered_bundles_are_refused() {
        fn tar_with(files: &[(&str, &[u8])]) -> Vec<u8> {
            let mut tar = tar::Builder::new(Vec::new());
            for (name, data) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o600);
                header.set_cksum();
                tar.append_data(&mut header, name, *data).unwrap();
            }
            tar.into_inner().unwrap()
        }
        let newer = tar_with(&[
            (
                "lopi-backup/manifest.toml",
                b"format = 99\ncreated_at = \"x\"\nlopi_version = \"9\"\n",
            ),
            ("lopi-backup/profiles.toml", b""),
        ]);
        assert!(
            unpack(&newer)
                .err()
                .unwrap()
                .to_string()
                .contains("newer lopi")
        );

        let outside = tar_with(&[("elsewhere/manifest.toml", b"")]);
        assert!(unpack(&outside).is_err());

        let missing_key = tar_with(&[
            (
                "lopi-backup/manifest.toml",
                b"format = 1\ncreated_at = \"x\"\nlopi_version = \"1\"\n[[keys]]\npath = \"~/k\"\nfile = \"keys/1-k\"\n",
            ),
            ("lopi-backup/profiles.toml", b""),
        ]);
        assert!(
            unpack(&missing_key)
                .err()
                .unwrap()
                .to_string()
                .contains("does not contain")
        );
    }
}
