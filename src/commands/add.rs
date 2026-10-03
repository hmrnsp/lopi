use anyhow::{Context, Result};

use super::{
    ask_new_password, non_empty, non_empty_list, password_store, require_terminal, save_password,
    stored_key, warn_about_captured_jumps, warn_about_jump,
};
use crate::cli::AddArgs;
use crate::config::model::{split_target, validate_name};
use crate::config::store::{self, StorePaths};
use crate::config::{Auth, Config, Profile};
use crate::ssh::args::destination;
use crate::ssh::jump::jump_dependents;
use crate::ui::prompt::TerminalPrompter;
use crate::ui::wizard::{self, AddPlan};

/// With a name and a target, adds right away (asking only for the password with
/// `--password`). Otherwise (in a terminal) asks for what is missing; flags already given
/// are not asked again.
pub fn run(args: AddArgs) -> Result<i32> {
    let plan = if args.name.is_some() && args.target.is_some() {
        let password = if args.password {
            password_store()?;
            Some(ask_new_password(
                args.target.as_deref().unwrap_or_default(),
            )?)
        } else {
            None
        };
        AddPlan { args, password }
    } else {
        require_terminal("lopi add <name> [user@]host [options]")?;
        if args.password {
            password_store()?;
        }
        let config = store::load()?;
        let keys = wizard::local_keys(dirs::home_dir().as_deref());
        wizard::add(&config, args, &mut TerminalPrompter, keys)?
    };
    save(plan)
}

fn save(plan: AddPlan) -> Result<i32> {
    let AddPlan { args, password } = plan;
    let AddArgs {
        name,
        target,
        port,
        key,
        jump,
        forwards,
        note,
        password: password_login,
    } = args;
    let name = name.context("internal error: add without a name")?;
    let target = target.context("internal error: add without a target")?;
    validate_name(&name)?;
    let (user, host) = split_target(&target);
    let profile = Profile {
        user: user.map(str::to_string),
        port,
        key: key.map(stored_key).transpose()?.flatten(),
        jump: jump.and_then(non_empty),
        forward: non_empty_list(forwards),
        auth: password_login.then_some(Auth::Password),
        note: note.and_then(non_empty),
        ..Profile::new(host)
    };
    // Checked here too (mutate validates again) so the error names the bad field directly.
    profile.validate()?;
    let shown = destination(&profile);
    // Checked before saving, so a profile is never saved with a password that cannot be.
    let secrets = password.as_ref().map(|_| password_store()).transpose()?;

    let config = insert_profile(&StorePaths::from_env()?, &name, profile)?;
    println!("added '{name}' ({shown}); connect with `lopi {name}`");
    if let (Some(secrets), Some(password)) = (&secrets, &password) {
        let id = config.profiles.get(&name).and_then(|p| p.id.as_deref());
        save_password(secrets, &name, id, password);
    }
    warn_about_jump(&config, &name);
    warn_about_captured_jumps(&name, &jump_dependents(&config, &name));
    Ok(0)
}

/// Saves `profile` as `name`. Returns the config as saved: only there does the new
/// profile have its id, which its password is stored under.
fn insert_profile(store: &StorePaths, name: &str, profile: Profile) -> Result<Config> {
    let ((), config) = store::mutate_saved_in(store, |config| {
        config.check_unique(name, None)?;
        config.profiles.insert(name.to_string(), profile);
        Ok(())
    })?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::SecretStore;
    use crate::secrets::memory::MemoryStore;

    fn temp_store() -> (tempfile::TempDir, StorePaths) {
        let dir = tempfile::tempdir().unwrap();
        let store = StorePaths {
            file: dir.path().join("profiles.toml"),
            backups: dir.path().join("backups"),
        };
        (dir, store)
    }

    #[test]
    fn password_is_saved_under_the_new_profiles_id() {
        let (_dir, store) = temp_store();
        let config = insert_profile(&store, "vps", Profile::new("h")).unwrap();
        let id = config.profiles["vps"]
            .id
            .as_deref()
            .expect("a new profile gets an id");
        let on_disk = store::load_from(&store.file).unwrap();
        assert_eq!(on_disk.profiles["vps"].id.as_deref(), Some(id));

        let secrets = MemoryStore::default();
        save_password(&secrets, "vps", Some(id), "s3cret");
        let saved = secrets.get(id).unwrap();
        assert_eq!(saved.as_deref().map(String::as_str), Some("s3cret"));
    }

    #[test]
    fn names_differing_only_in_case_are_refused() {
        let (_dir, store) = temp_store();
        insert_profile(&store, "vps", Profile::new("h")).unwrap();
        let err = insert_profile(&store, "VPS", Profile::new("h")).unwrap_err();
        assert!(format!("{err:#}").contains("vps"), "{err:#}");
        assert_eq!(store::load_from(&store.file).unwrap().profiles.len(), 1);
    }
}
