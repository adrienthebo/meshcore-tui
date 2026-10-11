use std::collections::HashMap;

use chrono::Local;
use meshcore_rs::events::{
    AdvertisementData, ChannelInfoData, ChannelMessage, Contact, ContactMessage, EventPayload, MsgSentInfo,
    PathUpdateData, SelfInfo,
};

use crate::input::InputLine;
use crate::text::sanitize;

/// Stored channel info
#[derive(Clone, Debug)]
pub struct ChannelEntry {
    pub name: String,
    pub secret: [u8; 16],
}

// ── Data ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub timestamp: String,
    pub sender: String,
    pub text: String,
    pub msg_type: MsgType,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MsgType {
    Incoming,
    Outgoing,
    Channel(u8),
    System,
    Error,
}

#[derive(Clone, Debug)]
pub struct PacketLogEntry {
    pub timestamp: String,
    pub ptype: String,
    pub hops: u8,
    pub rssi: i16,
    pub snr: f32,
    pub info: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Panel {
    Messages,
    Contacts,
    Packets,
}

pub struct App {
    pub input: InputLine,
    pub panel: Panel,
    pub messages: Vec<ChatMessage>,
    pub contacts: Vec<Contact>,
    pub channels: HashMap<u8, ChannelEntry>,
    /// Maps firmware channel_idx → local channel index for received message lookup
    pub fw_channel_map: HashMap<u8, u8>,
    pub packets: Vec<PacketLogEntry>,
    /// Public key of the contact messages go to; None sends to the active channel
    pub selected_key: Option<[u8; 32]>,
    pub self_info: Option<SelfInfo>,
    pub battery_level: Option<u16>,
    pub active_channel: u8,
    pub port_info: String,
    pub rx_count: u32,
    pub tx_count: u32,
    pub connected: bool,
    pub should_quit: bool,
}

impl App {
    pub fn new(channel: u8) -> Self {
        Self {
            input: InputLine::default(),
            panel: Panel::Messages,
            messages: Vec::new(),
            contacts: Vec::new(),
            channels: HashMap::new(),
            fw_channel_map: HashMap::new(),
            packets: Vec::new(),
            selected_key: None,
            self_info: None,
            battery_level: None,
            active_channel: channel,
            port_info: String::new(),
            rx_count: 0,
            tx_count: 0,
            connected: false,
            should_quit: false,
        }
    }

    /// Append a chat message, dropping the oldest beyond MAX_MESSAGES
    fn push_message(&mut self, sender: String, text: String, msg_type: MsgType) {
        self.messages.push(ChatMessage {
            timestamp: now(),
            sender,
            text,
            msg_type,
        });
        if self.messages.len() > MAX_MESSAGES {
            let excess = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..excess);
        }
    }

    /// Append a packet log entry, dropping the oldest beyond MAX_PACKETS
    fn push_packet(&mut self, ptype: &str, hops: u8, info: String) {
        self.packets.push(PacketLogEntry {
            timestamp: now(),
            ptype: ptype.into(),
            hops,
            rssi: 0,
            snr: 0.0,
            info,
        });
        if self.packets.len() > MAX_PACKETS {
            let excess = self.packets.len() - MAX_PACKETS;
            self.packets.drain(..excess);
        }
    }

    pub fn sys(&mut self, text: &str) {
        self.push_message("SYS".into(), text.into(), MsgType::System);
    }

    pub fn err(&mut self, text: &str) {
        self.push_message("ERR".into(), text.into(), MsgType::Error);
    }

    // ── Radio events ─────────────────────────────────────────────────────────

    /// Apply a radio event; returns true when a chat message arrived
    pub fn apply_radio_event(&mut self, payload: EventPayload) -> bool {
        match payload {
            EventPayload::ContactMessage(msg) => {
                self.on_contact_message(&msg);
                true
            }
            EventPayload::ChannelMessage(msg) => {
                self.on_channel_message(&msg);
                true
            }
            EventPayload::Advertisement(adv) => {
                self.on_advertisement(&adv);
                false
            }
            EventPayload::Contact(contact) => {
                self.on_new_contact(contact);
                false
            }
            EventPayload::Ack { tag } => {
                self.on_ack(&tag);
                false
            }
            EventPayload::PathUpdate(pu) => {
                self.on_path_update(&pu);
                false
            }
            // meshcore-rs defines many payloads the app doesn't subscribe to
            _ => false,
        }
    }

    fn on_contact_message(&mut self, msg: &ContactMessage) {
        let sender = self.resolve_sender(&msg.sender_prefix);
        self.rx_count += 1;
        self.push_message(sender, sanitize(&msg.text), MsgType::Incoming);
    }

    /// Channel messages carry the sender's name in the body as "name: text"
    fn on_channel_message(&mut self, msg: &ChannelMessage) {
        let (sender, text) = match msg.text.split_once(": ") {
            Some((name, text)) => (sanitize(name), text),
            None => ("?".to_string(), msg.text.as_str()),
        };
        self.rx_count += 1;
        self.push_message(sender, sanitize(text), MsgType::Channel(msg.channel_idx));
    }

    /// A known contact advertised again; the push carries only their key
    fn on_advertisement(&mut self, adv: &AdvertisementData) {
        let who = self.resolve_key(&adv.public_key);
        self.push_packet("ADV", 0, who);
    }

    fn on_new_contact(&mut self, contact: Contact) {
        let info = format!(
            "{} [{}] @{:.4},{:.4}",
            sanitize(&contact.adv_name),
            contact.prefix_hex(),
            contact.latitude(),
            contact.longitude(),
        );
        self.push_packet("ADV", hop_count(contact.path_len), info);
        self.contacts.retain(|c| c.prefix() != contact.prefix());
        self.contacts.push(contact);
    }

    fn on_ack(&mut self, tag: &[u8; 4]) {
        self.push_packet("ACK", 0, format!("tag:{}", meshcore_rs::parsing::hex_encode(tag)));
    }

    /// The radio found a new route to a contact; the push carries only their key
    fn on_path_update(&mut self, pu: &PathUpdateData) {
        let who = self.resolve_key(&pu.public_key);
        self.push_packet("PATH", 0, who);
    }

    /// Name the contact with this public key, falling back to its prefix in hex
    fn resolve_key(&self, public_key: &[u8; 32]) -> String {
        let mut prefix = [0u8; 6];
        prefix.copy_from_slice(&public_key[..6]);
        self.resolve_sender(&prefix)
    }

    pub fn on_direct_sent(&mut self, contact: &Contact, text: &str, info: &MsgSentInfo) {
        self.tx_count += 1;
        let sender = format!("\u{2192} {}", sanitize(&contact.adv_name));
        self.push_message(sender, text.into(), MsgType::Outgoing);
        let packet_info = format!(
            "ack:{} timeout:{}ms",
            meshcore_rs::parsing::hex_encode(&info.expected_ack),
            info.suggested_timeout
        );
        self.push_packet("SND", 0, packet_info);
    }

    pub fn on_channel_sent(&mut self, channel: u8, text: &str) {
        self.tx_count += 1;
        let sender = format!("\u{2192} {}", self.channel_label(channel));
        self.push_message(sender, text.into(), MsgType::Outgoing);
    }

    pub fn device_name(&self) -> String {
        self.self_info
            .as_ref()
            .map(|i| sanitize(&i.name))
            .unwrap_or_else(|| "Unknown".into())
    }

    // ── Contact selection ────────────────────────────────────────────────────

    /// The contact messages go to, if one is selected
    pub fn selected_contact(&self) -> Option<&Contact> {
        self.selected_index().map(|idx| &self.contacts[idx])
    }

    /// Position of the selected contact in the contact list
    pub fn selected_index(&self) -> Option<usize> {
        let key = self.selected_key?;
        self.contacts.iter().position(|c| c.public_key == key)
    }

    /// Select the contact at `idx` in the contact list; out of range selects nothing
    pub fn select_contact(&mut self, idx: usize) {
        self.selected_key = self.contacts.get(idx).map(|c| c.public_key);
    }

    /// Replace the contact list, keeping the selection on the same contact
    pub fn set_contacts(&mut self, contacts: Vec<Contact>) {
        let selected_name = self.selected_contact().map(|c| sanitize(&c.adv_name));
        self.contacts = contacts;
        if let (Some(name), None) = (selected_name, self.selected_index()) {
            self.selected_key = None;
            self.err(&format!("Contact {} is no longer in the list; deselected", name));
        }
    }

    pub fn target_label(&self) -> String {
        self.selected_contact()
            .map(|c| sanitize(&c.adv_name))
            .unwrap_or_else(|| self.channel_label(self.active_channel))
    }

    /// Record a channel read from the radio at local index `idx`; returns its name,
    /// or None for an unnamed (unconfigured) slot, which is not stored
    pub fn store_channel(&mut self, idx: u8, info: &ChannelInfoData) -> Option<String> {
        let name = sanitize(&info.name);
        if name.is_empty() {
            return None;
        }
        self.channels.insert(idx, ChannelEntry {
            name: name.clone(),
            secret: info.secret,
        });
        self.fw_channel_map.insert(info.channel_idx, idx);
        Some(name)
    }

    pub fn channel_label(&self, ch: u8) -> String {
        // Direct lookup by local index
        if let Some(entry) = self.channels.get(&ch) {
            if !entry.name.is_empty() {
                return entry.name.clone();
            }
        }
        // Lookup via firmware channel_idx → local index mapping
        if let Some(&local_idx) = self.fw_channel_map.get(&ch) {
            if let Some(entry) = self.channels.get(&local_idx) {
                if !entry.name.is_empty() {
                    return entry.name.clone();
                }
            }
        }
        format!("CH#{}", ch)
    }

    fn resolve_sender(&self, prefix: &[u8; 6]) -> String {
        self.contacts
            .iter()
            .find(|c| &c.public_key[..6] == prefix)
            .map(|c| sanitize(&c.adv_name))
            .unwrap_or_else(|| {
                let hex = meshcore_rs::parsing::hex_encode(prefix);
                // Trim leading zeros for cleaner display
                let trimmed = hex.trim_start_matches('0');
                if trimmed.is_empty() {
                    "0".into()
                } else {
                    trimmed.to_string()
                }
            })
    }
}

const MAX_MESSAGES: usize = 1000;
const MAX_PACKETS: usize = 100;

/// Hop count for display; a negative path length means the path is unknown (flood)
fn hop_count(path_len: i8) -> u8 {
    path_len.max(0) as u8
}

fn now() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    // ── Radio events ─────────────────────────────────────────────────────────

    fn last_packet(app: &App) -> &PacketLogEntry {
        app.packets.last().expect("no packets")
    }

    #[test]
    fn apply_radio_event_reports_only_chat_messages() {
        let mut app = App::new(0);
        assert!(app.apply_radio_event(EventPayload::ContactMessage(contact_message([1; 6], "hi"))));
        assert!(app.apply_radio_event(EventPayload::ChannelMessage(channel_message(0, "Bob: hi"))));
        assert!(!app.apply_radio_event(EventPayload::Ack { tag: [0; 4] }));
        assert!(!app.apply_radio_event(EventPayload::Contact(contact("Alice", [2; 6]))));
        let adv = AdvertisementData { public_key: [3; 32] };
        assert!(!app.apply_radio_event(EventPayload::Advertisement(adv)));
        let path = PathUpdateData { public_key: [2; 32] };
        assert!(!app.apply_radio_event(EventPayload::PathUpdate(path)));
        assert!(!app.apply_radio_event(EventPayload::Time(0)));
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.packets.len(), 4);
        assert_eq!(app.contacts.len(), 1);
    }

    #[test]
    fn contact_message_resolves_sender_and_counts_rx() {
        let mut app = App::new(0);
        app.contacts.push(contact("Alice", [1; 6]));
        app.on_contact_message(&contact_message([1; 6], "hi\u{0} there"));
        let msg = app.messages.last().unwrap();
        assert_eq!(msg.sender, "Alice");
        assert_eq!(msg.text, "hi there");
        assert_eq!(msg.msg_type, MsgType::Incoming);
        assert_eq!(app.rx_count, 1);
    }

    #[test]
    fn channel_message_splits_sender_from_body() {
        let mut app = App::new(0);
        app.on_channel_message(&channel_message(3, "Bob: hello: world"));
        let msg = app.messages.last().unwrap();
        assert_eq!(msg.sender, "Bob");
        assert_eq!(msg.text, "hello: world");
        assert_eq!(msg.msg_type, MsgType::Channel(3));
        assert_eq!(app.rx_count, 1);
    }

    #[test]
    fn channel_message_without_sender_uses_placeholder() {
        let mut app = App::new(0);
        app.on_channel_message(&channel_message(0, "no separator"));
        let msg = app.messages.last().unwrap();
        assert_eq!(msg.sender, "?");
        assert_eq!(msg.text, "no separator");
    }

    #[test]
    fn advertisement_names_the_contact() {
        let mut app = App::new(0);
        let repeater = contact("Repeater", [1; 6]);
        app.on_advertisement(&AdvertisementData { public_key: repeater.public_key });
        app.contacts.push(repeater.clone());
        app.on_advertisement(&AdvertisementData { public_key: repeater.public_key });
        let infos: Vec<&str> = app.packets.iter().map(|p| p.info.as_str()).collect();
        assert_eq!(infos, vec!["10101010101", "Repeater"]);
        assert!(app.packets.iter().all(|p| p.ptype == "ADV"));
    }

    #[test]
    fn new_contact_replaces_existing_contact_with_same_prefix() {
        let mut app = App::new(0);
        app.contacts.push(contact("Old name", [1; 6]));
        app.contacts.push(contact("Other", [2; 6]));
        let mut updated = contact("New name", [1; 6]);
        updated.path_len = 2;
        app.on_new_contact(updated);
        let names: Vec<&str> = app.contacts.iter().map(|c| c.adv_name.as_str()).collect();
        assert_eq!(names, vec!["Other", "New name"]);
        assert_eq!(last_packet(&app).ptype, "ADV");
        assert_eq!(last_packet(&app).hops, 2);
    }

    #[test]
    fn new_contact_with_flood_path_logs_zero_hops() {
        let mut app = App::new(0);
        let mut flood = contact("Flood", [1; 6]);
        flood.path_len = -1;
        app.on_new_contact(flood);
        assert_eq!(last_packet(&app).hops, 0);
    }

    #[test]
    fn ack_logs_tag() {
        let mut app = App::new(0);
        app.on_ack(&[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(last_packet(&app).ptype, "ACK");
        assert_eq!(last_packet(&app).info, "tag:deadbeef");
    }

    #[test]
    fn path_update_names_the_contact() {
        let mut app = App::new(0);
        let alice = contact("Alice", [1; 6]);
        app.on_path_update(&PathUpdateData { public_key: alice.public_key });
        app.contacts.push(alice.clone());
        app.on_path_update(&PathUpdateData { public_key: alice.public_key });
        let infos: Vec<&str> = app.packets.iter().map(|p| p.info.as_str()).collect();
        assert_eq!(infos, vec!["10101010101", "Alice"]);
        assert!(app.packets.iter().all(|p| p.ptype == "PATH"));
    }

    #[test]
    fn direct_sent_logs_message_and_packet() {
        let mut app = App::new(0);
        let info = MsgSentInfo { message_type: 0, expected_ack: [1, 2, 3, 4], suggested_timeout: 5000 };
        app.on_direct_sent(&contact("Alice", [1; 6]), "yo", &info);
        let msg = app.messages.last().unwrap();
        assert_eq!(msg.sender, "\u{2192} Alice");
        assert_eq!(msg.msg_type, MsgType::Outgoing);
        assert_eq!(last_packet(&app).info, "ack:01020304 timeout:5000ms");
        assert_eq!(app.tx_count, 1);
    }

    #[test]
    fn channel_sent_logs_message_with_channel_label() {
        let mut app = App::new(0);
        app.channels.insert(2, channel("Ops"));
        app.on_channel_sent(2, "yo");
        let msg = app.messages.last().unwrap();
        assert_eq!(msg.sender, "\u{2192} Ops");
        assert_eq!(msg.text, "yo");
        assert_eq!(app.tx_count, 1);
        assert!(app.packets.is_empty());
    }

    #[test]
    fn messages_are_capped_keeping_newest() {
        let mut app = App::new(0);
        for i in 0..MAX_MESSAGES + 5 {
            app.sys(&i.to_string());
        }
        assert_eq!(app.messages.len(), MAX_MESSAGES);
        assert_eq!(app.messages[0].text, "5");
        assert_eq!(last_message(&app), (MAX_MESSAGES + 4).to_string());
    }

    #[test]
    fn packets_are_capped_including_sent_entries() {
        let mut app = App::new(0);
        let info = MsgSentInfo { message_type: 0, expected_ack: [0; 4], suggested_timeout: 0 };
        let alice = contact("Alice", [1; 6]);
        for _ in 0..MAX_PACKETS + 5 {
            app.on_direct_sent(&alice, "x", &info);
        }
        assert_eq!(app.packets.len(), MAX_PACKETS);
    }

    // ── App labels ───────────────────────────────────────────────────────────

    #[test]
    fn channel_label_uses_local_channel_name() {
        let mut app = App::new(0);
        app.channels.insert(1, channel("Local"));
        assert_eq!(app.channel_label(1), "Local");
    }

    #[test]
    fn channel_label_falls_back_to_firmware_index_mapping() {
        let mut app = App::new(0);
        app.channels.insert(1, channel("Mapped"));
        app.fw_channel_map.insert(5, 1);
        assert_eq!(app.channel_label(5), "Mapped");
    }

    #[test]
    fn channel_label_ignores_empty_names() {
        let mut app = App::new(0);
        app.channels.insert(2, channel(""));
        assert_eq!(app.channel_label(2), "CH#2");
        app.fw_channel_map.insert(5, 2);
        assert_eq!(app.channel_label(5), "CH#5");
    }

    #[test]
    fn store_channel_records_sanitized_name_and_firmware_index() {
        let mut app = App::new(0);
        let info = ChannelInfoData { channel_idx: 5, name: "Ops\u{0}".to_string(), secret: [7; 16] };
        assert_eq!(app.store_channel(1, &info), Some("Ops".to_string()));
        assert_eq!(app.channels[&1].secret, [7; 16]);
        assert_eq!(app.channel_label(5), "Ops");
    }

    #[test]
    fn store_channel_skips_unnamed_slots() {
        let mut app = App::new(0);
        let info = ChannelInfoData { channel_idx: 5, name: "\u{0}".to_string(), secret: [7; 16] };
        assert_eq!(app.store_channel(1, &info), None);
        assert!(app.channels.is_empty());
        assert!(app.fw_channel_map.is_empty());
    }

    #[test]
    fn channel_label_defaults_to_index() {
        assert_eq!(App::new(0).channel_label(7), "CH#7");
    }

    #[test]
    fn resolve_sender_finds_known_contact() {
        let mut app = App::new(0);
        app.contacts.push(contact("Alice 😀", [1, 2, 3, 4, 5, 6]));
        assert_eq!(app.resolve_sender(&[1, 2, 3, 4, 5, 6]), "Alice");
    }

    #[test]
    fn resolve_sender_unknown_prefix_is_hex_without_leading_zeros() {
        let app = App::new(0);
        assert_eq!(app.resolve_sender(&[0, 0x0a, 0xbc, 0, 0, 1]), "abc000001");
    }

    #[test]
    fn resolve_sender_all_zero_prefix_is_zero() {
        assert_eq!(App::new(0).resolve_sender(&[0; 6]), "0");
    }

    #[test]
    fn target_label_prefers_selected_contact() {
        let mut app = App::new(0);
        app.contacts.push(contact("Bob", [9; 6]));
        app.select_contact(0);
        assert_eq!(app.target_label(), "Bob");
    }

    #[test]
    fn target_label_without_selection_is_active_channel() {
        let mut app = App::new(3);
        app.channels.insert(3, channel("Ops"));
        assert_eq!(app.target_label(), "Ops");
    }

    #[test]
    fn target_label_with_stale_selection_is_active_channel() {
        let mut app = App::new(0);
        app.selected_key = Some([4; 32]);
        assert_eq!(app.target_label(), "CH#0");
    }

    #[test]
    fn select_contact_out_of_range_selects_nothing() {
        let mut app = app_with_contacts(1);
        app.select_contact(0);
        app.select_contact(1);
        assert_eq!(app.selected_key, None);
    }

    #[test]
    fn contacts_reload_keeps_selection_on_the_same_contact() {
        let mut app = app_with_contacts(2);
        app.select_contact(1);
        let reordered = vec![app.contacts[1].clone(), app.contacts[0].clone()];
        app.set_contacts(reordered);
        assert_eq!(app.selected_contact().unwrap().adv_name, "c1");
        assert_eq!(app.selected_index(), Some(0));
    }

    #[test]
    fn contacts_reload_without_the_selected_contact_deselects() {
        let mut app = app_with_contacts(2);
        app.select_contact(1);
        let remaining = vec![app.contacts[0].clone()];
        app.set_contacts(remaining);
        assert_eq!(app.selected_key, None);
        assert_eq!(app.messages.last().unwrap().msg_type, MsgType::Error);
        assert_eq!(last_message(&app), "Contact c1 is no longer in the list; deselected");
    }

    #[test]
    fn device_name_defaults_to_unknown() {
        assert_eq!(App::new(0).device_name(), "Unknown");
    }

    #[test]
    fn device_name_uses_sanitized_self_info_name() {
        let mut app = App::new(0);
        app.self_info = Some(SelfInfo {
            name: " Base\u{0} ".to_string(),
            ..Default::default()
        });
        assert_eq!(app.device_name(), "Base");
    }

    #[test]
    fn sys_and_err_append_tagged_messages() {
        let mut app = App::new(0);
        app.sys("hello");
        app.err("oops");
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[0].sender, "SYS");
        assert_eq!(app.messages[0].msg_type, MsgType::System);
        assert_eq!(app.messages[1].sender, "ERR");
        assert_eq!(app.messages[1].msg_type, MsgType::Error);
    }
}
