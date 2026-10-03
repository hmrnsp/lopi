use anyhow::Result;

use super::{check_no_jump_dependents, forget_password, require_terminal};
use crate::config::store;
use crate::error::Abort;
use crate::resolve::resolve;
use crate::state;
use crate::ui;
use crate::ui::picker::pick_profile;
use crate::ui::prompt::{Prompter, TerminalPrompter};

/// Removes a profile after confirmation (`-y` skips it). Exact names only; without a name,
/// the user chooses from a list.
pub fn run(name: Option<String>, yes: bool) -> Result<i32> {
    let config = store::load()?;
    // A wrong name is reported before anything about terminals.
    let (name, profile) = match &name {
        Some(name) => resolve(&config, name)?,
        None => {
            require_terminal("lopi rm <name> [-y]")?;
            let name = pick_profile(&config, &state::load(), &mut TerminalPrompter, "Remove")?;
            (name, &config.profiles[name])
        }
    };
    // Checked before asking, so nobody confirms a removal that is then refused.
    check_no_jump_dependents(&config, name)?;
    if !yes {
        require_terminal("-y to remove without confirmation")?;
        let question = format!("Remove profile '{name}' ({})?", ui::target(profile));
        if !TerminalPrompter.confirm(&question, false)? {
            return Err(Abort::Cancelled.into());
        }
    }
    let removed = store::mutate(|config| {
        // Exact name again: the file may have changed since it was loaded above.
        resolve(config, name)?;
        check_no_jump_dependents(config, name)?;
        Ok(config.profiles.remove(name))
    })?;
    println!("removed '{name}'");
    // Only touch the credential store when there is something to delete there.
    if let Some(profile) = removed.filter(|profile| profile.uses_password()) {
        forget_password(name, profile.id.as_deref());
    }
    Ok(0)
}
