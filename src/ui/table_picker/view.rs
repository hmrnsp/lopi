//! Draws the picker: a rounded box with the title, a search line and the table, and a key
//! help line under it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, HighlightSpacing, Padding, Row, Table, TableState};

use super::state::PickerState;

/// The one accent color: title, search mark, highlighted row.
const ACCENT: Color = Color::Cyan;
/// Borders, the header line and hints: present but quiet.
const MUTED: Color = Color::DarkGray;
const PLACEHOLDER: &str = "type to filter";
/// Key, then what it does.
const HELP: [(&str, &str); 4] = [
    ("↑↓", "move"),
    ("type", "to filter"),
    ("enter", "choose"),
    ("esc", "cancel"),
];
/// The header line and the rule under it.
const HEADER_HEIGHT: u16 = 2;

pub struct View<'a> {
    title: &'a str,
    header: &'a [&'a str],
    /// Fitted to every row, not just the visible ones, so columns stay put while filtering.
    widths: Vec<Constraint>,
}

impl<'a> View<'a> {
    pub fn new(title: &'a str, header: &'a [&'a str], rows: &[Vec<String>]) -> Self {
        Self {
            title,
            header,
            widths: column_widths(header, rows),
        }
    }

    /// `scroll` keeps the table's scroll position from one frame to the next.
    pub fn draw(&self, frame: &mut Frame, state: &mut PickerState, scroll: &mut TableState) {
        let [box_area, help_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let frame_box = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(MUTED))
            .title_top(Line::from(format!(" {} ", self.title)).bold().fg(ACCENT))
            .title_top(
                Line::from(format!(" {} ", count(state)))
                    .fg(MUTED)
                    .right_aligned(),
            )
            .padding(Padding::horizontal(1));
        let inner = frame_box.inner(box_area);
        frame.render_widget(frame_box, box_area);
        frame.render_widget(help_line(), help_area);

        let [search_area, _, table_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(inner);
        self.draw_search(frame, state, search_area);
        state.set_page(usize::from(table_area.height.saturating_sub(HEADER_HEIGHT)));
        if state.visible().is_empty() {
            let message = format!("no profile matches '{}'", state.filter());
            frame.render_widget(Line::from(message).fg(MUTED), table_area);
            return;
        }
        self.draw_table(frame, state, scroll, table_area);
    }

    /// `/ filter`, with the cursor where typing goes; a hint while the filter is empty.
    fn draw_search(&self, frame: &mut Frame, state: &PickerState, area: Rect) {
        let mark = Span::from("/ ").fg(ACCENT).bold();
        let cursor_x = area
            .x
            .saturating_add(to_u16(mark.width() + Span::from(state.filter()).width()))
            .min(area.right().saturating_sub(1));
        let text = if state.filter().is_empty() {
            Span::from(PLACEHOLDER).fg(MUTED)
        } else {
            Span::from(state.filter())
        };
        frame.render_widget(Line::from(vec![mark, text]), area);
        frame.set_cursor_position(Position::new(cursor_x, area.y));
    }

    fn draw_table(
        &self,
        frame: &mut Frame,
        state: &PickerState,
        scroll: &mut TableState,
        area: Rect,
    ) {
        let all = state.rows();
        let rows = state
            .visible()
            .iter()
            .map(|&i| Row::new(all[i].iter().map(String::as_str)));
        let header = Row::new(self.header.iter().copied())
            .bold()
            .bottom_margin(HEADER_HEIGHT - 1);
        let table = Table::new(rows, self.widths.iter().copied())
            .header(header)
            .column_spacing(2)
            .row_highlight_style(Style::new().bg(ACCENT).fg(Color::Black).bold())
            .highlight_symbol("› ")
            .highlight_spacing(HighlightSpacing::Always);
        scroll.select(state.selected());
        frame.render_stateful_widget(table, area, scroll);

        // The header's bottom margin is left blank by the table; draw an unbroken rule
        // there (a border per cell would break at every column gap).
        if area.height >= HEADER_HEIGHT {
            let rule = Line::from("─".repeat(usize::from(area.width))).fg(MUTED);
            frame.render_widget(
                rule,
                Rect {
                    y: area.y + 1,
                    height: 1,
                    ..area
                },
            );
        }
    }
}

/// `3 profiles`, or `1 of 3` while filtering.
fn count(state: &PickerState) -> String {
    let total = state.rows().len();
    if state.filter().is_empty() {
        let plural = if total == 1 { "" } else { "s" };
        format!("{total} profile{plural}")
    } else {
        format!("{} of {total}", state.visible().len())
    }
}

fn help_line() -> Line<'static> {
    let mut spans = vec![Span::from(" ")];
    for (key, action) in HELP {
        spans.push(Span::from(format!(" {key}")).bold());
        spans.push(Span::from(format!(" {action}  ")).fg(MUTED));
    }
    Line::from(spans)
}

/// Each column as wide as its widest cell; the last one takes the remaining space.
fn column_widths(header: &[&str], rows: &[Vec<String>]) -> Vec<Constraint> {
    let mut widths: Vec<usize> = header
        .iter()
        .map(|title| Span::from(*title).width())
        .collect();
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(Span::from(cell.as_str()).width());
        }
    }
    let last = widths.len().saturating_sub(1);
    widths
        .into_iter()
        .enumerate()
        .map(|(i, width)| {
            if i == last {
                Constraint::Fill(1)
            } else {
                Constraint::Length(to_u16(width))
            }
        })
        .collect()
}

fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    const HEADER: [&str; 3] = ["NAME", "TARGET", "NOTE"];

    fn rows() -> Vec<Vec<String>> {
        [
            ["kantor", "root@10.0.0.5:2222", "office"],
            ["vps", "vps.example", ""],
        ]
        .iter()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect())
        .collect()
    }

    /// Draws one frame and returns the screen, one string per line (trailing spaces cut).
    fn screen(
        state: &mut PickerState,
        rows: &[Vec<String>],
        width: u16,
        height: u16,
    ) -> (Vec<String>, Terminal<TestBackend>) {
        let view = View::new("Connect to", &HEADER, rows);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut scroll = TableState::default();
        terminal
            .draw(|frame| view.draw(frame, state, &mut scroll))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines = (0..height)
            .map(|y| {
                let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                line.trim_end().to_string()
            })
            .collect();
        (lines, terminal)
    }

    fn type_text(state: &mut PickerState, text: &str) {
        for c in text.chars() {
            state.handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    #[test]
    fn shows_box_search_table_and_help() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        let (lines, mut terminal) = screen(&mut state, &rows, 60, 10);
        assert_eq!(
            lines,
            [
                "╭ Connect to ────────────────────────────────── 2 profiles ╮",
                "│ / type to filter                                         │",
                "│                                                          │",
                "│   NAME    TARGET              NOTE                       │",
                "│ ──────────────────────────────────────────────────────── │",
                "│ › kantor  root@10.0.0.5:2222  office                     │",
                "│   vps     vps.example                                    │",
                "│                                                          │",
                "╰──────────────────────────────────────────────────────────╯",
                "  ↑↓ move   type to filter   enter choose   esc cancel",
            ]
        );
        // Typing goes right after the search mark.
        terminal.backend_mut().assert_cursor_position((4, 1));
    }

    #[test]
    fn highlighted_row_uses_the_accent_color() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        let (_, terminal) = screen(&mut state, &rows, 60, 10);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(4, 5)].symbol(), "k");
        assert_eq!(buffer[(4, 5)].bg, ACCENT);
        assert_ne!(buffer[(4, 6)].bg, ACCENT, "other rows stay plain");
    }

    #[test]
    fn filtering_counts_matches_and_keeps_columns() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        type_text(&mut state, "vps");
        let (lines, mut terminal) = screen(&mut state, &rows, 60, 10);
        assert_eq!(
            lines[0],
            "╭ Connect to ────────────────────────────────────── 1 of 2 ╮"
        );
        assert_eq!(
            lines[1],
            "│ / vps                                                    │"
        );
        assert_eq!(
            lines[3],
            "│   NAME    TARGET              NOTE                       │"
        );
        assert_eq!(
            lines[5],
            "│ › vps     vps.example                                    │"
        );
        terminal.backend_mut().assert_cursor_position((7, 1));
    }

    #[test]
    fn says_so_when_nothing_matches() {
        let rows = rows();
        let mut state = PickerState::new(&rows);
        type_text(&mut state, "zzz");
        let (lines, _) = screen(&mut state, &rows, 60, 10);
        assert_eq!(
            lines[0],
            "╭ Connect to ────────────────────────────────────── 0 of 2 ╮"
        );
        assert_eq!(
            lines[3],
            "│ no profile matches 'zzz'                                 │"
        );
    }

    #[test]
    fn one_profile_is_singular() {
        let rows = vec![vec!["vps".to_string(), "h".into(), String::new()]];
        let mut state = PickerState::new(&rows);
        let (lines, _) = screen(&mut state, &rows, 60, 10);
        assert!(lines[0].ends_with("─ 1 profile ╮"), "{}", lines[0]);
    }

    #[test]
    fn page_size_follows_the_screen() {
        let rows: Vec<Vec<String>> = (0..5)
            .map(|i| vec![format!("host{i}"), "h".into(), String::new()])
            .collect();
        let mut state = PickerState::new(&rows);
        // 9 lines: box borders, search, blank, header, rule and help leave 2 for rows.
        screen(&mut state, &rows, 60, 9);
        state.handle(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(state.selected(), Some(2));
    }

    #[test]
    fn tiny_screens_do_not_panic() {
        let rows = rows();
        for (width, height) in [(1, 1), (2, 2), (20, 5), (8, 3), (60, 4), (60, 7)] {
            let mut state = PickerState::new(&rows);
            screen(&mut state, &rows, width, height);
        }
    }

    #[test]
    fn widths_fit_the_widest_cell_and_the_last_column_fills() {
        assert_eq!(
            column_widths(&HEADER, &rows()),
            [
                Constraint::Length(6),
                Constraint::Length(18),
                Constraint::Fill(1)
            ]
        );
    }
}
