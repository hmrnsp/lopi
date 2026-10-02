use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "lopi",
    version,
    about = "Open SSH connections from saved profiles with one short command",
    after_help = "Connect:\n  lopi <profile> [ssh options...] [-- remote command]\n  A unique prefix of the profile name also works, e.g. `lopi kan` for `kantor`."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List all profiles
    List {
        /// Most recently used first
        #[arg(short, long)]
        recent: bool,
    },

    /// Add a new profile (asks for what is missing when run in a terminal)
    Add(AddArgs),

    /// Remove a profile (exact name only; without a name, choose from a list)
    Rm {
        /// Exact profile name
        name: Option<String>,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },

    /// Change a profile: with flags, only those fields; without, a guided edit
    Edit(EditArgs),

    /// Print the path of the profiles file
    Path,

    /// Check ssh, the profiles file, key files and saved passwords
    Doctor,

    /// Save profiles, saved passwords and key files to one passphrase-encrypted file
    Backup {
        /// Where to write it (default: lopi-backup-YYYYMMDD.age in this folder)
        file: Option<PathBuf>,
        /// Leave private key files out
        #[arg(long)]
        no_keys: bool,
        /// Replace FILE without asking
        #[arg(long)]
        force: bool,
    },

    /// Restore a backup file or a profiles file; without FILE, choose an automatic snapshot
    Restore {
        /// A `lopi backup` file (.age) or a profiles file
        file: Option<PathBuf>,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
        /// Replace existing key files that differ from the backup's
        #[arg(long)]
        overwrite_keys: bool,
    },

    /// Save or change a profile's password in the system credential store
    Passwd {
        /// Exact profile name (without it, choose from a list)
        name: Option<String>,
        /// Delete the saved password and stop using password login
        #[arg(long)]
        remove: bool,
    },

    /// Install this exe for the current user and put it on PATH (no admin needed)
    Install {
        /// Also enable Tab completion in PowerShell, without asking
        #[arg(long, conflicts_with = "no_completion")]
        completion: bool,
        /// Do not enable Tab completion, without asking
        #[arg(long)]
        no_completion: bool,
    },

    /// Remove the installed exe and its PATH entry (profiles are kept)
    Uninstall {
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },

    /// Print a shell completion script (see --help for how to install it)
    #[command(after_long_help = COMPLETION_HELP)]
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Profile names for shell completion (used by the completion scripts)
    #[command(name = "__complete", hide = true)]
    Complete,

    /// Connect to a profile (use when the name clashes with a subcommand)
    Connect {
        /// Profile name or unique prefix, then extra ssh options, then `--` and a remote command.
        /// One argument on purpose: as a separate positional, clap would swallow a `--` that
        /// directly follows the name.
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "PROFILE [SSH_ARGS]..."
        )]
        args: Vec<OsString>,
    },

    #[command(external_subcommand)]
    External(Vec<OsString>),
}

#[derive(Debug, Default, Args)]
pub struct AddArgs {
    /// Profile name
    pub name: Option<String>,
    /// Destination as [user@]host
    pub target: Option<String>,
    /// SSH port
    #[arg(short, long, value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,
    /// Private key file, e.g. ~/.ssh/id_ed25519
    #[arg(short = 'i', long)]
    pub key: Option<String>,
    /// Jump host(s) for ssh -J: a profile name or [user@]host[:port], comma-separated
    #[arg(short = 'J', long)]
    pub jump: Option<String>,
    /// Port forward opened on every connection: L:8080:localhost:80, R:..., D:1080
    /// (repeatable)
    #[arg(short = 'f', long = "forward", value_name = "KIND:SPEC")]
    pub forwards: Vec<String>,
    /// Free-form note
    #[arg(short, long)]
    pub note: Option<String>,
    /// Log in with a password: you are asked for it (masked), and it is saved in the
    /// system credential store, never in the profiles file
    #[arg(long)]
    pub password: bool,
}

/// Only the given fields change. Pass "" to clear user, port, key, jump, forwards or note.
#[derive(Debug, Default, Args)]
pub struct EditArgs {
    /// Exact profile name (without it, choose from a list)
    pub name: Option<String>,
    #[arg(long)]
    pub host: Option<String>,
    /// Login user ("" clears it)
    #[arg(short, long)]
    pub user: Option<String>,
    /// SSH port ("" clears it)
    #[arg(short, long, value_parser = parse_port_arg)]
    pub port: Option<PortArg>,
    /// Private key file ("" clears it)
    #[arg(short = 'i', long)]
    pub key: Option<String>,
    /// Jump host(s) for ssh -J ("" clears it)
    #[arg(short = 'J', long)]
    pub jump: Option<String>,
    /// Port forwards; replaces the saved list (repeatable, "" clears it)
    #[arg(short = 'f', long = "forward", value_name = "KIND:SPEC")]
    pub forwards: Option<Vec<String>>,
    /// Free-form note ("" clears it)
    #[arg(short, long)]
    pub note: Option<String>,
    /// New profile name
    #[arg(long)]
    pub rename: Option<String>,
    /// How to log in; `password` asks for the password to save
    #[arg(long, value_enum)]
    pub auth: Option<AuthArg>,
}

/// `edit --auth` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AuthArg {
    Key,
    Password,
    Agent,
}

impl From<AuthArg> for crate::config::Auth {
    fn from(arg: AuthArg) -> Self {
        match arg {
            AuthArg::Key => Self::Key,
            AuthArg::Password => Self::Password,
            AuthArg::Agent => Self::Agent,
        }
    }
}

/// Shells with completion scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Powershell,
}

const COMPLETION_HELP: &str = "\
Install (completes profile names and subcommands; cmd.exe is not supported):
  bash        add to ~/.bashrc:   eval \"$(lopi completion bash)\"
  zsh         add to ~/.zshrc after compinit:   eval \"$(lopi completion zsh)\"
  PowerShell  add to $PROFILE:    lopi completion powershell | Out-String | Invoke-Expression";

/// `edit --port` value: a port, or `""` to remove the port from the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortArg {
    Clear,
    Set(u16),
}

impl PortArg {
    pub fn into_option(self) -> Option<u16> {
        match self {
            Self::Clear => None,
            Self::Set(port) => Some(port),
        }
    }
}

fn parse_port_arg(value: &str) -> Result<PortArg, String> {
    if value.is_empty() {
        return Ok(PortArg::Clear);
    }
    match value.parse::<u16>() {
        Ok(port) if port > 0 => Ok(PortArg::Set(port)),
        _ => Err(format!(
            "'{value}' is not a port; use 1-65535, or \"\" to remove the port"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_arg() {
        assert_eq!(parse_port_arg(""), Ok(PortArg::Clear));
        assert_eq!(parse_port_arg("2222"), Ok(PortArg::Set(2222)));
        for bad in ["0", "65536", "-1", "x", " 22"] {
            assert!(parse_port_arg(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
