use crossterm::event::KeyEvent;
use meshcore_rs::events::Contact;
use ratatui::{layout::Rect, Frame};

use crate::app::App;
use crate::choices::{Choices, ChoicesKey};
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
    choices: Choices<Choice>,
}

impl ContactMenu {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            choices: Choices::new(CHOICES.map(|c| (c.label(), c)).to_vec()),
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
        let popover = Popover::new(title, self.choices.width(), self.choices.height());
        let inner = popover.render(f, area, theme);
        self.choices.draw(f, inner, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match self.choices.handle_key(key) {
            ChoicesKey::Ignored => KeyResult::Ignored,
            ChoicesKey::Handled => KeyResult::Handled,
            ChoicesKey::Chosen(choice) => match self.contact(app) {
                Some(contact) => KeyResult::Action(choice.action(contact)),
                None => KeyResult::Ignored,
            },
        }
    }

    fn key_hints(&self) -> Vec<&'static str> {
        vec!["↑↓=choose", "Enter=select", "Esc=close"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crossterm::event::{KeyCode, KeyModifiers};

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
    fn arrows_are_handled() {
        let app = app_with_contacts(1);
        let mut menu = ContactMenu::new(app.contacts[0].public_key);
        assert_eq!(press(&mut menu, &app, KeyCode::Up), KeyResult::Handled);
        assert_eq!(press(&mut menu, &app, KeyCode::Down), KeyResult::Handled);
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
