use anyhow::Result;
use clap::CommandFactory;

use super::{add, connect};
use crate::cli::{AddArgs, Cli};
use crate::config::store;
use crate::install;
use crate::state;
use crate::ui::picker::pick_profile;
use crate::ui::prompt::{Prompter, TerminalPrompter};
use crate::ui::tty;

/// `lopi` with no arguments: choose a profile and connect. Without a terminal there
/// is nothing to choose with, so print help instead (exit 2, like a usage error).
pub fn run() -> Result<i32> {
    if !tty::interactive() {
        Cli::command().print_help()?;
        if let Some(hint) = tty::not_interactive_hint() {
            eprintln!("\nlopi: note: {hint}");
        }
        return Ok(2);
    }
    // A friend double-clicked the exe they were sent: offer to set it up first.
    if tty::launched_by_double_click()
        && !install::running_installed_copy()
        && TerminalPrompter.confirm("Install lopi so you can run it from any terminal?", true)?
    {
        return super::install::run(None);
    }
    let config = store::load()?;
    if config.profiles.is_empty() {
        if TerminalPrompter.confirm("No profiles yet. Add one now?", true)? {
            return add::run(AddArgs::default());
        }
        return Ok(0);
    }
    let name = pick_profile(&config, &state::load(), &mut TerminalPrompter, "Connect to")?;
    connect::connect(&config, name, &[])
}
