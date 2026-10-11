use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

use crate::app::App;
use crate::input::InputLine;
use crate::theme::Theme;
use crate::ui::{draw_contacts, draw_messages, draw_overview, draw_packets, draw_radio};
use crate::update::Action;
use crate::view::{KeyResult, View};

/// The Overview panel that has keyboard focus
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    Input,
    Messages,
    Contacts,
    Packets,
}

/// Chat, contacts, packets and the radio, with the input line for sending
#[derive(Debug, Default)]
pub struct Overview {
    pub focus: Panel,
    pub messages: MessageLog,
    pub contacts: ContactList,
    pub packets: PacketLog,
    pub radio: RadioPanel,
    pub input: InputLine,
}

impl View for Overview {
    fn title(&self) -> &str {
        "Overview"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_overview(f, app, self, theme, area);
    }

    /// The panel with focus gets keys first; Overview takes the ones it ignores:
    /// Tab moves focus between panels and F1 deselects
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        let result = self.focused_panel().handle_key(key, app);
        if result != KeyResult::Ignored {
            return result;
        }
        match key.code {
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Panel::Input => Panel::Messages,
                    Panel::Messages => Panel::Contacts,
                    Panel::Contacts => Panel::Packets,
                    Panel::Packets => Panel::Input,
                };
                KeyResult::Handled
            }
            KeyCode::F(1) => KeyResult::Action(Action::Deselect),
            _ => KeyResult::Ignored,
        }
    }
}

impl Overview {
    fn focused_panel(&mut self) -> &mut dyn View {
        match self.focus {
            Panel::Input => &mut self.input,
            Panel::Messages => &mut self.messages,
            Panel::Contacts => &mut self.contacts,
            Panel::Packets => &mut self.packets,
        }
    }
}

/// Chat and system messages, newest at the bottom
#[derive(Debug, Default)]
pub struct MessageLog;

impl View for MessageLog {
    fn title(&self) -> &str {
        "Messages"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        draw_messages(f, app, theme, focused, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }
}

/// The radio's contacts; ↑↓ choose who messages go to
#[derive(Debug, Default)]
pub struct ContactList;

impl View for ContactList {
    fn title(&self) -> &str {
        "Contacts"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        draw_contacts(f, app, theme, focused, area);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match key.code {
            KeyCode::Up => {
                let idx = app.selected_index().map_or(0, |s| s.saturating_sub(1));
                KeyResult::Action(Action::SelectContact(idx))
            }
            KeyCode::Down => {
                let last = app.contacts.len().saturating_sub(1);
                let idx = app.selected_index().map_or(0, |s| (s + 1).min(last));
                KeyResult::Action(Action::SelectContact(idx))
            }
            _ => KeyResult::Ignored,
        }
    }
}

/// Packets heard and sent, newest at the bottom
#[derive(Debug, Default)]
pub struct PacketLog;

impl View for PacketLog {
    fn title(&self) -> &str {
        "Packets"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        draw_packets(f, app, theme, focused, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }
}

/// The noise floor graph and latest signal readings
#[derive(Debug, Default)]
pub struct RadioPanel;

impl View for RadioPanel {
    fn title(&self) -> &str {
        "Radio"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_radio(f, app, theme, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crossterm::event::KeyModifiers;

    fn press(view: &mut impl View, app: &App, code: KeyCode) -> KeyResult {
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app)
    }

    // ── Overview ─────────────────────────────────────────────────────────────

    #[test]
    fn typing_reaches_the_input_line_only_while_it_has_focus() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        assert_eq!(overview.focus, Panel::Input);
        press(&mut overview, &app, KeyCode::Char('a'));
        overview.focus = Panel::Contacts;
        assert_eq!(press(&mut overview, &app, KeyCode::Char('b')), KeyResult::Ignored);
        assert_eq!(overview.input.as_str(), "a");
    }

    #[test]
    fn keys_the_focused_panel_ignores_reach_the_overview() {
        let (mut overview, app) = (Overview::default(), app_with_contacts(1));
        // The input line ignores Tab and F1
        assert_eq!(press(&mut overview, &app, KeyCode::F(1)), KeyResult::Action(Action::Deselect));
        assert_eq!(press(&mut overview, &app, KeyCode::Tab), KeyResult::Handled);
        assert_eq!(overview.focus, Panel::Messages);
    }

    #[test]
    fn enter_submits_the_input_line() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        press(&mut overview, &app, KeyCode::Char('x'));
        assert_eq!(
            press(&mut overview, &app, KeyCode::Enter),
            KeyResult::Action(Action::Submit("x".to_string()))
        );
    }

    #[test]
    fn tab_cycles_panels() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        let mut order = Vec::new();
        for _ in 0..4 {
            press(&mut overview, &app, KeyCode::Tab);
            order.push(overview.focus.clone());
        }
        assert_eq!(order, [Panel::Messages, Panel::Contacts, Panel::Packets, Panel::Input]);
    }

    #[test]
    fn arrows_reach_the_contact_list_only_while_it_has_focus() {
        let (mut overview, app) = (Overview::default(), app_with_contacts(3));
        assert_eq!(press(&mut overview, &app, KeyCode::Down), KeyResult::Ignored);
        overview.focus = Panel::Contacts;
        assert_eq!(press(&mut overview, &app, KeyCode::Down), KeyResult::Action(Action::SelectContact(0)));
    }

    #[test]
    fn f1_deselects() {
        let (mut overview, app) = (Overview::default(), app_with_contacts(1));
        assert_eq!(press(&mut overview, &app, KeyCode::F(1)), KeyResult::Action(Action::Deselect));
    }

    // ── ContactList ──────────────────────────────────────────────────────────

    #[test]
    fn contact_arrows_move_the_selection_within_the_list() {
        let mut app = app_with_contacts(3);
        let mut list = ContactList;
        app.select_contact(2);
        assert_eq!(press(&mut list, &app, KeyCode::Down), KeyResult::Action(Action::SelectContact(2)));
        assert_eq!(press(&mut list, &app, KeyCode::Up), KeyResult::Action(Action::SelectContact(1)));
        app.select_contact(0);
        assert_eq!(press(&mut list, &app, KeyCode::Up), KeyResult::Action(Action::SelectContact(0)));
    }

    #[test]
    fn contact_arrows_with_no_selection_select_the_first() {
        let app = app_with_contacts(2);
        assert_eq!(press(&mut ContactList, &app, KeyCode::Up), KeyResult::Action(Action::SelectContact(0)));
        assert_eq!(press(&mut ContactList, &app, KeyCode::Down), KeyResult::Action(Action::SelectContact(0)));
    }

    #[test]
    fn contact_list_ignores_other_keys() {
        assert_eq!(press(&mut ContactList, &App::new(0), KeyCode::Char('a')), KeyResult::Ignored);
    }
}
