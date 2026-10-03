use std::io::{self, IsTerminal};

use anyhow::{Context, Result, bail};
use zeroize::Zeroizing;

use super::{ask_new_password, password_store, require_terminal};
use crate::config::{Auth, Config, store};
use crate::resolve::resolve;
use crate::secrets::{KeyringStore, SecretStore};
use crate::ssh::args::destination;
use crate::state;
use crate::ui::picker::pick_profile;
use crate::ui::prompt::TerminalPrompter;
use crate::ui::reveal::show_secret;
use crate::ui::tty;

/// `lopi passwd [name]`: saves or changes a profile's password and switches it to
/// password login. `--remove` deletes the saved password and stops password login;
/// `--show` shows it on the terminal.
pub fn run(name: Option<String>, remove: bool, show: bool) -> Result<i32> {
    if show {
        // Before anything is read, so a pipe or file never gets near the password.
        require_screen()?;
    }
    let config = store::load()?;
    let name = match &name {
        Some(name) => resolve(&config, name)?.0.to_string(),
        None => {
            require_terminal("lopi passwd <name>")?;
            let message = if show {
                "Show the password of"
            } else {
                "Password for"
            };
            pick_profile(&config, &state::load(), &mut TerminalPrompter, message)?.to_string()
        }
    };
    if show {
        return show_password(&config, &name);
    }
    if remove {
        return remove_password(&name);
    }

    let secrets = password_store()?;
    let password = ask_new_password(&destination(&config.profiles[&name]))?;
    // Saving the profile first gives it an id (hand-written profiles may lack one).
    let id = store::mutate(|config| {
        let profile = config
            .profiles
            .get_mut(&name)
            .with_context(|| format!("profile '{name}' disappeared"))?;
        profile.auth = Some(Auth::Password);
        profile
            .id
            .clone()
            .context("internal error: profile without id")
    })?;
    secrets.set(&id, &password)?;
    println!("saved the password for '{name}'; `lopi {name}` now logs in without asking");
    Ok(0)
}

fn remove_password(name: &str) -> Result<i32> {
    let id = store::mutate(|config| {
        let profile = config
            .profiles
            .get_mut(name)
            .with_context(|| format!("profile '{name}' disappeared"))?;
        if profile.uses_password() {
            profile.auth = None;
        }
        Ok(profile.id.clone())
    })?;
    let deleted = match id {
        Some(id) => KeyringStore::new().delete(&id)?,
        None => false,
    };
    if deleted {
        println!("deleted the saved password for '{name}'; ssh will ask for it from now on");
    } else {
        println!("'{name}' had no saved password; it no longer uses password login");
    }
    Ok(0)
}

/// `--show` needs a keyboard and a screen on every side: with stdout or stderr sent to a
/// pipe or file, the password could end up there.
fn require_screen() -> Result<()> {
    if tty::interactive() && io::stdout().is_terminal() {
        return Ok(());
    }
    let hint = tty::not_interactive_hint()
        .map(|hint| format!(" ({hint})"))
        .unwrap_or_default();
    bail!("the password is shown only on a terminal, never written to a pipe or file{hint}")
}

/// Shows the saved password on the alternate screen until Enter. Reads only: the profiles
/// file is not touched.
fn show_password(config: &Config, name: &str) -> Result<i32> {
    let secrets = KeyringStore::new();
    secrets
        .status()
        .context("cannot read saved passwords on this computer")?;
    let password = saved_password(&secrets, config, name)?;
    let title = format!(
        "Password for '{name}' ({}):",
        destination(&config.profiles[name])
    );
    show_secret(&title, &password)?;
    Ok(0)
}

/// The saved password of `name`. A profile without an id never had one saved.
fn saved_password(
    secrets: &dyn SecretStore,
    config: &Config,
    name: &str,
) -> Result<Zeroizing<String>> {
    let id = config.profiles.get(name).and_then(|p| p.id.as_deref());
    let password = match id {
        Some(id) => secrets.get(id)?,
        None => None,
    };
    password.with_context(|| {
        format!("'{name}' has no saved password; save one with `lopi passwd {name}`")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;
    use crate::secrets::memory::MemoryStore;

    fn config() -> Config {
        let mut config = Config::default();
        let with_id = Profile {
            id: Some("id-vps".into()),
            auth: Some(Auth::Password),
            ..Profile::new("h")
        };
        config.profiles.insert("vps".into(), with_id);
        config.profiles.insert("bare".into(), Profile::new("h"));
        config
    }

    #[test]
    fn saved_password_is_read_by_profile_id() {
        let secrets = MemoryStore::default();
        secrets.set("id-vps", "s3cret").unwrap();
        let password = saved_password(&secrets, &config(), "vps").unwrap();
        assert_eq!(password.as_str(), "s3cret");
    }

    #[test]
    fn missing_password_says_how_to_save_one() {
        let secrets = MemoryStore::default();
        for name in ["vps", "bare"] {
            let err = saved_password(&secrets, &config(), name).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("'{name}' has no saved password; save one with `lopi passwd {name}`")
            );
        }
    }

    #[test]
    fn unavailable_store_is_an_error() {
        let secrets = MemoryStore {
            unavailable: true,
            ..MemoryStore::default()
        };
        let err = saved_password(&secrets, &config(), "vps").unwrap_err();
        assert!(err.to_string().contains("not available"), "{err}");
    }
}
