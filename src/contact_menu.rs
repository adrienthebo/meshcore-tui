use crossterm::event::KeyEvent;
use meshcore_rs::events::Contact;
use ratatui::{layout::Rect, Frame};

use crate::app::{is_favorite, App};
use crate::choices::{Choices, ChoicesKey, Row};
use crate::popover::Popover;
use crate::text::sanitize;
use crate::theme::Theme;
use crate::update::Action;
use crate::view::{KeyResult, View};

/// Something to do with a contact, as the menu offers it
#[derive(Debug, Clone, Copy, PartialEq)]
enum Choice {
    Message,
    /// Mark the contact as a favorite (true) or unmark it
    Favorite(bool),
}

impl Choice {
    fn action(self, contact: &Contact) -> Action {
        let key = contact.public_key;
        match self {
            Choice::Message => Action::MessageContact(key),
            Choice::Favorite(favorite) => Action::SetFavorite { key, favorite },
        }
    }
}

/// The menu for a contact of this type (ADV_TYPE_*, helpers/AdvertDataHelpers.h):
/// what only that type offers, a divider, then what every contact offers.
/// Actions that aren't built yet show disabled
fn rows(contact_type: u8, favorite: bool) -> Vec<Row<Choice>> {
    let mut rows = match contact_type {
        1 => vec![Row::Enabled("Message", Choice::Message)],
        2 => vec![Row::Disabled("Log in"), Row::Disabled("Status")],
        3 => vec![Row::Disabled("Log in"), Row::Disabled("Read posts")],
        _ => Vec::new(),
    };
    if !rows.is_empty() {
        rows.push(Row::Divider);
    }
    rows.push(if favorite {
        Row::Enabled("Unfavorite", Choice::Favorite(false))
    } else {
        Row::Enabled("Favorite", Choice::Favorite(true))
    });
    rows.push(Row::Disabled("Delete"));
    rows
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
    pub fn new(contact: &Contact) -> Self {
        Self {
            key: contact.public_key,
            choices: Choices::new(rows(contact.contact_type, is_favorite(contact))),
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
    fn enter_asks_to_message_a_companion() {
        let app = app_with_contacts(2);
        let key = app.contacts[1].public_key;
        let mut menu = ContactMenu::new(&app.contacts[1]);
        assert_eq!(
            press(&mut menu, &app, KeyCode::Enter),
            KeyResult::Action(Action::MessageContact(key))
        );
    }

    #[test]
    fn each_type_offers_its_own_actions_then_the_shared_ones() {
        let shared = [
            Row::Divider,
            Row::Enabled("Favorite", Choice::Favorite(true)),
            Row::Disabled("Delete"),
        ];
        let with_shared = |own: &[Row<Choice>]| [own, &shared].concat();
        assert_eq!(
            rows(1, false),
            with_shared(&[Row::Enabled("Message", Choice::Message)])
        );
        assert_eq!(
            rows(2, false),
            with_shared(&[Row::Disabled("Log in"), Row::Disabled("Status")])
        );
        assert_eq!(
            rows(3, false),
            with_shared(&[Row::Disabled("Log in"), Row::Disabled("Read posts")])
        );
        // Nothing of their own, so nothing to divide
        assert_eq!(rows(4, false), shared[1..]);
        assert_eq!(rows(0, false), shared[1..]);
    }

    #[test]
    fn a_favorite_offers_to_unfavorite() {
        assert_eq!(
            rows(4, true),
            [
                Row::Enabled("Unfavorite", Choice::Favorite(false)),
                Row::Disabled("Delete"),
            ]
        );
    }

    #[test]
    fn enter_on_favorite_asks_to_mark_the_contact() {
        let app = app_with_contacts(1);
        let key = app.contacts[0].public_key;
        let mut menu = ContactMenu::new(&app.contacts[0]);
        press(&mut menu, &app, KeyCode::Down);
        assert_eq!(
            press(&mut menu, &app, KeyCode::Enter),
            KeyResult::Action(Action::SetFavorite {
                key,
                favorite: true
            })
        );
    }

    #[test]
    fn enter_on_a_stub_does_nothing() {
        let mut app = app_with_contacts(1);
        app.contacts[0].contact_type = 2;
        let mut menu = ContactMenu::new(&app.contacts[0]);
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Handled);
        assert_eq!(press(&mut menu, &app, KeyCode::Down), KeyResult::Handled);
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Handled);
    }

    #[test]
    fn a_contact_that_has_gone_offers_nothing() {
        let mut app = app_with_contacts(1);
        let mut menu = ContactMenu::new(&app.contacts[0]);
        app.contacts.clear();
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn other_keys_are_not_the_menus() {
        let app = app_with_contacts(1);
        let mut menu = ContactMenu::new(&app.contacts[0]);
        assert_eq!(press(&mut menu, &app, KeyCode::Tab), KeyResult::Ignored);
        assert_eq!(press(&mut menu, &app, KeyCode::Esc), KeyResult::Ignored);
    }
}
