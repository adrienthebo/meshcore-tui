use crossterm::event::KeyEvent;
use meshcore_rs::events::{Contact, EventPayload};

use crate::app::App;
use crate::commands::{Command, Outcome};
use crate::view::Ui;

/// Something that happened, for `update` to apply to the app
pub enum Msg {
    Key(KeyEvent),
    Radio(EventPayload),
    /// The radio finished a command or send
    EffectDone(Result<Outcome, String>),
    /// The stats poll interval elapsed
    StatsPollDue,
}

/// Who an outgoing message is addressed to
#[derive(Debug)]
pub enum Recipient {
    Contact(Contact),
    Channel(u8),
}

/// Work outside the app state that `update` asks the main loop to carry out
#[derive(Debug)]
pub enum Effect {
    /// Get the user's attention because a chat message arrived
    Alert,
    /// Run a slash command on the radio
    Radio(Command),
    /// Send a chat message
    Send { to: Recipient, text: String },
    /// Read the radio's stats
    PollRadioStats,
    /// Read the diagnostics the Radio tab shows: core and packet stats, battery and clock
    PollDiagnostics,
}

/// A change to the App that a view asks for in response to a key
#[derive(Debug, PartialEq)]
pub enum Action {
    Quit,
    /// Send a chat message or run a slash command
    Submit(String),
    /// Address messages to the contact with this public key
    SelectContact([u8; 32]),
    /// Address messages to the contact with this public key and show the Overview to write them
    MessageContact([u8; 32]),
    /// Add the discovered node with this public key as a contact
    AddDiscovered([u8; 32]),
    /// Mark the contact with this public key as a favorite, or unmark it
    SetFavorite {
        key: [u8; 32],
        favorite: bool,
    },
    /// Delete the contact with this public key
    DeleteContact([u8; 32]),
}

/// Apply a message to the app and what's on screen; returns the effects it calls for
pub fn update(app: &mut App, ui: &mut Ui, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Key(key) => match ui.handle_key(key, app) {
            Some(action) => apply_action(app, ui, action),
            None => Vec::new(),
        },
        Msg::Radio(payload) => {
            if app.apply_radio_event(payload) {
                vec![Effect::Alert]
            } else {
                Vec::new()
            }
        }
        Msg::EffectDone(outcome) => {
            app.apply_outcome(outcome);
            Vec::new()
        }
        Msg::StatsPollDue => {
            let mut effects: Vec<Effect> = app.poll_radio_stats().into_iter().collect();
            if ui.active_tab().shows_diagnostics() {
                effects.extend(app.poll_diagnostics());
            }
            effects
        }
    }
}

fn apply_action(app: &mut App, ui: &mut Ui, action: Action) -> Vec<Effect> {
    match action {
        Action::Quit => app.should_quit = true,
        Action::Submit(line) => return app.submit(&line).into_iter().collect(),
        Action::SelectContact(key) => {
            app.select_contact(key);
        }
        Action::MessageContact(key) => {
            if app.select_contact(key) {
                ui.show_overview();
            } else {
                app.err("That contact is no longer in the list");
            }
        }
        Action::AddDiscovered(key) => return app.add_discovered(&key).into_iter().collect(),
        Action::SetFavorite { key, favorite } => {
            return app.set_favorite(key, favorite).into_iter().collect()
        }
        Action::DeleteContact(key) => return app.delete_contact(key).into_iter().collect(),
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn chat_message_alerts() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        let msg = Msg::Radio(EventPayload::ChannelMessage(channel_message(0, "Bob: hi")));
        assert!(matches!(
            update(&mut app, &mut ui, msg)[..],
            [Effect::Alert]
        ));
        assert_eq!(last_message(&app), "hi");
    }

    #[test]
    fn other_radio_events_do_not_alert() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        assert!(update(
            &mut app,
            &mut ui,
            Msg::Radio(EventPayload::Ack { tag: [0; 4] })
        )
        .is_empty());
        assert_eq!(app.packets.len(), 1);
    }

    #[test]
    fn finished_radio_work_is_applied() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        assert!(update(&mut app, &mut ui, Msg::EffectDone(Ok(Outcome::AdvertSent))).is_empty());
        assert_eq!(last_message(&app), "Advertisement sent");
    }

    #[test]
    fn stats_poll_asks_the_radio_only_when_connected_and_idle() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        assert!(update(&mut app, &mut ui, Msg::StatsPollDue).is_empty());
        app.connected = true;
        assert!(matches!(
            update(&mut app, &mut ui, Msg::StatsPollDue)[..],
            [Effect::PollRadioStats]
        ));
        assert!(update(&mut app, &mut ui, Msg::StatsPollDue).is_empty());
        update(
            &mut app,
            &mut ui,
            Msg::EffectDone(Ok(Outcome::RadioStats(Ok(radio_stats(-110))))),
        );
        assert!(matches!(
            update(&mut app, &mut ui, Msg::StatsPollDue)[..],
            [Effect::PollRadioStats]
        ));
    }

    #[test]
    fn diagnostics_are_polled_only_while_the_radio_tab_shows() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        app.connected = true;
        assert!(matches!(
            update(&mut app, &mut ui, Msg::StatsPollDue)[..],
            [Effect::PollRadioStats]
        ));
        update(
            &mut app,
            &mut ui,
            Msg::EffectDone(Ok(Outcome::RadioStats(Ok(radio_stats(-110))))),
        );
        ui.active = ui
            .tabs
            .iter()
            .position(|tab| tab.title() == "Radio")
            .unwrap();
        let effects = update(&mut app, &mut ui, Msg::StatsPollDue);
        assert!(matches!(
            effects[..],
            [Effect::PollRadioStats, Effect::PollDiagnostics]
        ));
        // A diagnostics poll still in flight isn't sent again
        update(
            &mut app,
            &mut ui,
            Msg::EffectDone(Ok(Outcome::RadioStats(Ok(radio_stats(-110))))),
        );
        assert!(matches!(
            update(&mut app, &mut ui, Msg::StatsPollDue)[..],
            [Effect::PollRadioStats]
        ));
    }

    #[test]
    fn enter_submits_the_input_line() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        app.connected = true;
        assert!(update(&mut app, &mut ui, key(KeyCode::Char('h'))).is_empty());
        assert!(update(&mut app, &mut ui, key(KeyCode::Char('i'))).is_empty());
        let effects = update(&mut app, &mut ui, key(KeyCode::Enter));
        assert!(matches!(&effects[..], [Effect::Send { text, .. }] if text == "hi"));
        // The line was cleared, so Enter has nothing more to send
        assert!(update(&mut app, &mut ui, key(KeyCode::Enter)).is_empty());
    }

    #[test]
    fn esc_quits() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        assert!(update(&mut app, &mut ui, key(KeyCode::Esc)).is_empty());
        assert!(app.should_quit);
    }

    #[test]
    fn select_contact_changes_the_selection() {
        let (mut app, mut ui) = (app_with_contacts(2), test_ui());
        let key = app.contacts[1].public_key;
        apply_action(&mut app, &mut ui, Action::SelectContact(key));
        assert_eq!(app.selected_index(), Some(1));
        apply_action(&mut app, &mut ui, Action::SelectContact([4; 32]));
        assert_eq!(app.selected_index(), Some(1));
    }

    #[test]
    fn message_contact_selects_it_and_shows_the_overview() {
        let (mut app, mut ui) = (app_with_contacts(3), test_ui());
        ui.active = 1;
        let key = app.contacts[2].public_key;
        apply_action(&mut app, &mut ui, Action::MessageContact(key));
        assert_eq!(app.selected_index(), Some(2));
        assert_eq!(ui.active_tab().title(), "Overview");
    }

    #[test]
    fn message_contact_that_has_gone_stays_put() {
        let (mut app, mut ui) = (app_with_contacts(1), test_ui());
        ui.active = 1;
        apply_action(&mut app, &mut ui, Action::MessageContact([9; 32]));
        assert_eq!(app.selected_key, None);
        assert_eq!(ui.active, 1);
        assert_eq!(last_message(&app), "That contact is no longer in the list");
    }

    #[test]
    fn add_discovered_asks_the_radio_to_add_it() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        app.connected = true;
        let bob = contact("Bob", [0xd4; 6]);
        app.apply_radio_event(EventPayload::Contact(bob.clone()));
        assert!(matches!(
            apply_action(&mut app, &mut ui, Action::AddDiscovered(bob.public_key))[..],
            [Effect::Radio(Command::AddContact(_))]
        ));
    }

    #[test]
    fn set_favorite_asks_the_radio_once_connected() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        let action = || Action::SetFavorite {
            key: [7; 32],
            favorite: true,
        };
        assert!(apply_action(&mut app, &mut ui, action()).is_empty());
        app.connected = true;
        assert!(matches!(
            apply_action(&mut app, &mut ui, action())[..],
            [Effect::Radio(Command::SetFavorite {
                public_key: [7, ..],
                favorite: true
            })]
        ));
    }

    #[test]
    fn delete_contact_asks_the_radio_to_remove_it_by_key() {
        let (mut app, mut ui) = (app_with_contacts(2), test_ui());
        let key = app.contacts[1].public_key;
        assert!(apply_action(&mut app, &mut ui, Action::DeleteContact(key)).is_empty());
        assert_eq!(last_message(&app), "Not connected");
        app.connected = true;
        let effects = apply_action(&mut app, &mut ui, Action::DeleteContact(key));
        assert!(matches!(
            &effects[..],
            [Effect::Radio(Command::RemoveContact { name, public_key })]
                if *name == app.contacts[1].adv_name && *public_key == key
        ));
    }

    #[test]
    fn delete_contact_that_has_gone_says_so() {
        let (mut app, mut ui) = (app_with_contacts(1), test_ui());
        app.connected = true;
        assert!(apply_action(&mut app, &mut ui, Action::DeleteContact([9; 32])).is_empty());
        assert_eq!(last_message(&app), "That contact is no longer in the list");
    }
}
