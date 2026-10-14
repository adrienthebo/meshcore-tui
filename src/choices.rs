use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{List, ListState},
    Frame,
};

use crate::theme::Theme;

/// Marks the highlighted option
const HIGHLIGHT_SYMBOL: &str = "> ";

/// A short list of labelled options, one highlighted; ↑↓ move the highlight
/// and Enter chooses it. The caller decides where to draw it, e.g. in a Popover
#[derive(Debug)]
pub struct Choices<T> {
    /// Each option's label and what choosing it gives back
    options: Vec<(&'static str, T)>,
    /// The highlighted option's index in `options`
    cursor: usize,
    /// Drawing selects the highlighted option's row
    list: RefCell<ListState>,
}

/// What Choices did with a key
#[derive(Debug, PartialEq)]
pub enum ChoicesKey<T> {
    /// Not a key Choices uses
    Ignored,
    /// Used to move the highlight
    Handled,
    /// Enter, on this option
    Chosen(T),
}

impl<T: Copy> Choices<T> {
    /// Options to choose from, highlighting the first; `options` mustn't be empty
    pub fn new(options: Vec<(&'static str, T)>) -> Self {
        Self {
            options,
            cursor: 0,
            list: RefCell::default(),
        }
    }

    /// Columns the options take, highlight symbol included
    pub fn width(&self) -> u16 {
        let label = self.options.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
        (label + HIGHLIGHT_SYMBOL.len()) as u16
    }

    /// Rows the options take
    pub fn height(&self) -> u16 {
        self.options.len() as u16
    }

    /// The highlighted option
    pub fn highlighted(&self) -> T {
        self.options[self.cursor].1
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ChoicesKey<T> {
        match key.code {
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => self.cursor = (self.cursor + 1).min(self.options.len() - 1),
            KeyCode::Enter => return ChoicesKey::Chosen(self.highlighted()),
            _ => return ChoicesKey::Ignored,
        }
        ChoicesKey::Handled
    }

    /// The options as a column, the highlighted one marked
    pub fn draw(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let list = List::new(self.options.iter().map(|(label, _)| *label))
            .style(Style::default().fg(theme.text))
            .highlight_symbol(HIGHLIGHT_SYMBOL)
            .highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg));
        let mut state = self.list.borrow_mut();
        state.select(Some(self.cursor));
        f.render_stateful_widget(list, area, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn press(choices: &mut Choices<u8>, code: KeyCode) -> ChoicesKey<u8> {
        choices.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn three() -> Choices<u8> {
        Choices::new(vec![("One", 1), ("Two", 2), ("Three", 3)])
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
        assert_eq!(choices.highlighted(), 1);
        for _ in 0..5 {
            press(&mut choices, KeyCode::Down);
        }
        assert_eq!(choices.highlighted(), 3);
    }

    #[test]
    fn other_keys_are_ignored() {
        let mut choices = three();
        assert_eq!(press(&mut choices, KeyCode::Tab), ChoicesKey::Ignored);
        assert_eq!(press(&mut choices, KeyCode::Esc), ChoicesKey::Ignored);
    }

    #[test]
    fn size_fits_the_widest_label_and_every_option() {
        let choices = three();
        assert_eq!(choices.width(), "Three".len() as u16 + 2);
        assert_eq!(choices.height(), 3);
    }
}
