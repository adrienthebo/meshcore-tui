use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::events::Contact;
use ratatui::{layout::Rect, widgets::Paragraph, Frame};

use crate::app::{is_favorite, App};
use crate::choices::{Choices, ChoicesKey, Row};
use crate::commands::MAX_PASSWORD_LEN;
use crate::input::InputLine;
use crate::popover::Popover;
use crate::text::sanitize;
use crate::theme::Theme;
use crate::update::Action;
use crate::view::{KeyResult, View};

/// Something to do with a contact, as the menu offers it
#[derive(Debug, Clone, Copy, PartialEq)]
enum Choice {
    Message,
    /// Ask for the password to log in to a repeater or room server
    LogIn,
    /// Show the repeater on the Repeaters tab
    Status,
    /// Mark the contact as a favorite (true) or unmark it
    Favorite(bool),
    /// Ask whether to delete the contact
    Delete,
}

/// The answer to whether to delete the contact
#[derive(Debug, Clone, Copy, PartialEq)]
enum Confirm {
    Cancel,
    Delete,
}

/// Asks whether to delete; Cancel comes first, so it's highlighted
fn confirm_rows() -> Vec<Row<Confirm>> {
    vec![
        Row::Enabled("Cancel", Confirm::Cancel),
        Row::Enabled("Delete", Confirm::Delete),
    ]
}

/// The menu for a contact of this type (ADV_TYPE_*, helpers/AdvertDataHelpers.h):
/// what only that type offers, a divider, then what every contact offers.
/// Actions that aren't built yet show disabled, and so does a repeater's
/// Status until we're logged in to it
fn rows(contact_type: u8, favorite: bool, logged_in: bool) -> Vec<Row<Choice>> {
    let mut rows = match contact_type {
        1 => vec![Row::Enabled("Message", Choice::Message)],
        2 => vec![
            Row::Enabled("Log in", Choice::LogIn),
            if logged_in {
                Row::Enabled("Status", Choice::Status)
            } else {
                Row::Disabled("Status")
            },
        ],
        3 => vec![
            Row::Enabled("Log in", Choice::LogIn),
            Row::Disabled("Read posts"),
        ],
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
    rows.push(Row::Enabled("Delete", Choice::Delete));
    rows
}

/// Choices in a popover with this title, centered in `area`
fn draw_choices<T: Copy>(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: String,
    choices: &Choices<T>,
) {
    let inner = Popover::new(title, choices.width(), choices.height()).render(f, area, theme);
    choices.draw(f, inner, theme);
}

/// The things to do with one contact, drawn as a popover; ↑↓ choose and Enter
/// asks for the chosen one. Delete asks first, Log in asks for the password,
/// and Esc goes back from asking
#[derive(Debug)]
pub struct ContactMenu {
    /// The contact's public key; the menu draws nothing and chooses nothing
    /// while it isn't in the contact list
    key: [u8; 32],
    choices: Choices<Choice>,
    /// Whether to delete the contact, while the menu is asking
    confirm: Option<Choices<Confirm>>,
    /// The login password, while the menu is asking for it
    password: Option<InputLine>,
}

impl ContactMenu {
    pub fn new(contact: &Contact, app: &App) -> Self {
        let logged_in = app.logins.contains_key(&contact.public_key);
        Self {
            key: contact.public_key,
            choices: Choices::new(rows(contact.contact_type, is_favorite(contact), logged_in)),
            confirm: None,
            password: None,
        }
    }

    /// What choosing does: ask for an action, ask whether to delete, or ask
    /// for the password
    fn choose(&mut self, choice: Choice) -> KeyResult {
        let key = self.key;
        match choice {
            Choice::Message => KeyResult::Action(Action::MessageContact(key)),
            Choice::LogIn => {
                self.password = Some(InputLine::default());
                KeyResult::Handled
            }
            Choice::Status => KeyResult::Action(Action::ShowRepeater(key)),
            Choice::Favorite(favorite) => KeyResult::Action(Action::SetFavorite { key, favorite }),
            Choice::Delete => {
                self.confirm = Some(Choices::new(confirm_rows()));
                KeyResult::Handled
            }
        }
    }

    /// Keys while asking whether to delete; Esc and Cancel go back to the menu
    fn handle_confirm_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        let Some(confirm) = &mut self.confirm else {
            return KeyResult::Ignored;
        };
        if key.code == KeyCode::Esc {
            self.confirm = None;
            return KeyResult::Handled;
        }
        match confirm.handle_key(key) {
            ChoicesKey::Ignored => KeyResult::Ignored,
            ChoicesKey::Handled => KeyResult::Handled,
            ChoicesKey::Chosen(_) if self.contact(app).is_none() => KeyResult::Ignored,
            ChoicesKey::Chosen(Confirm::Cancel) => {
                self.confirm = None;
                KeyResult::Handled
            }
            ChoicesKey::Chosen(Confirm::Delete) => {
                KeyResult::Action(Action::DeleteContact(self.key))
            }
        }
    }

    /// Keys while asking for the password: edit it, refusing chars past what
    /// the firmware sends; Enter logs in, an empty password as guest, and Esc
    /// goes back to the menu
    fn handle_password_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        let gone = self.contact(app).is_none();
        let Some(password) = &mut self.password else {
            return KeyResult::Ignored;
        };
        match key.code {
            KeyCode::Esc => self.password = None,
            KeyCode::Enter if gone => return KeyResult::Ignored,
            KeyCode::Enter => {
                return KeyResult::Action(Action::Login {
                    key: self.key,
                    password: password.take(),
                })
            }
            KeyCode::Char(c) if password.as_str().len() + c.len_utf8() > MAX_PASSWORD_LEN => {}
            KeyCode::Char(c) => password.insert(c),
            KeyCode::Backspace => password.backspace(),
            KeyCode::Left => password.left(),
            KeyCode::Right => password.right(),
            KeyCode::Home => password.home(),
            KeyCode::End => password.end(),
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
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

    /// A popover titled with the contact's name and key prefix, centered in
    /// `area`; while asking whether to delete or for the password, the title
    /// asks instead. The password shows as one • per char
    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        let Some(contact) = self.contact(app) else {
            return;
        };
        let name = format!(
            "{} [{}]",
            sanitize(&contact.adv_name),
            &contact.prefix_hex()[..8]
        );
        if let Some(password) = &self.password {
            let title = format!(" Log in to {name} ");
            let inner = Popover::new(title, MAX_PASSWORD_LEN as u16, 1).render(f, area, theme);
            let masked = "•".repeat(password.as_str().chars().count());
            f.render_widget(Paragraph::new(masked).style(theme.text), inner);
            if focused {
                f.set_cursor_position((inner.x + password.cursor() as u16, inner.y));
            }
            return;
        }
        match &self.confirm {
            Some(confirm) => draw_choices(f, area, theme, format!(" Delete {name}? "), confirm),
            None => draw_choices(f, area, theme, format!(" {name} "), &self.choices),
        }
    }

    /// ↑↓ choose and Enter selects; Esc goes back while asking whether to
    /// delete or for the password, and otherwise isn't the menu's
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        if self.password.is_some() {
            return self.handle_password_key(key, app);
        }
        if self.confirm.is_some() {
            return self.handle_confirm_key(key, app);
        }
        match self.choices.handle_key(key) {
            ChoicesKey::Ignored => KeyResult::Ignored,
            ChoicesKey::Handled => KeyResult::Handled,
            ChoicesKey::Chosen(_) if self.contact(app).is_none() => KeyResult::Ignored,
            ChoicesKey::Chosen(choice) => self.choose(choice),
        }
    }

    fn key_hints(&self) -> Vec<&'static str> {
        if self.password.is_some() {
            return vec!["Enter=log in", "Esc=back"];
        }
        let esc = match self.confirm {
            Some(_) => "Esc=back",
            None => "Esc=close",
        };
        vec!["↑↓=choose", "Enter=select", esc]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Role;
    use crate::test_support::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn press(menu: &mut ContactMenu, app: &App, code: KeyCode) -> KeyResult {
        menu.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app)
    }

    #[test]
    fn enter_asks_to_message_a_companion() {
        let app = app_with_contacts(2);
        let key = app.contacts[1].public_key;
        let mut menu = ContactMenu::new(&app.contacts[1], &app);
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
            Row::Enabled("Delete", Choice::Delete),
        ];
        let with_shared = |own: &[Row<Choice>]| [own, &shared].concat();
        assert_eq!(
            rows(1, false, false),
            with_shared(&[Row::Enabled("Message", Choice::Message)])
        );
        assert_eq!(
            rows(2, false, false),
            with_shared(&[
                Row::Enabled("Log in", Choice::LogIn),
                Row::Disabled("Status")
            ])
        );
        assert_eq!(
            rows(3, false, false),
            with_shared(&[
                Row::Enabled("Log in", Choice::LogIn),
                Row::Disabled("Read posts")
            ])
        );
        // Nothing of their own, so nothing to divide
        assert_eq!(rows(4, false, false), shared[1..]);
        assert_eq!(rows(0, false, false), shared[1..]);
    }

    #[test]
    fn a_repeater_logged_in_to_offers_its_status() {
        let mut app = app_with_contacts(1);
        app.contacts[0].contact_type = 2;
        let key = app.contacts[0].public_key;
        app.logins.insert(key, Role::Guest);
        let mut menu = ContactMenu::new(&app.contacts[0], &app);
        press(&mut menu, &app, KeyCode::Down);
        assert_eq!(
            press(&mut menu, &app, KeyCode::Enter),
            KeyResult::Action(Action::ShowRepeater(key))
        );
    }

    #[test]
    fn a_favorite_offers_to_unfavorite() {
        assert_eq!(
            rows(4, true, false),
            [
                Row::Enabled("Unfavorite", Choice::Favorite(false)),
                Row::Enabled("Delete", Choice::Delete),
            ]
        );
    }

    /// A menu for a lone companion, asking whether to delete it
    fn asking_to_delete(app: &App) -> ContactMenu {
        let mut menu = ContactMenu::new(&app.contacts[0], app);
        press(&mut menu, app, KeyCode::Down);
        press(&mut menu, app, KeyCode::Down);
        assert_eq!(press(&mut menu, app, KeyCode::Enter), KeyResult::Handled);
        menu
    }

    #[test]
    fn delete_asks_first_and_deletes_only_when_confirmed() {
        let app = app_with_contacts(1);
        let key = app.contacts[0].public_key;
        let mut menu = asking_to_delete(&app);
        assert_eq!(menu.key_hints(), ["↑↓=choose", "Enter=select", "Esc=back"]);
        press(&mut menu, &app, KeyCode::Down);
        assert_eq!(
            press(&mut menu, &app, KeyCode::Enter),
            KeyResult::Action(Action::DeleteContact(key))
        );
    }

    #[test]
    fn asking_to_delete_highlights_cancel_which_goes_back_to_the_menu() {
        let app = app_with_contacts(1);
        let mut menu = asking_to_delete(&app);
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Handled);
        assert!(menu.confirm.is_none());
        assert_eq!(menu.key_hints(), ["↑↓=choose", "Enter=select", "Esc=close"]);
    }

    #[test]
    fn esc_while_asking_to_delete_goes_back_to_the_menu() {
        let app = app_with_contacts(1);
        let mut menu = asking_to_delete(&app);
        assert_eq!(press(&mut menu, &app, KeyCode::Esc), KeyResult::Handled);
        assert!(menu.confirm.is_none());
        // Back in the menu, the highlight is still on Delete
        press(&mut menu, &app, KeyCode::Enter);
        assert!(menu.confirm.is_some());
    }

    #[test]
    fn a_contact_that_has_gone_cant_be_deleted() {
        let mut app = app_with_contacts(1);
        let mut menu = asking_to_delete(&app);
        app.contacts.clear();
        press(&mut menu, &app, KeyCode::Down);
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn enter_on_favorite_asks_to_mark_the_contact() {
        let app = app_with_contacts(1);
        let key = app.contacts[0].public_key;
        let mut menu = ContactMenu::new(&app.contacts[0], &app);
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
        let mut menu = ContactMenu::new(&app.contacts[0], &app);
        assert_eq!(press(&mut menu, &app, KeyCode::Down), KeyResult::Handled);
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Handled);
        assert!(menu.password.is_none());
    }

    /// A menu for a lone repeater, asking for the password
    fn asking_for_password(app: &mut App) -> ContactMenu {
        app.contacts[0].contact_type = 2;
        let mut menu = ContactMenu::new(&app.contacts[0], app);
        assert_eq!(press(&mut menu, app, KeyCode::Enter), KeyResult::Handled);
        assert_eq!(menu.key_hints(), ["Enter=log in", "Esc=back"]);
        menu
    }

    #[test]
    fn log_in_asks_for_the_password_and_sends_it() {
        let mut app = app_with_contacts(1);
        let key = app.contacts[0].public_key;
        let mut menu = asking_for_password(&mut app);
        for c in "hunter2".chars() {
            press(&mut menu, &app, KeyCode::Char(c));
        }
        press(&mut menu, &app, KeyCode::Backspace);
        assert_eq!(
            press(&mut menu, &app, KeyCode::Enter),
            KeyResult::Action(Action::Login {
                key,
                password: "hunter".to_string()
            })
        );
    }

    #[test]
    fn an_empty_password_logs_in_as_guest() {
        let mut app = app_with_contacts(1);
        let key = app.contacts[0].public_key;
        let mut menu = asking_for_password(&mut app);
        assert_eq!(
            press(&mut menu, &app, KeyCode::Enter),
            KeyResult::Action(Action::Login {
                key,
                password: String::new()
            })
        );
    }

    #[test]
    fn the_password_stops_at_what_the_firmware_sends() {
        let mut app = app_with_contacts(1);
        let mut menu = asking_for_password(&mut app);
        for _ in 0..MAX_PASSWORD_LEN + 3 {
            press(&mut menu, &app, KeyCode::Char('x'));
        }
        assert_eq!(
            menu.password.as_ref().map(|p| p.as_str().len()),
            Some(MAX_PASSWORD_LEN)
        );
    }

    #[test]
    fn esc_while_asking_for_the_password_goes_back_to_the_menu() {
        let mut app = app_with_contacts(1);
        let mut menu = asking_for_password(&mut app);
        assert_eq!(press(&mut menu, &app, KeyCode::Esc), KeyResult::Handled);
        assert!(menu.password.is_none());
        assert_eq!(menu.key_hints(), ["↑↓=choose", "Enter=select", "Esc=close"]);
    }

    #[test]
    fn a_server_that_has_gone_cant_be_logged_in_to() {
        let mut app = app_with_contacts(1);
        let mut menu = asking_for_password(&mut app);
        app.contacts.clear();
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn a_contact_that_has_gone_offers_nothing() {
        let mut app = app_with_contacts(1);
        let mut menu = ContactMenu::new(&app.contacts[0], &app);
        app.contacts.clear();
        assert_eq!(press(&mut menu, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn other_keys_are_not_the_menus() {
        let app = app_with_contacts(1);
        let mut menu = ContactMenu::new(&app.contacts[0], &app);
        assert_eq!(press(&mut menu, &app, KeyCode::Tab), KeyResult::Ignored);
        assert_eq!(press(&mut menu, &app, KeyCode::Esc), KeyResult::Ignored);
    }
}
