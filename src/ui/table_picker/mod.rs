//! A full-screen table to choose a row from (ratatui). Draws on stderr, like the inquire
//! prompts, so stdout stays clean for scripts.

mod state;
mod view;

use std::io::{self, BufWriter};
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event;
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};
use ratatui::widgets::TableState;

use self::state::{Outcome, PickerState};
use self::view::View;
use crate::error::Abort;

/// Shows `rows` under `header` until the user chooses one; returns its index. Fails with
/// [`Abort`] on Esc or Ctrl+C. The terminal is back to normal when this returns, so ssh
/// or another prompt can use it right away.
pub fn pick_row(title: &str, header: &[&str], rows: &[Vec<String>]) -> Result<usize> {
    let view = View::new(title, header, rows);
    let mut state = PickerState::new(rows);
    let mut scroll = TableState::default();

    terminal::enable_raw_mode().context("cannot switch the terminal to raw mode")?;
    // Declared before the terminal, so dropped after it: from here on, an early return or
    // a panic still restores the terminal.
    let _restore = Restore;
    execute!(io::stderr(), EnterAlternateScreen).context("cannot open the full-screen view")?;
    // stderr is unbuffered: without this, every cursor move and color change of a frame
    // is its own write, and each one is slow on a Windows console.
    let backend = CrosstermBackend::new(BufWriter::new(io::stderr()));
    let mut terminal = Terminal::new(backend).context("cannot draw on the terminal")?;

    loop {
        terminal.draw(|frame| view.draw(frame, &mut state, &mut scroll))?;
        // Handle everything already waiting before drawing again, so holding a key or
        // typing fast never leaves the screen behind. Events that change nothing (such
        // as key releases) do not cause a redraw.
        let mut redraw = false;
        while !redraw || event::poll(Duration::ZERO).context(KEYBOARD)? {
            match state.on_event(event::read().context(KEYBOARD)?) {
                Outcome::Changed => redraw = true,
                Outcome::Unchanged => {}
                Outcome::Chosen(index) => return Ok(index),
                Outcome::Cancelled => return Err(Abort::Cancelled.into()),
                Outcome::Interrupted => return Err(Abort::Interrupted.into()),
            }
        }
    }
}

const KEYBOARD: &str = "cannot read the keyboard";

/// Leaves the alternate screen and raw mode and shows the cursor: the state the shell,
/// ssh (keys typed into the session) and the inquire prompts expect.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        // Nothing useful to do on failure; the user's terminal gets the best attempt.
        let _ = execute!(io::stderr(), LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}
