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
}

/// A change to the App that a view asks for in response to a key
#[derive(Debug, PartialEq)]
pub enum Action {
    Quit,
    /// Send a chat message or run a slash command
    Submit(String),
    /// Address messages to the contact at this index
    SelectContact(usize),
}

/// Apply a message to the app and what's on screen; returns the effects it calls for
pub fn update(app: &mut App, ui: &mut Ui, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Key(key) => match ui.handle_key(key, app) {
            Some(action) => apply_action(app, action),
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
        Msg::StatsPollDue => app.poll_radio_stats().into_iter().collect(),
    }
}

fn apply_action(app: &mut App, action: Action) -> Vec<Effect> {
    match action {
        Action::Quit => app.should_quit = true,
        Action::Submit(line) => return app.submit(&line).into_iter().collect(),
        Action::SelectContact(idx) => app.select_contact(idx),
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
        assert!(matches!(update(&mut app, &mut ui, msg)[..], [Effect::Alert]));
        assert_eq!(last_message(&app), "hi");
    }

    #[test]
    fn other_radio_events_do_not_alert() {
        let (mut app, mut ui) = (App::new(0), test_ui());
        assert!(update(&mut app, &mut ui, Msg::Radio(EventPayload::Ack { tag: [0; 4] })).is_empty());
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
        assert!(matches!(update(&mut app, &mut ui, Msg::StatsPollDue)[..], [Effect::PollRadioStats]));
        assert!(update(&mut app, &mut ui, Msg::StatsPollDue).is_empty());
        update(&mut app, &mut ui, Msg::EffectDone(Ok(Outcome::RadioStats(Ok(radio_stats(-110))))));
        assert!(matches!(update(&mut app, &mut ui, Msg::StatsPollDue)[..], [Effect::PollRadioStats]));
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
        let mut app = app_with_contacts(2);
        apply_action(&mut app, Action::SelectContact(1));
        assert_eq!(app.selected_index(), Some(1));
        apply_action(&mut app, Action::SelectContact(5));
        assert_eq!(app.selected_key, None);
    }
}
