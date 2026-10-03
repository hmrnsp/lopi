//! Takes the lopi binary out of a release archive, in memory. Pure.
//!
//! cargo-dist puts it at `lopi-ssh-<target>/lopi` in a `.tar.xz`, and at `lopi.exe` in the
//! Windows `.zip`. Exactly one regular file with that name must be there; links and folders
//! of that name do not count.

use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail};

/// No lopi binary comes close; anything larger is not one.
const MAX_EXE_SIZE: u64 = 256 * 1024 * 1024;

/// The binary for this system, from the archive for this system.
#[cfg(not(windows))]
pub fn extract_exe(archive: &[u8]) -> Result<Vec<u8>> {
    from_tar_xz(archive, "lopi")
}

/// The binary for this system, from the archive for this system.
#[cfg(windows)]
pub fn extract_exe(archive: &[u8]) -> Result<Vec<u8>> {
    from_zip(archive, "lopi.exe")
}

/// The single regular file called `name` (in any folder) inside a `.tar.xz`.
#[cfg(any(not(windows), test))]
fn from_tar_xz(archive: &[u8], name: &str) -> Result<Vec<u8>> {
    let mut tar_bytes = Vec::new();
    lzma_rs::xz_decompress(&mut Cursor::new(archive), &mut tar_bytes)
        .context("the downloaded archive is not a valid .tar.xz")?;
    let mut tar = tar::Archive::new(Cursor::new(tar_bytes));
    let mut found = None;
    for entry in tar
        .entries()
        .context("cannot read the downloaded archive")?
    {
        let mut entry = entry.context("cannot read the downloaded archive")?;
        let matches = entry
            .path()
            .ok()
            .is_some_and(|path| path.file_name().is_some_and(|file| file == name));
        if !matches || !entry.header().entry_type().is_file() {
            continue;
        }
        if found.is_some() {
            bail!("the downloaded archive holds more than one {name}");
        }
        let size = entry.size();
        found = Some(read_limited(&mut entry, size, name)?);
    }
    found.with_context(|| format!("the downloaded archive has no {name}"))
}

/// The single regular file called `name` (in any folder) inside a `.zip`.
#[cfg(any(windows, test))]
fn from_zip(archive: &[u8], name: &str) -> Result<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive))
        .context("the downloaded archive is not a valid .zip")?;
    let mut found = None;
    for index in 0..zip.len() {
        let mut file = zip
            .by_index(index)
            .context("cannot read the downloaded archive")?;
        let matches = file
            .enclosed_name()
            .is_some_and(|path| path.file_name().is_some_and(|file| file == name));
        if !matches || !file.is_file() || file.is_symlink() {
            continue;
        }
        if found.is_some() {
            bail!("the downloaded archive holds more than one {name}");
        }
        let size = file.size();
        found = Some(read_limited(&mut file, size, name)?);
    }
    found.with_context(|| format!("the downloaded archive has no {name}"))
}

fn read_limited(reader: &mut impl Read, size: u64, name: &str) -> Result<Vec<u8>> {
    if size == 0 || size > MAX_EXE_SIZE {
        bail!("the {name} in the downloaded archive has an unexpected size ({size} bytes)");
    }
    let mut bytes = Vec::with_capacity(size as usize);
    reader
        .take(MAX_EXE_SIZE)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot unpack {name} from the downloaded archive"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// An entry of a test archive.
    enum Item<'a> {
        File(&'a str, &'a [u8]),
        Dir(&'a str),
        Link(&'a str, &'a str),
    }

    /// A `.tar.xz` like cargo-dist's.
    fn tar_xz(items: &[Item<'_>]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for item in items {
            let mut header = tar::Header::new_gnu();
            match *item {
                Item::File(path, bytes) => {
                    header.set_entry_type(tar::EntryType::Regular);
                    header.set_size(bytes.len() as u64);
                    header.set_mode(0o755);
                    builder.append_data(&mut header, path, bytes).unwrap();
                }
                Item::Dir(path) => {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                    header.set_mode(0o755);
                    builder.append_data(&mut header, path, &[][..]).unwrap();
                }
                Item::Link(path, target) => {
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    builder.append_link(&mut header, path, target).unwrap();
                }
            }
        }
        let tar = builder.into_inner().unwrap();
        let mut xz = Vec::new();
        lzma_rs::xz_compress(&mut Cursor::new(tar), &mut xz).unwrap();
        xz
    }

    fn zip(items: &[Item<'_>]) -> Vec<u8> {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for item in items {
            match *item {
                Item::File(path, bytes) => {
                    writer.start_file(path, options).unwrap();
                    writer.write_all(bytes).unwrap();
                }
                Item::Dir(path) => writer.add_directory(path, options).unwrap(),
                Item::Link(path, target) => writer.add_symlink(path, target, options).unwrap(),
            }
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn finds_the_binary_in_a_tar_xz() {
        let archive = tar_xz(&[
            Item::Dir("lopi-ssh-x86_64-unknown-linux-musl/"),
            Item::File("lopi-ssh-x86_64-unknown-linux-musl/README.md", b"readme"),
            Item::File("lopi-ssh-x86_64-unknown-linux-musl/lopi", b"new lopi"),
        ]);
        assert_eq!(from_tar_xz(&archive, "lopi").unwrap(), b"new lopi");
        let err = from_tar_xz(&archive, "lopi.exe").unwrap_err().to_string();
        assert!(err.contains("has no lopi.exe"), "{err}");
    }

    #[test]
    fn tar_xz_needs_exactly_one_regular_file() {
        let two = tar_xz(&[Item::File("a/lopi", b"1"), Item::File("b/lopi", b"2")]);
        let err = from_tar_xz(&two, "lopi").unwrap_err().to_string();
        assert!(err.contains("more than one"), "{err}");

        let not_files = tar_xz(&[Item::Dir("lopi/"), Item::Link("x/lopi", "/bin/sh")]);
        let err = from_tar_xz(&not_files, "lopi").unwrap_err().to_string();
        assert!(err.contains("has no lopi"), "{err}");

        let empty = tar_xz(&[Item::File("lopi", b"")]);
        assert!(from_tar_xz(&empty, "lopi").is_err());

        assert!(from_tar_xz(b"not an archive", "lopi").is_err());
    }

    #[test]
    fn finds_the_binary_in_a_zip() {
        let archive = zip(&[
            Item::File("README.md", b"readme"),
            Item::File("lopi.exe", b"new lopi"),
        ]);
        assert_eq!(from_zip(&archive, "lopi.exe").unwrap(), b"new lopi");
        assert!(from_zip(&archive, "lopi").is_err());

        let two = zip(&[Item::File("lopi.exe", b"1"), Item::File("x/lopi.exe", b"2")]);
        let err = from_zip(&two, "lopi.exe").unwrap_err().to_string();
        assert!(err.contains("more than one"), "{err}");

        let not_files = zip(&[Item::Dir("lopi.exe/"), Item::Link("y/lopi.exe", "C:/x")]);
        let err = from_zip(&not_files, "lopi.exe").unwrap_err().to_string();
        assert!(err.contains("has no lopi.exe"), "{err}");

        // outside the archive's folder: never matched
        let escaping = zip(&[Item::File("../lopi.exe", b"evil")]);
        assert!(from_zip(&escaping, "lopi.exe").is_err());

        assert!(from_zip(b"not an archive", "lopi.exe").is_err());
    }

    #[test]
    fn extracts_for_this_system() {
        let name = format!("lopi{}", std::env::consts::EXE_SUFFIX);
        let archive = if cfg!(windows) {
            zip(&[Item::File(&name, b"bin")])
        } else {
            tar_xz(&[Item::File(&format!("dir/{name}"), b"bin")])
        };
        assert_eq!(extract_exe(&archive).unwrap(), b"bin");
    }
}
