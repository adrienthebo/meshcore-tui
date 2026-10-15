use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::theme::Theme;
use crate::ui::draw_input;
use crate::update::Action;
use crate::view::{KeyResult, View};

/// Single-line text input with a cursor measured in chars
#[derive(Debug, Default)]
pub struct InputLine {
    text: String,
    cursor: usize,
}

impl InputLine {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    /// Byte offset of the char at `char_idx` (or the end of the text)
    fn byte_offset(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte_offset(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_offset(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.char_count());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Return the text and clear the line
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Chars before the cursor
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Display width of the text before the cursor
    pub fn cursor_column(&self) -> u16 {
        let before = &self.text[..self.byte_offset(self.cursor)];
        UnicodeWidthStr::width(before) as u16
    }
}

impl View for InputLine {
    fn title(&self) -> &str {
        "Input"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        draw_input(f, app, self, theme, focused, area);
    }

    /// Edit the line, refusing chars past the radio's limit; Enter submits it
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match key.code {
            KeyCode::Enter if !self.is_empty() => {
                return KeyResult::Action(Action::Submit(self.take()))
            }
            KeyCode::Char(c) if self.text.len() + c.len_utf8() > app.text_limit() => {}
            KeyCode::Char(c) => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }

    fn key_hints(&self) -> Vec<&'static str> {
        vec!["Enter=send"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── InputLine ────────────────────────────────────────────────────────────

    fn typed(s: &str) -> InputLine {
        let mut line = InputLine::default();
        for c in s.chars() {
            line.insert(c);
        }
        line
    }

    #[test]
    fn input_insert_appends_at_cursor() {
        let mut line = typed("ac");
        line.left();
        line.insert('b');
        assert_eq!(line.as_str(), "abc");
        assert_eq!(line.cursor, 2);
    }

    #[test]
    fn input_backspace_removes_char_before_cursor() {
        let mut line = typed("abc");
        line.left();
        line.backspace();
        assert_eq!(line.as_str(), "ac");
        assert_eq!(line.cursor, 1);
    }

    #[test]
    fn input_backspace_at_start_does_nothing() {
        let mut line = typed("ab");
        line.home();
        line.backspace();
        assert_eq!(line.as_str(), "ab");
        assert_eq!(line.cursor, 0);
    }

    #[test]
    fn input_cursor_movement_is_clamped() {
        let mut line = typed("ab");
        line.right();
        assert_eq!(line.cursor, 2);
        line.home();
        line.left();
        assert_eq!(line.cursor, 0);
        line.end();
        assert_eq!(line.cursor, 2);
    }

    #[test]
    fn input_edits_multibyte_text_without_panicking() {
        let mut line = typed("héllo");
        line.home();
        line.right();
        line.right();
        line.backspace();
        assert_eq!(line.as_str(), "hllo");
        line.insert('ë');
        assert_eq!(line.as_str(), "hëllo");
        line.end();
        line.insert('ü');
        assert_eq!(line.as_str(), "hëlloü");
    }

    #[test]
    fn input_cursor_column_counts_display_width() {
        let mut line = typed("日本x");
        assert_eq!(line.cursor_column(), 5);
        line.left();
        assert_eq!(line.cursor_column(), 4);
        line.home();
        assert_eq!(line.cursor_column(), 0);
    }

    #[test]
    fn input_take_returns_text_and_clears() {
        let mut line = typed("héllo");
        assert_eq!(line.take(), "héllo");
        assert!(line.is_empty());
        assert_eq!(line.cursor, 0);
    }

    // ── Keys ─────────────────────────────────────────────────────────────────

    fn press(line: &mut InputLine, code: KeyCode) -> KeyResult {
        line.handle_key(
            KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
            &App::new(0),
        )
    }

    #[test]
    fn keys_edit_the_line() {
        let mut line = InputLine::default();
        for c in "hi!".chars() {
            assert_eq!(press(&mut line, KeyCode::Char(c)), KeyResult::Handled);
        }
        press(&mut line, KeyCode::Backspace);
        press(&mut line, KeyCode::Home);
        press(&mut line, KeyCode::Char('>'));
        assert_eq!(line.as_str(), ">hi");
    }

    #[test]
    fn chars_past_the_byte_limit_are_refused_whole() {
        let mut line = typed(&"x".repeat(App::new(0).text_limit() - 2));
        assert_eq!(press(&mut line, KeyCode::Char('🖖')), KeyResult::Handled);
        assert_eq!(press(&mut line, KeyCode::Char('é')), KeyResult::Handled);
        assert!(line.as_str().ends_with("xé"));
        press(&mut line, KeyCode::Char('x'));
        assert!(line.as_str().ends_with("xé"));
    }

    #[test]
    fn enter_submits_and_clears_the_line() {
        let mut line = typed("x");
        assert_eq!(
            press(&mut line, KeyCode::Enter),
            KeyResult::Action(Action::Submit("x".to_string()))
        );
        assert!(line.is_empty());
    }

    #[test]
    fn enter_on_an_empty_line_and_other_keys_are_ignored() {
        let mut line = InputLine::default();
        assert_eq!(press(&mut line, KeyCode::Enter), KeyResult::Ignored);
        assert_eq!(press(&mut line, KeyCode::Up), KeyResult::Ignored);
    }
}
