use anyhow::{Context, Result};

use super::{ask_new_password, password_store, require_terminal};
use crate::config::{Auth, store};
use crate::resolve::resolve_exact;
use crate::secrets::{KeyringStore, SecretStore};
use crate::ssh::args::destination;
use crate::state;
use crate::ui::picker::pick_profile;
use crate::ui::prompt::TerminalPrompter;

/// `lopi passwd [name]`: saves or changes a profile's password and switches it to
/// password login. `--remove` deletes the saved password and stops password login.
pub fn run(name: Option<String>, remove: bool) -> Result<i32> {
    let config = store::load()?;
    let name = match &name {
        Some(name) => resolve_exact(&config, name)?.0.to_string(),
        None => {
            require_terminal("lopi passwd <name>")?;
            pick_profile(
                &config,
                &state::load(),
                &mut TerminalPrompter,
                "Password for",
            )?
            .to_string()
        }
    };
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
