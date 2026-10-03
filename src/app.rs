use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::commands;

/// Parses the command line and runs the chosen command. Returns the process exit code.
pub fn run() -> Result<i32> {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        return commands::pick::run();
    };

    match command {
        Command::List { recent, json } => commands::list::run(recent, json),
        Command::Add(args) => commands::add::run(args),
        Command::Rm { name, yes } => commands::rm::run(name, yes),
        Command::Edit(args) => commands::edit::run(args),
        Command::Path => commands::path::run(),
        Command::Doctor => commands::doctor::run(),
        Command::Ping { name } => commands::ping::run(name),
        Command::Backup {
            file,
            no_keys,
            force,
        } => commands::backup::run(file, no_keys, force),
        Command::Restore {
            file,
            yes,
            overwrite_keys,
        } => commands::restore::run(file, yes, overwrite_keys),
        Command::Passwd { name, remove, show } => commands::passwd::run(name, remove, show),
        Command::Install {
            completion,
            no_completion,
        } => {
            let completion = match (completion, no_completion) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            };
            commands::install::run(completion)
        }
        Command::Uninstall { yes } => commands::uninstall::run(yes),
        Command::Update { check, yes } => commands::update::run(check, yes),
        Command::Completion { shell } => {
            crate::output::print(&crate::completion::script(shell))?;
            Ok(0)
        }
        Command::Complete => Ok(commands::complete::run()),
        // Both start with the profile name; clap guarantees it is present.
        Command::Connect { args } | Command::External(args) => {
            commands::connect::run(&args[0], &args[1..])
        }
    }
}
