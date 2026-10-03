use anyhow::{Result, bail};

use super::{
    ask_new_password, check_jump_input, forget_password, non_empty, non_empty_list, password_store,
    require_terminal, save_password, stored_key, warn_about_captured_jumps, warn_about_jump,
};
use crate::cli::{AuthArg, EditArgs};
use crate::config::model::{parse_host_input, validate_name};
use crate::config::store;
use crate::resolve::resolve;
use crate::ssh::args::destination;
use crate::ssh::jump::{jump_dependents, rename_in_jump};
use crate::state;
use crate::ui::picker::pick_profile;
use crate::ui::prompt::TerminalPrompter;
use crate::ui::wizard::{self, EditPlan};

/// With flags, changes just those fields. Without flags (in a terminal), runs the guided
/// edit; without a name, lets the user choose the profile first.
pub fn run(mut args: EditArgs) -> Result<i32> {
    let config = store::load()?;
    // A wrong name is reported before anything about terminals.
    let name = match &args.name {
        Some(name) => resolve(&config, name)?.0,
        None => {
            require_terminal("lopi edit <name> [--host ...] [--port ...]")?;
            pick_profile(&config, &state::load(), &mut TerminalPrompter, "Edit")?
        }
    };
    let profile = &config.profiles[name];

    let plan = if wizard::has_changes(&args) {
        args.name = Some(name.to_string());
        // Switching to password login needs the password itself.
        let password = if args.auth == Some(AuthArg::Password) && !profile.uses_password() {
            password_store()?;
            Some(ask_new_password(&destination(profile))?)
        } else {
            None
        };
        EditPlan { args, password }
    } else {
        require_terminal(&format!(
            "flags to say what to change, e.g. `lopi edit {name} --port 2200 --host 10.0.0.5` \
             (see `lopi edit --help`)"
        ))?;
        let keys = wizard::local_keys(dirs::home_dir().as_deref());
        let plan = wizard::edit(&config, name, profile, &mut TerminalPrompter, keys)?;
        if !wizard::has_changes(&plan.args) {
            println!("nothing changed");
            return Ok(0);
        }
        plan
    };
    save(plan)
}

/// What `save` changed, beyond the profile itself.
struct Changed {
    old_name: String,
    new_name: String,
    used_password: bool,
    /// Profiles whose `jump` was updated to the new name.
    rewritten: Vec<String>,
    /// Profiles that jumped through a host with the new name and now reach this profile.
    captured: Vec<String>,
}

/// Applies the given fields to the profile named exactly `args.name`, then saves or
/// deletes its password to match the new login method.
fn save(plan: EditPlan) -> Result<i32> {
    let EditPlan { args, password } = plan;
    let EditArgs {
        name,
        host,
        user,
        port,
        key,
        jump,
        forwards,
        note,
        rename,
        auth,
    } = args;
    let Some(name) = name else {
        bail!("internal error: edit without a profile name");
    };
    if let Some(new_name) = &rename {
        validate_name(new_name)?;
    }
    let host = host.as_deref().map(parse_host_input).transpose()?;
    if let Some(jump) = &jump {
        check_jump_input(jump)?;
    }
    // Normalized before taking the lock: it may print a warning about a missing file.
    let key = key.map(stored_key).transpose()?;
    let secrets = password.as_ref().map(|_| password_store()).transpose()?;

    let (changed, config) = store::mutate_saved(|config| {
        let old_name = resolve(config, &name)?.0.to_string();
        let new_name = rename.unwrap_or_else(|| old_name.clone());
        let mut captured = Vec::new();
        if new_name != old_name {
            config.check_unique(&new_name, Some(&old_name))?;
            // Before the rename: these jump through a host that happens to have the new name.
            captured = (jump_dependents(config, &new_name).into_iter())
                .map(String::from)
                .collect();
        }
        let mut profile = config.profiles.remove(&old_name).expect("resolved above");
        let used_password = profile.uses_password();
        if let Some(host) = host {
            profile.host = host;
        }
        if let Some(user) = user {
            profile.user = non_empty(user);
        }
        if let Some(port) = port {
            profile.port = port.into_option();
        }
        if let Some(key) = key {
            profile.key = key;
        }
        if let Some(jump) = jump {
            profile.jump = non_empty(jump);
        }
        if let Some(forwards) = forwards {
            profile.forward = non_empty_list(forwards);
        }
        if let Some(note) = note {
            profile.note = non_empty(note);
        }
        if let Some(auth) = auth {
            profile.auth = Some(auth.into());
        }
        config.profiles.insert(new_name.clone(), profile);
        // Jumps that named the old name follow the rename; left alone, they would turn into
        // a host name and might reach another machine.
        let mut rewritten = Vec::new();
        if new_name != old_name {
            for (other, profile) in &mut config.profiles {
                let renamed = (profile.jump.as_deref())
                    .and_then(|jump| rename_in_jump(jump, &old_name, &new_name));
                if let Some(jump) = renamed {
                    profile.jump = Some(jump);
                    rewritten.push(other.clone());
                }
            }
        }
        Ok(Changed {
            old_name,
            new_name,
            used_password,
            rewritten,
            captured,
        })
    })?;
    let Changed {
        old_name,
        new_name,
        used_password,
        rewritten,
        captured,
    } = changed;

    if new_name == old_name {
        println!("updated '{old_name}'");
    } else {
        println!("updated '{old_name}', now named '{new_name}'");
    }
    for other in &rewritten {
        println!("updated the jump of '{other}' to use '{new_name}'");
    }
    warn_about_captured_jumps(&new_name, &captured);
    let saved = &config.profiles[&new_name];
    let id = saved.id.as_deref();
    if let (Some(secrets), Some(password)) = (&secrets, &password) {
        save_password(secrets, &new_name, id, password);
    }
    if used_password && !saved.uses_password() {
        forget_password(&new_name, id);
    }
    warn_about_jump(&config, &new_name);
    Ok(0)
}
