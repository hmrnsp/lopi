use std::env;
use std::io::{self, BufRead, IsTerminal, Write};

/// Started by double-clicking the exe in Explorer: no arguments, and Windows created a
/// console window just for this process (no shell shares it). That window closes the
/// moment the program exits, so messages must be held on screen. A hidden
/// `Start-Process` also gets its own console, hence the no-arguments condition.
pub fn launched_by_double_click() -> bool {
    env::args_os().len() == 1 && owns_console()
}

#[cfg(windows)]
fn owns_console() -> bool {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;
    let mut ids = [0u32; 2];
    // SAFETY: `ids` has room for the 2 entries passed as its length.
    let count = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) };
    count == 1
}

#[cfg(not(windows))]
fn owns_console() -> bool {
    false
}

/// Keeps a double-clicked window open until the user has read it.
pub fn wait_for_enter() {
    eprint!("\nPress Enter to close this window...");
    let _ = io::stderr().flush();
    let _ = io::stdin().lock().read_line(&mut String::new());
}

/// Prompts need a keyboard (stdin) and a screen to draw on (stderr, where they render).
pub fn interactive() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

/// Why prompts are unavailable, when there is a known fix. Git Bash's default terminal
/// (mintty) connects programs through pipes instead of a console, so they cannot tell it
/// is a terminal.
pub fn not_interactive_hint() -> Option<&'static str> {
    let mintty = env::var_os("TERM_PROGRAM").is_some_and(|t| t == "mintty");
    let git_bash = env::var_os("MSYSTEM").is_some();
    (cfg!(windows) && (mintty || git_bash)).then_some(
        "interactive prompts do not work in Git Bash's default window (mintty); \
         run `winpty lopi`, or use Windows Terminal",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_are_never_a_double_click() {
        // The test binary runs with arguments and shares cargo's console (or has none).
        assert!(!launched_by_double_click());
    }
}
