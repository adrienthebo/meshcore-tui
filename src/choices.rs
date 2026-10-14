use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

/// Marks the highlighted option
const HIGHLIGHT_SYMBOL: &str = "\u{25b6} ";

/// A short list of labelled options, one highlighted; ↑↓ move the highlight
/// and Enter chooses it. The caller decides where to draw it, e.g. in a Popover
#[derive(Debug)]
pub struct Choices<T> {
    rows: Vec<Row<T>>,
    /// The highlighted row's index in `rows`; never a divider while there's
    /// an option
    cursor: usize,
}

/// One row of Choices
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Row<T> {
    /// An option with its label and what choosing it gives back
    Enabled(&'static str, T),
    /// An option shown but not offered: it can be highlighted, not chosen
    Disabled(&'static str),
    /// A line between groups of options; the highlight passes over it
    Divider,
}

impl<T> Row<T> {
    fn label(&self) -> &'static str {
        match self {
            Row::Enabled(label, _) | Row::Disabled(label) => label,
            Row::Divider => "",
        }
    }

    fn is_option(&self) -> bool {
        match self {
            Row::Enabled(..) | Row::Disabled(_) => true,
            Row::Divider => false,
        }
    }
}

/// What Choices did with a key
#[derive(Debug, PartialEq)]
pub enum ChoicesKey<T> {
    /// Not a key Choices uses
    Ignored,
    /// Used to move the highlight, or Enter on a disabled option
    Handled,
    /// Enter, on this option
    Chosen(T),
}

impl<T: Copy> Choices<T> {
    /// Rows to choose from, highlighting the first option
    pub fn new(rows: Vec<Row<T>>) -> Self {
        let cursor = rows.iter().position(Row::is_option).unwrap_or(0);
        Self { rows, cursor }
    }

    /// Columns the rows take, highlight symbol included
    pub fn width(&self) -> u16 {
        let label = self
            .rows
            .iter()
            .map(|r| r.label().width())
            .max()
            .unwrap_or(0);
        (label + HIGHLIGHT_SYMBOL.width()) as u16
    }

    /// Rows the options and dividers take
    pub fn height(&self) -> u16 {
        self.rows.len() as u16
    }

    /// The highlighted option, if it's enabled
    pub fn highlighted(&self) -> Option<T> {
        match self.rows.get(self.cursor)? {
            Row::Enabled(_, value) => Some(*value),
            Row::Disabled(_) | Row::Divider => None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ChoicesKey<T> {
        let cursor = self.cursor;
        // The nearest option in the key's direction; the highlight stays put
        // when there's none
        let next = match key.code {
            KeyCode::Up => self.rows[..cursor].iter().rposition(Row::is_option),
            KeyCode::Down => self
                .rows
                .get(cursor + 1..)
                .and_then(|after| after.iter().position(Row::is_option))
                .map(|i| cursor + 1 + i),
            KeyCode::Enter => {
                return self
                    .highlighted()
                    .map_or(ChoicesKey::Handled, ChoicesKey::Chosen)
            }
            _ => return ChoicesKey::Ignored,
        };
        self.cursor = next.unwrap_or(cursor);
        ChoicesKey::Handled
    }

    /// The rows as a column, one per line: the highlighted option marked,
    /// disabled ones grey and italic, dividers as a rule across `area`. Rows
    /// past the bottom of `area` are cut off
    pub fn draw(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        for (index, (row, line)) in self.rows.iter().zip(area.rows()).enumerate() {
            let highlighted = index == self.cursor;
            let text = match row {
                Row::Divider => "─".repeat(line.width as usize),
                Row::Enabled(label, _) | Row::Disabled(label) if highlighted => {
                    format!("{HIGHLIGHT_SYMBOL}{label}")
                }
                Row::Enabled(label, _) | Row::Disabled(label) => {
                    format!("{}{label}", " ".repeat(HIGHLIGHT_SYMBOL.width()))
                }
            };
            // A highlighted disabled option keeps its disabled look
            let mut style = match row {
                Row::Enabled(..) if highlighted => Style::default().fg(theme.selected_fg),
                Row::Enabled(..) => Style::default().fg(theme.text),
                Row::Disabled(_) => Style::default()
                    .fg(theme.disabled)
                    .add_modifier(Modifier::ITALIC),
                Row::Divider => Style::default().fg(theme.muted),
            };
            if highlighted {
                style = style.bg(theme.selected_bg);
            }
            f.render_widget(Paragraph::new(text).style(style), line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn press(choices: &mut Choices<u8>, code: KeyCode) -> ChoicesKey<u8> {
        choices.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn three() -> Choices<u8> {
        Choices::new(vec![
            Row::Enabled("One", 1),
            Row::Enabled("Two", 2),
            Row::Enabled("Three", 3),
        ])
    }

    /// Two groups split by a divider, with disabled options in each
    fn grouped() -> Choices<u8> {
        Choices::new(vec![
            Row::Disabled("Soon"),
            Row::Enabled("One", 1),
            Row::Divider,
            Row::Enabled("Two", 2),
            Row::Disabled("Never"),
        ])
    }

    fn render(choices: &Choices<u8>, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| choices.draw(f, f.area(), &Theme::CLASSIC))
            .unwrap();
        terminal.backend().clone()
    }

    #[test]
    fn enter_chooses_the_highlighted_option() {
        let mut choices = three();
        assert_eq!(press(&mut choices, KeyCode::Enter), ChoicesKey::Chosen(1));
        press(&mut choices, KeyCode::Down);
        assert_eq!(press(&mut choices, KeyCode::Enter), ChoicesKey::Chosen(2));
    }

    #[test]
    fn arrows_move_within_bounds() {
        let mut choices = three();
        assert_eq!(press(&mut choices, KeyCode::Up), ChoicesKey::Handled);
        assert_eq!(choices.highlighted(), Some(1));
        for _ in 0..5 {
            press(&mut choices, KeyCode::Down);
        }
        assert_eq!(choices.highlighted(), Some(3));
    }

    #[test]
    fn disabled_options_highlight_but_enter_chooses_nothing() {
        let mut choices = grouped();
        assert_eq!(choices.cursor, 0);
        assert_eq!(choices.highlighted(), None);
        assert_eq!(press(&mut choices, KeyCode::Enter), ChoicesKey::Handled);
        press(&mut choices, KeyCode::Down);
        assert_eq!(press(&mut choices, KeyCode::Enter), ChoicesKey::Chosen(1));
    }

    #[test]
    fn the_highlight_passes_over_dividers() {
        let mut choices = grouped();
        press(&mut choices, KeyCode::Down);
        press(&mut choices, KeyCode::Down);
        assert_eq!(choices.highlighted(), Some(2));
        press(&mut choices, KeyCode::Down);
        press(&mut choices, KeyCode::Down);
        assert_eq!(choices.cursor, 4);
        press(&mut choices, KeyCode::Up);
        press(&mut choices, KeyCode::Up);
        assert_eq!(choices.highlighted(), Some(1));
    }

    #[test]
    fn other_keys_are_ignored() {
        let mut choices = three();
        assert_eq!(press(&mut choices, KeyCode::Tab), ChoicesKey::Ignored);
        assert_eq!(press(&mut choices, KeyCode::Esc), ChoicesKey::Ignored);
    }

    #[test]
    fn size_fits_the_widest_label_and_every_row() {
        let choices = grouped();
        assert_eq!(choices.width(), "Never".len() as u16 + 2);
        assert_eq!(choices.height(), 5);
    }

    #[test]
    fn rows_draw_marked_disabled_and_divided() {
        let mut choices = grouped();
        press(&mut choices, KeyCode::Down);
        let backend = render(&choices, 8, 5);
        insta::assert_snapshot!(backend);
        let buffer = backend.buffer();
        let theme = Theme::CLASSIC;
        assert_eq!(buffer[(2, 0)].fg, theme.disabled);
        assert!(buffer[(2, 0)].modifier.contains(Modifier::ITALIC));
        assert!(!buffer[(2, 3)].modifier.contains(Modifier::ITALIC));
        // The highlight spans the row
        assert_eq!(buffer[(7, 1)].bg, theme.selected_bg);
        assert_eq!(buffer[(2, 1)].fg, theme.selected_fg);
        assert_eq!(buffer[(0, 2)].fg, theme.muted);
        assert_eq!(buffer[(2, 3)].fg, theme.text);
    }

    #[test]
    fn a_highlighted_disabled_option_stays_grey_and_italic() {
        let buffer = render(&grouped(), 8, 5).buffer().clone();
        let theme = Theme::CLASSIC;
        assert_eq!(buffer[(2, 0)].fg, theme.disabled);
        assert!(buffer[(2, 0)].modifier.contains(Modifier::ITALIC));
        assert_eq!(buffer[(2, 0)].bg, theme.selected_bg);
    }
}
