use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::events::Contact;
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{List, ListState},
    Frame,
};

use crate::app::App;
use crate::popover::Popover;
use crate::text::sanitize;
use crate::theme::Theme;
use crate::update::Action;
use crate::view::{KeyResult, View};

/// Something to do with a contact, as the menu offers it
#[derive(Debug, Clone, Copy, PartialEq)]
enum Choice {
    Message,
}

/// Every choice, in menu order
const CHOICES: [Choice; 1] = [Choice::Message];

/// Marks the highlighted choice
const HIGHLIGHT_SYMBOL: &str = "> ";

impl Choice {
    fn label(self) -> &'static str {
        match self {
            Choice::Message => "Message",
        }
    }

    fn action(self, contact: &Contact) -> Action {
        match self {
            Choice::Message => Action::MessageContact(contact.public_key),
        }
    }
}

/// The things to do with one contact, drawn as a popover; ↑↓ choose and Enter
/// asks for the chosen one
#[derive(Debug)]
pub struct ContactMenu {
    /// The contact's public key; the menu draws nothing and chooses nothing
    /// while it isn't in the contact list
    key: [u8; 32],
    /// The highlighted choice's index in CHOICES
    cursor: usize,
    /// Drawing selects the highlighted choice's row
    list: RefCell<ListState>,
}

impl ContactMenu {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            cursor: 0,
            list: RefCell::default(),
        }
    }

    /// The contact the menu is for, while it's in the contact list
    pub fn contact<'a>(&self, app: &'a App) -> Option<&'a Contact> {
        app.contacts.iter().find(|c| c.public_key == self.key)
    }
}

impl View for ContactMenu {
    fn title(&self) -> &str {
        "Contact"
    }

    /// A popover titled with the contact's name and key prefix, centered in `area`
    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        let Some(contact) = self.contact(app) else {
            return;
        };
        let title = format!(
            " {} [{}] ",
            sanitize(&contact.adv_name),
            &contact.prefix_hex()[..8]
        );
        let labels = CHOICES.map(Choice::label);
        let width = labels.iter().map(|l| l.len()).max().unwrap_or(0) + HIGHLIGHT_SYMBOL.len();
        let inner = Popover::new(title, width as u16, CHOICES.len() as u16).render(f, area, theme);
        let list = List::new(labels)
            .style(Style::default().fg(theme.text))
            .highlight_symbol(HIGHLIGHT_SYMBOL)
            .highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg));
        let mut state = self.list.borrow_mut();
        state.select(Some(self.cursor));
        f.render_stateful_widget(list, inner, &mut state);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match key.code {
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => self.cursor = (self.cursor + 1).min(CHOICES.len() - 1),
            KeyCode::Enter => {
                return match self.contact(app) {
                    Some(contact) => KeyResult::Action(CHOICES[self.cursor].action(contact)),
                    None => KeyResult::Ignored,
                };
            }
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }

    fn key_hints(&self) -> Vec<&'static str> {
        vec!["↑↓=choose", "Enter=select", "Esc=close"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crossterm::event::KeyModifiers;

    fn press(menu: &mut ContactMenu, app: &App, code: KeyCode) -> KeyResult {
        menu.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app)
    }

    #[test]
    fn enter_asks_to_message_the_contact() {
        let app = app_with_contacts(2);
        let key = app.contacts[1].public_key;
        let mut menu = ContactMenu::new(key);
        assert_eq!(
            press(&mut menu, &app, KeyCode::Enter),
            KeyResult::Action(Action::MessageContact(key))
        );
    }

    #[test]
    fn arrows_choose_within_bounds() {
        let app = app_with_contacts(1);
        let mut menu = ContactMenu::new(app.contacts[0].public_key);
        assert_eq!(press(&mut menu, &app, KeyCode::Up), KeyResult::Handled);
        assert_eq!(menu.cursor, 0);
        for _ in 0..5 {
            press(&mut menu, &app, KeyCode::Down);
        }
        assert_eq!(menu.cursor, CHOICES.len() - 1);
    }

    #[test]
    fn a_contact_that_has_gone_offers_nothing() {
        let app = app_with_contacts(1);
        let mut menu = ContactMenu::new([9; 32]);
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn other_keys_are_not_the_menus() {
        let app = app_with_contacts(1);
        let mut menu = ContactMenu::new(app.contacts[0].public_key);
        assert_eq!(press(&mut menu, &app, KeyCode::Tab), KeyResult::Ignored);
        assert_eq!(press(&mut menu, &app, KeyCode::Esc), KeyResult::Ignored);
    }
}
