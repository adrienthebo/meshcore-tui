use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::view::{Panel, Tab, Ui};

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

    fn is_empty(&self) -> bool {
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

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_offset(self.cursor);
            self.text.remove(at);
        }
    }

    fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.char_count());
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Return the text and clear the line
    fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Display width of the text before the cursor
    pub fn cursor_column(&self) -> u16 {
        let before = &self.text[..self.byte_offset(self.cursor)];
        UnicodeWidthStr::width(before) as u16
    }
}

impl Ui {
    /// Apply a key press; keys every tab shares come first, the rest go to the
    /// tab on screen. Returns the input line when Enter submits it.
    pub fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> Option<String> {
        match key.code {
            KeyCode::Esc => app.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.should_quit = true;
            }
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::ALT) && c.is_ascii_digit() => {
                if let Some(tab) = Tab::for_digit(c) {
                    self.tab = tab;
                }
            }
            _ => {
                return match self.tab {
                    Tab::Overview => self.handle_overview_key(key, app),
                }
            }
        }
        None
    }

    /// Overview keys: edit and submit the input line, move focus between panels, select contacts
    fn handle_overview_key(&mut self, key: KeyEvent, app: &mut App) -> Option<String> {
        let input = &mut self.overview.input;
        match key.code {
            KeyCode::Enter if !input.is_empty() => return Some(input.take()),
            KeyCode::Char(c) => input.insert(c),
            KeyCode::Backspace => input.backspace(),
            KeyCode::Left => input.left(),
            KeyCode::Right => input.right(),
            KeyCode::Home => input.home(),
            KeyCode::End => input.end(),
            KeyCode::Tab => {
                self.overview.focus = match self.overview.focus {
                    Panel::Messages => Panel::Contacts,
                    Panel::Contacts => Panel::Packets,
                    Panel::Packets => Panel::Messages,
                };
            }
            KeyCode::Up if self.overview.focus == Panel::Contacts => {
                let idx = app.selected_index().map_or(0, |s| s.saturating_sub(1));
                app.select_contact(idx);
            }
            KeyCode::Down if self.overview.focus == Panel::Contacts => {
                let last = app.contacts.len().saturating_sub(1);
                let idx = app.selected_index().map_or(0, |s| (s + 1).min(last));
                app.select_contact(idx);
            }
            KeyCode::F(1) => app.selected_key = None,
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

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

    // ── Key handling ─────────────────────────────────────────────────────────

    fn press(ui: &mut Ui, app: &mut App, code: KeyCode) -> Option<String> {
        ui.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app)
    }

    #[test]
    fn esc_quits() {
        let (mut ui, mut app) = (test_ui(), App::new(0));
        press(&mut ui, &mut app, KeyCode::Esc);
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_c_quits_without_typing() {
        let (mut ui, mut app) = (test_ui(), App::new(0));
        ui.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), &mut app);
        assert!(app.should_quit);
        assert!(ui.overview.input.is_empty());
    }

    #[test]
    fn alt_digits_switch_tabs_without_typing() {
        let (mut ui, mut app) = (test_ui(), App::new(0));
        ui.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT), &mut app);
        ui.handle_key(KeyEvent::new(KeyCode::Char('9'), KeyModifiers::ALT), &mut app);
        assert_eq!(ui.tab, Tab::Overview);
        assert!(ui.overview.input.is_empty());
    }

    #[test]
    fn typing_edits_the_input_line() {
        let (mut ui, mut app) = (test_ui(), App::new(0));
        for c in "hi!".chars() {
            press(&mut ui, &mut app, KeyCode::Char(c));
        }
        press(&mut ui, &mut app, KeyCode::Backspace);
        press(&mut ui, &mut app, KeyCode::Home);
        press(&mut ui, &mut app, KeyCode::Char('>'));
        assert_eq!(ui.overview.input.as_str(), ">hi");
    }

    #[test]
    fn enter_submits_and_clears_input() {
        let (mut ui, mut app) = (test_ui(), App::new(0));
        press(&mut ui, &mut app, KeyCode::Char('x'));
        assert_eq!(press(&mut ui, &mut app, KeyCode::Enter), Some("x".to_string()));
        assert!(ui.overview.input.is_empty());
    }

    #[test]
    fn enter_on_empty_input_submits_nothing() {
        assert_eq!(press(&mut test_ui(), &mut App::new(0), KeyCode::Enter), None);
    }

    #[test]
    fn tab_cycles_panels() {
        let (mut ui, mut app) = (test_ui(), App::new(0));
        press(&mut ui, &mut app, KeyCode::Tab);
        assert_eq!(ui.overview.focus, Panel::Contacts);
        press(&mut ui, &mut app, KeyCode::Tab);
        assert_eq!(ui.overview.focus, Panel::Packets);
        press(&mut ui, &mut app, KeyCode::Tab);
        assert_eq!(ui.overview.focus, Panel::Messages);
    }

    #[test]
    fn arrows_select_contacts_only_in_contacts_panel() {
        let (mut ui, mut app) = (test_ui(), app_with_contacts(3));
        press(&mut ui, &mut app, KeyCode::Down);
        assert_eq!(app.selected_index(), None);
        ui.overview.focus = Panel::Contacts;
        press(&mut ui, &mut app, KeyCode::Down);
        assert_eq!(app.selected_index(), Some(0));
        press(&mut ui, &mut app, KeyCode::Down);
        press(&mut ui, &mut app, KeyCode::Down);
        press(&mut ui, &mut app, KeyCode::Down);
        assert_eq!(app.selected_index(), Some(2));
        press(&mut ui, &mut app, KeyCode::Up);
        press(&mut ui, &mut app, KeyCode::Up);
        press(&mut ui, &mut app, KeyCode::Up);
        assert_eq!(app.selected_index(), Some(0));
    }

    #[test]
    fn up_with_no_selection_selects_first_contact() {
        let (mut ui, mut app) = (test_ui(), app_with_contacts(2));
        ui.overview.focus = Panel::Contacts;
        press(&mut ui, &mut app, KeyCode::Up);
        assert_eq!(app.selected_index(), Some(0));
    }

    #[test]
    fn arrows_with_no_contacts_select_nothing() {
        let (mut ui, mut app) = (test_ui(), App::new(0));
        ui.overview.focus = Panel::Contacts;
        press(&mut ui, &mut app, KeyCode::Down);
        press(&mut ui, &mut app, KeyCode::Up);
        assert_eq!(app.selected_key, None);
    }

    #[test]
    fn f1_clears_contact_selection() {
        let (mut ui, mut app) = (test_ui(), app_with_contacts(1));
        app.select_contact(0);
        press(&mut ui, &mut app, KeyCode::F(1));
        assert_eq!(app.selected_key, None);
    }
}
