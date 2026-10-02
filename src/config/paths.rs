use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// Overrides the profiles file location (used by tests).
pub const CONFIG_ENV: &str = "LOPI_CONFIG";
/// Overrides both the data dir (backups) and the state dir (history). Tests set this so
/// they never touch the real user directories.
pub const DATA_DIR_ENV: &str = "LOPI_DATA_DIR";

const APP: &str = "lopi";

/// `~/.config/lopi/profiles.toml` on Linux, `%APPDATA%\lopi\profiles.toml` on Windows.
pub fn config_file() -> Result<PathBuf> {
    if let Some(path) = env_path(CONFIG_ENV) {
        return Ok(path);
    }
    let dir = dirs::config_dir().context("cannot determine the config directory for this user")?;
    Ok(dir.join(APP).join("profiles.toml"))
}

/// Data that should follow the user (backups): `~/.local/share/lopi` on Linux,
/// `%APPDATA%\lopi` on Windows.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(path) = env_path(DATA_DIR_ENV) {
        return Ok(path);
    }
    let dir = dirs::data_dir().context("cannot determine the data directory for this user")?;
    Ok(dir.join(APP))
}

/// Machine-local data (connection history): `~/.local/share/lopi` on Linux,
/// `%LOCALAPPDATA%\lopi` on Windows.
pub fn state_dir() -> Result<PathBuf> {
    if let Some(path) = env_path(DATA_DIR_ENV) {
        return Ok(path);
    }
    let dir = dirs::data_local_dir()
        .context("cannot determine the local data directory for this user")?;
    Ok(dir.join(APP))
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
