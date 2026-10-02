//! What the picker shows and which row is highlighted, driven by terminal events. No
//! terminal here, so every key can be tested.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// What an event led to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Keep showing the table; the screen needs redrawing.
    Changed,
    /// Keep showing the table as it is.
    Unchanged,
    /// The user chose this row (an index into all rows, not just the visible ones).
    Chosen(usize),
    /// Esc.
    Cancelled,
    /// Ctrl+C.
    Interrupted,
}

pub struct PickerState<'a> {
    rows: &'a [Vec<String>],
    /// Each row's cells lowercased and joined by a newline, which cannot be typed into
    /// the filter, so a match never spans two cells.
    haystacks: Vec<String>,
    filter: String,
    /// Indices into `rows` that match the filter, in their original order.
    visible: Vec<usize>,
    /// Highlighted position in `visible`; `None` only when nothing matches.
    selected: Option<usize>,
    /// How many rows fit on screen: the distance PgUp and PgDn move.
    page: usize,
}

impl<'a> PickerState<'a> {
    pub fn new(rows: &'a [Vec<String>]) -> Self {
        let haystacks = rows
            .iter()
            .map(|row| row.join("\n").to_lowercase())
            .collect();
        let mut state = Self {
            rows,
            haystacks,
            filter: String::new(),
            visible: Vec::new(),
            selected: None,
            page: 1,
        };
        state.apply_filter();
        state
    }

    pub fn rows(&self) -> &'a [Vec<String>] {
        self.rows
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn visible(&self) -> &[usize] {
        &self.visible
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn set_page(&mut self, rows: usize) {
        self.page = rows.max(1);
    }

    /// Like [`handle`](Self::handle), for any terminal event: a resize needs a redraw,
    /// anything else that is not a key (focus, paste, mouse) changes nothing.
    pub fn on_event(&mut self, event: Event) -> Outcome {
        match event {
            Event::Key(key) => self.handle(key),
            Event::Resize(..) => Outcome::Changed,
            _ => Outcome::Unchanged,
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> Outcome {
        // Windows reports releases too; acting on them would move twice per press.
        if key.kind == KeyEventKind::Release {
            return Outcome::Unchanged;
        }
        let (selected_before, filter_before) = (self.selected, self.filter.clone());
        let modifiers = key.modifiers;
        let control = modifiers.contains(KeyModifiers::CONTROL);
        // AltGr arrives as Ctrl+Alt on Windows and types text (`@` on many layouts).
        let shortcut = modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && !modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('c') if control => return Outcome::Interrupted,
            KeyCode::Esc => return Outcome::Cancelled,
            KeyCode::Enter => {
                if let Some(row) = self.selected_row() {
                    return Outcome::Chosen(row);
                }
            }
            KeyCode::Up => self.step(false),
            KeyCode::Down => self.step(true),
            KeyCode::PageUp => self.jump(|at, page, _| at.saturating_sub(page)),
            KeyCode::PageDown => self.jump(|at, page, last| (at + page).min(last)),
            KeyCode::Home => self.jump(|_, _, _| 0),
            KeyCode::End => self.jump(|_, _, last| last),
            KeyCode::Backspace => {
                if self.filter.pop().is_some() {
                    self.apply_filter();
                }
            }
            KeyCode::Char(c) if !shortcut => {
                self.filter.push(c);
                self.apply_filter();
            }
            _ => {}
        }
        if self.selected == selected_before && self.filter == filter_before {
            Outcome::Unchanged
        } else {
            Outcome::Changed
        }
    }

    fn selected_row(&self) -> Option<usize> {
        self.selected.and_then(|at| self.visible.get(at)).copied()
    }

    /// One row up or down, wrapping around at either end.
    fn step(&mut self, down: bool) {
        let count = self.visible.len();
        self.selected = self.selected.map(|at| {
            if down {
                (at + 1) % count
            } else {
                (at + count - 1) % count
            }
        });
    }

    /// Moves to `to(current, page, last)`; does nothing when nothing matches.
    fn jump(&mut self, to: impl Fn(usize, usize, usize) -> usize) {
        let last = self.visible.len().saturating_sub(1);
        self.selected = self.selected.map(|at| to(at, self.page, last));
    }

    /// Recomputes the matching rows and highlights the first of them.
    fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        self.visible = (0..self.rows.len())
            .filter(|&i| self.haystacks[i].contains(&needle))
            .collect();
        self.selected = (!self.visible.is_empty()).then_some(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Vec<String>> {
        [
            ["kantor", "root@10.0.0.5", "office"],
            ["vps", "vps.example", "public"],
            ["db", "postgres@10.0.0.9", ""],
            ["Kandang", "farm.example", ""],
        ]
        .iter()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect())
        .collect()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// Presses the keys in order; returns what the last one led to.
    fn press(state: &mut PickerState, codes: &[KeyCode]) -> Outcome {
        let mut outcome = Outcome::Unchanged;
        for &code in codes {
            outcome = state.handle(key(code));
        }
        outcome
    }

    fn type_text(state: &mut PickerState, text: &str) {
        for c in text.chars() {
            assert_eq!(state.handle(key(KeyCode::Char(c))), Outcome::Changed);
        }
    }

    #[test]
    fn starts_on_the_first_row() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        assert_eq!(state.visible(), [0, 1, 2, 3]);
        assert_eq!(press(&mut state, &[KeyCode::Enter]), Outcome::Chosen(0));
    }

    #[test]
    fn arrows_move_and_wrap_around() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        assert_eq!(press(&mut state, &[KeyCode::Up]), Outcome::Changed);
        assert_eq!(state.selected(), Some(3));
        press(&mut state, &[KeyCode::Down, KeyCode::Down]);
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn home_end_and_pages() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        state.set_page(2);
        press(&mut state, &[KeyCode::PageDown]);
        assert_eq!(state.selected(), Some(2));
        press(&mut state, &[KeyCode::PageDown]);
        assert_eq!(state.selected(), Some(3), "stops at the last row");
        press(&mut state, &[KeyCode::PageUp]);
        assert_eq!(state.selected(), Some(1));
        press(&mut state, &[KeyCode::End]);
        assert_eq!(state.selected(), Some(3));
        press(&mut state, &[KeyCode::Home]);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn filter_ignores_case_and_searches_every_cell() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        type_text(&mut state, "KAN");
        assert_eq!(state.visible(), [0, 3]);
        assert_eq!(state.filter(), "KAN");

        let mut state = PickerState::new(&rows);
        type_text(&mut state, "10.0.0");
        assert_eq!(state.visible(), [0, 2], "matches the target");

        let mut state = PickerState::new(&rows);
        type_text(&mut state, "public");
        assert_eq!(state.visible(), [1], "matches the note");
    }

    #[test]
    fn a_match_never_spans_two_cells() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        // "vps" + "vps.example" would read "vpsvps" if the cells were simply joined.
        type_text(&mut state, "vpsvps");
        assert!(state.visible().is_empty());
    }

    #[test]
    fn chosen_is_the_index_among_all_rows() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        type_text(&mut state, "kan");
        assert_eq!(
            press(&mut state, &[KeyCode::Down, KeyCode::Enter]),
            Outcome::Chosen(3)
        );
    }

    #[test]
    fn changing_the_filter_goes_back_to_the_first_match() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        press(&mut state, &[KeyCode::End]);
        type_text(&mut state, "a");
        assert_eq!(state.selected(), Some(0));
        press(&mut state, &[KeyCode::Down]);
        press(&mut state, &[KeyCode::Backspace]);
        assert_eq!(state.visible(), [0, 1, 2, 3]);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn backspace_on_an_empty_filter_keeps_the_selection() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        press(&mut state, &[KeyCode::Down, KeyCode::Backspace]);
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn nothing_to_choose_when_nothing_matches() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        type_text(&mut state, "zzz");
        assert_eq!(state.selected(), None);
        let keys = [
            KeyCode::Down,
            KeyCode::End,
            KeyCode::PageDown,
            KeyCode::Enter,
        ];
        assert_eq!(press(&mut state, &keys), Outcome::Unchanged);
        assert_eq!(state.selected(), None);

        let empty = Vec::new();
        let mut state = PickerState::new(&empty);
        assert_eq!(
            press(&mut state, &[KeyCode::Up, KeyCode::Enter]),
            Outcome::Unchanged
        );
    }

    #[test]
    fn esc_cancels_and_ctrl_c_interrupts() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        assert_eq!(press(&mut state, &[KeyCode::Esc]), Outcome::Cancelled);
        let ctrl_c = with(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(state.handle(ctrl_c), Outcome::Interrupted);
    }

    #[test]
    fn shortcuts_do_not_type_but_altgr_does() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        state.handle(with(KeyCode::Char('k'), KeyModifiers::CONTROL));
        state.handle(with(KeyCode::Char('k'), KeyModifiers::ALT));
        assert_eq!(state.filter(), "");

        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        state.handle(with(KeyCode::Char('@'), altgr));
        state.handle(with(KeyCode::Char('R'), KeyModifiers::SHIFT));
        assert_eq!(state.filter(), "@R");
    }

    #[test]
    fn key_releases_are_ignored() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        let mut release = key(KeyCode::Down);
        release.kind = KeyEventKind::Release;
        assert_eq!(state.handle(release), Outcome::Unchanged);
        assert_eq!(state.selected(), Some(0));

        let mut repeat = key(KeyCode::Down);
        repeat.kind = KeyEventKind::Repeat;
        assert_eq!(state.handle(repeat), Outcome::Changed);
        assert_eq!(state.selected(), Some(1), "holding a key down repeats it");
    }

    #[test]
    fn only_real_changes_ask_for_a_redraw() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        let unchanged = [
            KeyCode::Home,      // already on the first row
            KeyCode::PageUp,    // likewise
            KeyCode::Backspace, // nothing to delete
            KeyCode::F(5),      // not a picker key
        ];
        for code in unchanged {
            assert_eq!(state.handle(key(code)), Outcome::Unchanged, "{code:?}");
        }
        let ctrl_k = with(KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(state.handle(ctrl_k), Outcome::Unchanged);
        assert_eq!(state.handle(key(KeyCode::End)), Outcome::Changed);
        assert_eq!(state.handle(key(KeyCode::Char('x'))), Outcome::Changed);
        assert_eq!(state.handle(key(KeyCode::Backspace)), Outcome::Changed);

        // a single row: wrapping around lands on the same row
        let one = vec![vec!["vps".to_string()]];
        let mut state = PickerState::new(&one);
        assert_eq!(press(&mut state, &[KeyCode::Down]), Outcome::Unchanged);
    }

    #[test]
    fn other_terminal_events() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        assert_eq!(state.on_event(Event::Resize(80, 24)), Outcome::Changed);
        assert_eq!(state.on_event(Event::FocusGained), Outcome::Unchanged);
        let down = Event::Key(key(KeyCode::Down));
        assert_eq!(state.on_event(down), Outcome::Changed);
        assert_eq!(state.selected(), Some(1));
    }
}
