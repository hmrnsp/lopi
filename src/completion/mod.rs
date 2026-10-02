//! Shell completion scripts. The scripts are thin: subcommand names are filled in from
//! the clap definition when printed, and profile names come from `lopi __complete`
//! each time Tab is pressed.

use clap::{CommandFactory, ValueEnum};

use crate::cli::{Cli, Shell};

const BASH: &str = include_str!("lopi.bash");
const ZSH: &str = include_str!("lopi.zsh");
const POWERSHELL: &str = include_str!("lopi.ps1");

pub fn script(shell: Shell) -> String {
    let subcommands = visible_subcommands();
    let shells = shell_names();
    match shell {
        Shell::Bash => fill(BASH, &subcommands.join(" "), &shells.join(" ")),
        Shell::Zsh => fill(ZSH, &subcommands.join(" "), &shells.join(" ")),
        Shell::Powershell => fill(POWERSHELL, &quoted(&subcommands), &quoted(&shells)),
    }
}

/// Subcommands a user may type, from the CLI definition (hidden ones excluded).
pub fn visible_subcommands() -> Vec<String> {
    Cli::command()
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .map(|sub| sub.get_name().to_string())
        .collect()
}

fn shell_names() -> Vec<String> {
    Shell::value_variants()
        .iter()
        .filter_map(|shell| shell.to_possible_value())
        .map(|value| value.get_name().to_string())
        .collect()
}

/// Fills the placeholders. Line endings are forced to `\n`: a Windows checkout may turn
/// the templates into CRLF, which bash and zsh reject (`$'\r': command not found`).
fn fill(template: &str, subcommands: &str, shells: &str) -> String {
    template
        .replace("\r\n", "\n")
        .replace("@SUBCOMMANDS@", subcommands)
        .replace("@SHELLS@", shells)
}

/// `'a', 'b'` for a PowerShell array literal. Names never contain quotes.
fn quoted(words: &[String]) -> String {
    words
        .iter()
        .map(|word| format!("'{word}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_list_every_visible_subcommand_and_no_placeholder() {
        let subcommands = visible_subcommands();
        assert!(subcommands.contains(&"list".to_string()));
        assert!(subcommands.contains(&"completion".to_string()));
        assert!(!subcommands.iter().any(|s| s.starts_with("__")));

        for shell in Shell::value_variants() {
            let text = script(*shell);
            assert!(
                !text.contains("@SUBCOMMANDS@") && !text.contains("@SHELLS@"),
                "{shell:?} still has a placeholder"
            );
            for sub in &subcommands {
                assert!(text.contains(sub.as_str()), "{shell:?} misses {sub}");
            }
            assert!(text.contains("lopi __complete"), "{shell:?}");
        }
    }

    #[test]
    fn crlf_templates_come_out_as_lf() {
        let text = fill("a\r\nb @SHELLS@\r\n", "", "x");
        assert_eq!(text, "a\nb x\n");
        for shell in Shell::value_variants() {
            assert!(!script(*shell).contains('\r'), "{shell:?}");
        }
    }

    #[test]
    fn powershell_lists_are_quoted() {
        assert!(script(Shell::Powershell).contains("'bash', 'zsh', 'powershell'"));
    }
}
