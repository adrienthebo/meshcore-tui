use crossterm::event::KeyEvent;
use meshcore_rs::events::{Contact, EventPayload};

use crate::app::App;
use crate::commands::Command;

/// Something that happened, for `update` to apply to the app
pub enum Msg {
    Key(KeyEvent),
    Radio(EventPayload),
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
}

/// Apply a message to the app; returns the effects it calls for
pub fn update(app: &mut App, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Key(key) => app.handle_key(key).and_then(|line| app.submit(&line)).into_iter().collect(),
        Msg::Radio(payload) => {
            if app.apply_radio_event(payload) {
                vec![Effect::Alert]
            } else {
                Vec::new()
            }
        }
    }
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
        let mut app = App::new(0);
        let msg = Msg::Radio(EventPayload::ChannelMessage(channel_message(0, "Bob: hi")));
        assert!(matches!(update(&mut app, msg)[..], [Effect::Alert]));
        assert_eq!(last_message(&app), "hi");
    }

    #[test]
    fn other_radio_events_do_not_alert() {
        let mut app = App::new(0);
        assert!(update(&mut app, Msg::Radio(EventPayload::Ack { tag: [0; 4] })).is_empty());
        assert_eq!(app.packets.len(), 1);
    }

    #[test]
    fn enter_submits_the_input_line() {
        let mut app = App::new(0);
        app.connected = true;
        assert!(update(&mut app, key(KeyCode::Char('h'))).is_empty());
        assert!(update(&mut app, key(KeyCode::Char('i'))).is_empty());
        let effects = update(&mut app, key(KeyCode::Enter));
        assert!(matches!(&effects[..], [Effect::Send { text, .. }] if text == "hi"));
        assert_eq!(app.input.as_str(), "");
    }
}
