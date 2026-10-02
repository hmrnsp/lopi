//! Shell completion scripts. The scripts are thin: subcommand names are filled in from
//! the clap definition when printed, and profile names come from `lopi __complete`
//! each time Tab is pressed.

use clap::{CommandFactory, ValueEnum};

use crate::cli::{Cli, Shell};

const BASH: &str = include_str!("lopi.bash");
const ZSH: &str = include_str!("lopi.zsh");
const POWERSHELL: &str = include_str!("lopi.ps1");

/// Subcommands whose first argument is an existing profile name, completed with
/// `lopi __complete`. Add new ones here (not `add`: its name is a new one).
pub const PROFILE_SUBCOMMANDS: &[&str] = &["connect", "edit", "passwd", "rm"];

pub fn script(shell: Shell) -> String {
    let subcommands = visible_subcommands();
    let shells = shell_names();
    let takes_profile: Vec<String> = PROFILE_SUBCOMMANDS.iter().map(|s| s.to_string()).collect();
    match shell {
        Shell::Bash => fill(
            BASH,
            &subcommands.join(" "),
            &shells.join(" "),
            &takes_profile.join(" "),
        ),
        Shell::Zsh => fill(
            ZSH,
            &subcommands.join(" "),
            &shells.join(" "),
            &takes_profile.join("|"),
        ),
        Shell::Powershell => fill(
            POWERSHELL,
            &quoted(&subcommands),
            &quoted(&shells),
            &quoted(&takes_profile),
        ),
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
fn fill(template: &str, subcommands: &str, shells: &str, takes_profile: &str) -> String {
    template
        .replace("\r\n", "\n")
        .replace("@PROFILE_SUBCOMMANDS@", takes_profile)
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
            // Placeholders are `@UPPER_CASE@`; PowerShell's own `@(` is fine.
            let placeholder = text
                .split('@')
                .skip(1)
                .any(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase()));
            assert!(!placeholder, "{shell:?} still has a placeholder");
            for sub in &subcommands {
                assert!(text.contains(sub.as_str()), "{shell:?} misses {sub}");
            }
            assert!(text.contains("lopi __complete"), "{shell:?}");
        }
    }

    #[test]
    fn crlf_templates_come_out_as_lf() {
        let text = fill("a\r\nb @SHELLS@\r\n", "", "x", "");
        assert_eq!(text, "a\nb x\n");
        for shell in Shell::value_variants() {
            assert!(!script(*shell).contains('\r'), "{shell:?}");
        }
    }

    #[test]
    fn profile_names_are_completed_after_the_right_subcommands() {
        let visible = visible_subcommands();
        for sub in PROFILE_SUBCOMMANDS {
            assert!(
                visible.iter().any(|v| v == sub),
                "{sub} is not a subcommand"
            );
        }
        assert!(
            !PROFILE_SUBCOMMANDS.contains(&"add"),
            "add takes a new name"
        );
        let cases = [
            (Shell::Bash, r#"" connect edit passwd rm " == *" $sub "*"#),
            (Shell::Zsh, "== (connect|edit|passwd|rm)"),
            (
                Shell::Powershell,
                "@('connect', 'edit', 'passwd', 'rm') -contains $words[1]",
            ),
        ];
        for (shell, expected) in cases {
            assert!(script(shell).contains(expected), "{shell:?}");
        }
    }

    #[test]
    fn powershell_lists_are_quoted() {
        assert!(script(Shell::Powershell).contains("'bash', 'zsh', 'powershell'"));
    }
}
