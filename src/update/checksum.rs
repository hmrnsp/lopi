//! SHA-256 checks of downloaded archives. Pure.

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

/// Reads a `.sha256` file as cargo-dist writes it: `<64 hex digits> *<file name>`.
/// Returns the digest in lowercase. The file name, when present, must be `expected_name`.
pub fn parse_sha256_file(text: &str, expected_name: &str) -> Result<String> {
    let mut words = text.split_whitespace();
    let Some(hex) = words.next().filter(|hex| is_sha256_hex(hex)) else {
        bail!("the checksum file of {expected_name} is not a SHA-256 checksum");
    };
    if let Some(name) = words.next() {
        let name = name.strip_prefix('*').unwrap_or(name);
        if name != expected_name || words.next().is_some() {
            bail!("the checksum file of {expected_name} is for '{name}'");
        }
    }
    Ok(hex.to_ascii_lowercase())
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Fails unless `bytes` has the digest `expected` (lowercase hex).
pub fn verify(bytes: &[u8], expected: &str, name: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    if actual != expected {
        bail!(
            "{name} is not the file that was published (SHA-256 {actual}, expected {expected}); \
             nothing was changed"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn hashes() {
        assert_eq!(sha256_hex(b""), EMPTY);
        assert!(verify(b"", EMPTY, "a.tar.xz").is_ok());
        let err = verify(b"x", EMPTY, "a.tar.xz").unwrap_err().to_string();
        assert!(err.contains("not the file that was published"), "{err}");
    }

    #[test]
    fn reads_checksum_files() {
        let upper = EMPTY.to_ascii_uppercase();
        for text in [
            format!("{EMPTY} *a.tar.xz\n"),
            format!("{EMPTY}  a.tar.xz\r\n"),
            format!("{upper} *a.tar.xz"),
            format!("{EMPTY}\n"),
        ] {
            assert_eq!(
                parse_sha256_file(&text, "a.tar.xz").unwrap(),
                EMPTY,
                "{text}"
            );
        }
        for text in [
            String::new(),
            "abc *a.tar.xz".to_string(),
            format!("{}g *a.tar.xz", &EMPTY[..63]),
            format!("{EMPTY} *b.tar.xz"),
            format!("{EMPTY} *a.tar.xz extra"),
        ] {
            assert!(parse_sha256_file(&text, "a.tar.xz").is_err(), "{text}");
        }
    }
}
