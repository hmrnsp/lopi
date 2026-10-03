use std::env;
use std::ffi::{OsStr, OsString};

use anyhow::{Context, Result, anyhow};

use crate::askpass;
use crate::config::{Config, Profile, store};
use crate::resolve::resolve;
use crate::secrets::{KeyringStore, SecretStore};
use crate::ssh::args::{add_option, build_args, destination};
use crate::ssh::jump::{case_mismatch_warning, jump_case_mismatches, resolve_jump};
use crate::ssh::{self};
use crate::{state, time};

/// `lopi <name> [ssh options...] [-- remote command]` and `lopi connect ...`.
pub fn run(query: &OsStr, extra: &[OsString]) -> Result<i32> {
    let config = store::load()?;
    let query = query
        .to_str()
        .ok_or_else(|| anyhow!("no profile named '{}'", query.display()))?;
    let (name, _) = resolve(&config, query)?;
    connect(&config, name, extra)
}

/// The profile called exactly `name`, validated, with jump profile names replaced by their
/// address (warning about jump items in another letter case). Ready for `build_args`.
pub fn prepare(config: &Config, name: &str) -> Result<Profile> {
    let mut profile = config
        .profiles
        .get(name)
        .with_context(|| format!("internal error: no profile '{name}'"))?
        .clone();
    profile
        .validate()
        .with_context(|| format!("profile '{name}' is invalid; fix it with `lopi edit {name}`"))?;
    if let Some(jump) = &profile.jump {
        for (item, other) in jump_case_mismatches(config, jump) {
            eprintln!("lopi: warning: {}", case_mismatch_warning(item, other));
        }
    }
    profile.jump = profile.jump.map(|jump| resolve_jump(config, &jump));
    Ok(profile)
}

/// Connects to the profile called exactly `name`.
pub fn connect(config: &Config, name: &str, extra: &[OsString]) -> Result<i32> {
    let profile = prepare(config, name)?;
    let mut args = build_args(&profile, extra, dirs::home_dir().as_deref());
    let env = if profile.uses_password() {
        password_env(name, &profile, &mut args)?
    } else {
        Vec::new()
    };
    // Recorded before launching: on Unix, exec never returns. History must never stop a
    // connection, so failures are ignored.
    let _ = state::record_use(config, name, time::unix_now());
    let code = ssh::launch(&ssh::ssh_bin(config.ssh_bin.as_deref()), &args, &env)?;
    // Only reached on Windows (Unix execs). 255 is any ssh failure, so this is a hint.
    if code == 255 && !env.is_empty() {
        eprintln!(
            "lopi: note: if the login failed because the password changed, \
             update it with `lopi passwd {name}`"
        );
    }
    Ok(code)
}

/// The environment that makes ssh ask lopi (as askpass) for the saved password. Empty,
/// with a note, when there is nothing to fill in: ssh then asks as usual.
fn password_env(
    name: &str,
    profile: &Profile,
    args: &mut Vec<OsString>,
) -> Result<Vec<(OsString, OsString)>> {
    let secrets = KeyringStore::new();
    if let Err(err) = secrets.status() {
        eprintln!("lopi: note: {err:#}; ssh will ask for the password");
        return Ok(Vec::new());
    }
    let id = match &profile.id {
        Some(id) => id.clone(),
        None => assign_id(name)?,
    };
    match secrets.get(&id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            eprintln!(
                "lopi: note: no saved password for '{name}'; save one with `lopi passwd {name}`"
            );
            return Ok(Vec::new());
        }
        Err(err) => {
            eprintln!("lopi: note: {err:#}; ssh will ask for the password");
            return Ok(Vec::new());
        }
    }
    let exe = env::current_exe().context("cannot find this program's own file")?;
    // A wrong saved password must not be tried again and again.
    add_option(args, &["-o", "NumberOfPasswordPrompts=1"]);
    Ok(vec![
        ("SSH_ASKPASS".into(), exe.into_os_string()),
        ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
        (askpass::ID_ENV.into(), id.into()),
        (askpass::TARGET_ENV.into(), destination(profile).into()),
    ])
}

/// A hand-written profile has no id yet; saving the file gives every profile one.
fn assign_id(name: &str) -> Result<String> {
    store::mutate(|config| {
        config
            .profiles
            .get(name)
            .and_then(|profile| profile.id.clone())
            .with_context(|| format!("profile '{name}' disappeared"))
    })
}
