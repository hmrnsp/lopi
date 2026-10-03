pub mod add;
pub mod backup;
pub mod complete;
pub mod connect;
pub mod doctor;
pub mod edit;
pub mod install;
pub mod list;
pub mod passwd;
pub mod path;
pub mod pick;
pub mod restore;
pub mod rm;
pub mod uninstall;

use std::env;

use anyhow::{Context, Result, bail};

use zeroize::Zeroizing;

use crate::config::Config;
use crate::config::key_path::normalize_key;
use crate::secrets::{KeyringStore, SecretStore};
use crate::ssh::args::expand_tilde;
use crate::ssh::jump::{
    case_mismatch_warning, jump_case_mismatches, jump_dependents, jump_profiles_with_keys,
};
use crate::ui::prompt::TerminalPrompter;
use crate::ui::tty;
use crate::ui::wizard::ask_password;

/// Fails unless prompts can be shown. `instead` says what to pass on the command line
/// instead of being asked.
fn require_terminal(instead: &str) -> Result<()> {
    if tty::interactive() {
        return Ok(());
    }
    let hint = tty::not_interactive_hint()
        .map(|hint| format!(" ({hint})"))
        .unwrap_or_default();
    bail!("cannot ask questions because this is not a terminal{hint}; use {instead}")
}

/// Passwords are only ever typed in, so asking for one needs a terminal.
const PASSWORD_NEEDS_TERMINAL: &str =
    "a terminal (passwords are typed in, never passed as arguments)";

/// The credential store, checked before anyone types a password that could not be saved.
fn password_store() -> Result<KeyringStore> {
    let store = KeyringStore::new();
    store
        .status()
        .context("cannot save passwords on this computer")?;
    Ok(store)
}

/// Asks for a new password for `target` (hidden, typed twice).
fn ask_new_password(target: &str) -> Result<Zeroizing<String>> {
    require_terminal(PASSWORD_NEEDS_TERMINAL)?;
    ask_password(&mut TerminalPrompter, target)
}

/// Saves the password once the profile is stored (its id is needed). The profile is
/// already saved, so a failure here is a warning with the way to retry.
fn save_password(store: &dyn SecretStore, name: &str, id: Option<&str>, password: &str) {
    let result = match id {
        Some(id) => store.set(id, password),
        None => Err(anyhow::anyhow!(
            "internal error: profile '{name}' has no id"
        )),
    };
    match result {
        Ok(()) => println!("saved the password for '{name}' in the system credential store"),
        Err(err) => {
            eprintln!("lopi: warning: {err:#}; save it later with `lopi passwd {name}`")
        }
    }
}

/// Deletes a saved password that is no longer used. Missing is fine; a failure is a
/// warning (the profile change itself is already saved).
fn forget_password(name: &str, id: Option<&str>) {
    let Some(id) = id else { return };
    match KeyringStore::new().delete(id) {
        Ok(true) => println!("deleted the saved password for '{name}'"),
        Ok(false) => {}
        Err(err) => eprintln!("lopi: warning: {err:#}"),
    }
}

/// Converts a `--key` value to its stored form; `""` means "no key". Warns (but does not
/// fail) when the file does not exist, since the key may be copied over later.
fn stored_key(raw: String) -> Result<Option<String>> {
    if raw.is_empty() {
        return Ok(None);
    }
    let cwd = env::current_dir().context("cannot read the current directory")?;
    let home = dirs::home_dir();
    let stored = normalize_key(&raw, &cwd, home.as_deref())?;
    let on_disk = expand_tilde(&stored, home.as_deref());
    if !on_disk.is_file() {
        eprintln!(
            "lopi: warning: key file {} does not exist (saved anyway)",
            on_disk.display()
        );
    }
    Ok(Some(stored))
}

/// `""` on the command line means "clear this field".
fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// Repeated values with `""` entries dropped, so `--forward ""` clears the list.
fn non_empty_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect()
}

/// Warnings about a saved jump: a name in another letter case than a profile (used as a
/// host name; also repeated on every connection), and jump profiles whose key ssh will not
/// use (`-i` applies to the destination only; said once, here).
fn warn_about_jump(config: &Config, name: &str) {
    let Some(jump) = config.profiles.get(name).and_then(|p| p.jump.as_deref()) else {
        return;
    };
    for (item, other) in jump_case_mismatches(config, jump) {
        eprintln!("lopi: warning: {}", case_mismatch_warning(item, other));
    }
    for jump_profile in jump_profiles_with_keys(config, jump) {
        eprintln!(
            "lopi: note: ssh does not use the key of '{jump_profile}' for the jump; \
             add it to ssh-agent (`ssh-add <key>`) or set IdentityFile for that host in ~/.ssh/config"
        );
    }
}

/// `name` just came into being (added, or renamed to): profiles that jumped through a host
/// of that name now jump through this profile instead, since profile names win over hosts.
fn warn_about_captured_jumps(name: &str, captured: &[impl AsRef<str>]) {
    for other in captured.iter().map(AsRef::as_ref) {
        eprintln!(
            "lopi: warning: '{other}' jumps through '{name}', which now means this profile \
             (it was a host name)"
        );
    }
}

/// Refuses to remove a profile that others jump through: their `jump` would silently turn
/// into a host name, which may reach another machine.
fn check_no_jump_dependents(config: &Config, name: &str) -> Result<()> {
    let dependents = jump_dependents(config, name);
    let Some(first) = dependents.first() else {
        return Ok(());
    };
    bail!(
        "'{name}' is the jump host of: {}; change those first, e.g. \
         `lopi edit {first} --jump <other>` or `lopi edit {first} --jump \"\"`; nothing was removed",
        dependents.join(", ")
    )
}
