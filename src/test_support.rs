use meshcore_rs::events::Contact;

use crate::app::{App, ChannelEntry};

pub fn contact(name: &str, key_prefix: [u8; 6]) -> Contact {
    let mut public_key = [0u8; 32];
    public_key[..6].copy_from_slice(&key_prefix);
    Contact {
        public_key,
        contact_type: 1,
        flags: 0,
        path_len: 0,
        out_path: Vec::new(),
        adv_name: name.to_string(),
        last_advert: 0,
        adv_lat: 0,
        adv_lon: 0,
        last_modification_timestamp: 0,
    }
}

pub fn channel(name: &str) -> ChannelEntry {
    ChannelEntry {
        name: name.to_string(),
        secret: [0u8; 16],
    }
}

pub fn app_with_contacts(n: u8) -> App {
    let mut app = App::new(0);
    for i in 0..n {
        app.contacts.push(contact(&format!("c{}", i), [i + 1; 6]));
    }
    app
}

pub fn last_message(app: &App) -> &str {
    &app.messages.last().expect("no messages").text
}
