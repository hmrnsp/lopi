//! The full-screen view (alternate screen, raw mode) on stderr, shared by the table picker
//! and `passwd --show`.

use std::io;

use anyhow::{Context, Result};
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};

/// While alive, the terminal shows the alternate screen in raw mode. Dropping it (also on
/// an early return or a panic) brings the terminal back to normal.
pub struct FullScreen;

impl FullScreen {
    pub fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("cannot switch the terminal to raw mode")?;
        // Created before entering the alternate screen, so a failure below still
        // restores the terminal.
        let screen = Self;
        execute!(io::stderr(), EnterAlternateScreen).context("cannot open the full-screen view")?;
        Ok(screen)
    }
}

/// Leaves the alternate screen and raw mode and shows the cursor: the state the shell,
/// ssh (keys typed into the session) and the inquire prompts expect.
impl Drop for FullScreen {
    fn drop(&mut self) {
        // Nothing useful to do on failure; the user's terminal gets the best attempt.
        let _ = execute!(io::stderr(), LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}
