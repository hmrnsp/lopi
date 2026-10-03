//! Shows a saved password on the alternate screen until a key hides it, so it never stays
//! in the terminal's scrollback.

use std::io::{self, Write};

use anyhow::{Context, Result};
use ratatui::crossterm::cursor::MoveTo;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{Clear, ClearType};
use zeroize::Zeroizing;

use crate::error::Abort;
use crate::ui::screen::FullScreen;

/// Shows `secret` under `title` until Enter, Esc or `q`; fails with [`Abort::Interrupted`]
/// on Ctrl+C. The screen is cleared and the terminal back to normal when this returns.
pub fn show_secret(title: &str, secret: &str) -> Result<()> {
    let shown = printable(secret);
    let _screen = FullScreen::enter()?;
    loop {
        draw(title, &shown)?;
        // Redrawn after every other event, so a resized window shows it again.
        match hides(&event::read().context("cannot read the keyboard")?) {
            Some(Hide::Done) => return Ok(()),
            Some(Hide::Interrupted) => return Err(Abort::Interrupted.into()),
            None => {}
        }
    }
}

fn draw(title: &str, shown: &str) -> Result<()> {
    let mut err = io::stderr().lock();
    execute!(err, Clear(ClearType::All), MoveTo(0, 0)).context("cannot draw on the terminal")?;
    // Raw mode: a line ends with \r\n.
    write!(err, "{title}\r\n\r\n  ")?;
    err.write_all(shown.as_bytes())?;
    write!(err, "\r\n\r\nPress Enter to hide.")?;
    err.flush()?;
    Ok(())
}

/// How the view was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hide {
    /// Enter, Esc or `q`.
    Done,
    /// Ctrl+C.
    Interrupted,
}

fn hides(event: &Event) -> Option<Hide> {
    let Event::Key(key) = event else { return None };
    // Windows also reports key releases.
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Hide::Interrupted)
        }
        KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => Some(Hide::Done),
        _ => None,
    }
}

/// The secret with control characters escaped, so whatever is in the credential store
/// cannot send escape sequences to the terminal.
fn printable(secret: &str) -> Zeroizing<String> {
    let mut shown = Zeroizing::new(String::with_capacity(secret.len()));
    for c in secret.chars() {
        if c.is_control() {
            shown.extend(c.escape_default());
        } else {
            shown.push(c);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyEvent;

    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn enter_esc_and_q_hide_ctrl_c_interrupts() {
        for code in [KeyCode::Enter, KeyCode::Esc, KeyCode::Char('q')] {
            assert_eq!(hides(&key(code, KeyModifiers::NONE)), Some(Hide::Done));
        }
        let ctrl_c = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(hides(&ctrl_c), Some(Hide::Interrupted));
    }

    #[test]
    fn other_keys_releases_and_resizes_keep_it_shown() {
        assert_eq!(hides(&key(KeyCode::Char('c'), KeyModifiers::NONE)), None);
        assert_eq!(hides(&key(KeyCode::Char(' '), KeyModifiers::NONE)), None);
        assert_eq!(hides(&Event::Resize(80, 24)), None);
        let mut release = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(hides(&Event::Key(release)), None);
    }

    #[test]
    fn control_characters_are_escaped() {
        assert_eq!(printable("s3cret ü!").as_str(), "s3cret ü!");
        assert_eq!(printable("a\x1b[2Jb\n").as_str(), "a\\u{1b}[2Jb\\n");
    }
}
