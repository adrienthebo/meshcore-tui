use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

use crate::app::App;
use crate::input::InputLine;
use crate::theme::Theme;
use crate::ui::draw_overview;
use crate::update::Action;
use crate::view::{KeyResult, View};

/// The Overview panel that has keyboard focus
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    Messages,
    Contacts,
    Packets,
}

/// Chat, contacts, packets and the radio, with the input line for sending
#[derive(Debug, Default)]
pub struct Overview {
    pub input: InputLine,
    pub focus: Panel,
}

impl View for Overview {
    fn title(&self) -> &str {
        "Overview"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
        draw_overview(f, app, self, theme, area);
    }

    /// Edit and submit the input line, move focus between panels, select contacts
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        let input = &mut self.input;
        match key.code {
            KeyCode::Enter if !input.is_empty() => return KeyResult::Action(Action::Submit(input.take())),
            KeyCode::Char(c) => input.insert(c),
            KeyCode::Backspace => input.backspace(),
            KeyCode::Left => input.left(),
            KeyCode::Right => input.right(),
            KeyCode::Home => input.home(),
            KeyCode::End => input.end(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Panel::Messages => Panel::Contacts,
                    Panel::Contacts => Panel::Packets,
                    Panel::Packets => Panel::Messages,
                };
            }
            KeyCode::Up if self.focus == Panel::Contacts => {
                let idx = app.selected_index().map_or(0, |s| s.saturating_sub(1));
                return KeyResult::Action(Action::SelectContact(idx));
            }
            KeyCode::Down if self.focus == Panel::Contacts => {
                let last = app.contacts.len().saturating_sub(1);
                let idx = app.selected_index().map_or(0, |s| (s + 1).min(last));
                return KeyResult::Action(Action::SelectContact(idx));
            }
            KeyCode::F(1) => return KeyResult::Action(Action::Deselect),
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crossterm::event::KeyModifiers;

    fn press(overview: &mut Overview, app: &App, code: KeyCode) -> KeyResult {
        overview.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app)
    }

    #[test]
    fn typing_edits_the_input_line() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        for c in "hi!".chars() {
            assert_eq!(press(&mut overview, &app, KeyCode::Char(c)), KeyResult::Handled);
        }
        press(&mut overview, &app, KeyCode::Backspace);
        press(&mut overview, &app, KeyCode::Home);
        press(&mut overview, &app, KeyCode::Char('>'));
        assert_eq!(overview.input.as_str(), ">hi");
    }

    #[test]
    fn enter_submits_and_clears_input() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        press(&mut overview, &app, KeyCode::Char('x'));
        assert_eq!(
            press(&mut overview, &app, KeyCode::Enter),
            KeyResult::Action(Action::Submit("x".to_string()))
        );
        assert!(overview.input.is_empty());
    }

    #[test]
    fn enter_on_empty_input_is_ignored() {
        assert_eq!(press(&mut Overview::default(), &App::new(0), KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn tab_cycles_panels() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        press(&mut overview, &app, KeyCode::Tab);
        assert_eq!(overview.focus, Panel::Contacts);
        press(&mut overview, &app, KeyCode::Tab);
        assert_eq!(overview.focus, Panel::Packets);
        press(&mut overview, &app, KeyCode::Tab);
        assert_eq!(overview.focus, Panel::Messages);
    }

    #[test]
    fn arrows_select_contacts_only_in_contacts_panel() {
        let (mut overview, mut app) = (Overview::default(), app_with_contacts(3));
        assert_eq!(press(&mut overview, &app, KeyCode::Down), KeyResult::Ignored);
        overview.focus = Panel::Contacts;
        assert_eq!(press(&mut overview, &app, KeyCode::Down), KeyResult::Action(Action::SelectContact(0)));
        app.select_contact(2);
        assert_eq!(press(&mut overview, &app, KeyCode::Down), KeyResult::Action(Action::SelectContact(2)));
        assert_eq!(press(&mut overview, &app, KeyCode::Up), KeyResult::Action(Action::SelectContact(1)));
        app.select_contact(0);
        assert_eq!(press(&mut overview, &app, KeyCode::Up), KeyResult::Action(Action::SelectContact(0)));
    }

    #[test]
    fn up_with_no_selection_selects_first_contact() {
        let (mut overview, app) = (Overview::default(), app_with_contacts(2));
        overview.focus = Panel::Contacts;
        assert_eq!(press(&mut overview, &app, KeyCode::Up), KeyResult::Action(Action::SelectContact(0)));
    }

    #[test]
    fn f1_deselects() {
        let (mut overview, app) = (Overview::default(), app_with_contacts(1));
        assert_eq!(press(&mut overview, &app, KeyCode::F(1)), KeyResult::Action(Action::Deselect));
    }
}
