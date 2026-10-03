//! `lopi update`: find the latest release, download the archive for this system, check it
//! and replace the running binary. Pure parts (`version`, `manifest`, `checksum`, `channel`'s
//! `detect`) are separate so they can be tested without a network.

pub mod channel;
pub mod checksum;
pub mod manifest;
pub mod version;

/// The release target whose archive fits this build, or `None` where no binary is released.
/// Linux builds take the static musl binary. Windows on ARM runs the x86_64 one.
pub const RELEASE_TARGET: Option<&str> = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("x86_64-unknown-linux-musl")
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    Some("aarch64-unknown-linux-musl")
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    Some("x86_64-apple-darwin")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("aarch64-apple-darwin")
} else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("x86_64-pc-windows-msvc")
} else {
    None
};

/// Release archives are `.zip` for Windows and `.tar.xz` everywhere else.
pub const ARCHIVE_SUFFIX: &str = if cfg!(windows) { ".zip" } else { ".tar.xz" };
