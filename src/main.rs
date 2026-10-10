use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CEvent, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use meshcore_rs::events::{Contact, EventPayload, SelfInfo};
use meshcore_rs::{EventType, MeshCore};
use unicode_width::UnicodeWidthStr;

/// Strip Unicode replacement chars, control chars, and emoji from mesh strings.
/// Emoji have inconsistent terminal widths that break TUI border alignment.
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| {
            if *c == '\u{FFFD}' || (*c != ' ' && c.is_control()) {
                return false;
            }
            // Strip emoji and other wide/variable-width Unicode blocks
            let cp = *c as u32;
            // Keep ASCII and basic Latin/extended chars
            if cp < 0x2600 {
                return true;
            }
            // Block various emoji and symbol ranges
            matches!(cp,
                // Allow box drawing, block elements, etc. used by TUI
                0x2500..=0x257F | // Box Drawing
                0x2580..=0x259F   // Block Elements
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Truncate a string to fit within `max_width` display columns
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut w = 0usize;
    let mut result = String::new();
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if w + cw > max_width {
            break;
        }
        w += cw;
        result.push(c);
    }
    result
}

/// Single-line text input with a cursor measured in chars
#[derive(Debug, Default)]
struct InputLine {
    text: String,
    cursor: usize,
}

impl InputLine {
    fn as_str(&self) -> &str {
        &self.text
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    /// Byte offset of the char at `char_idx` (or the end of the text)
    fn byte_offset(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_offset(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_offset(self.cursor);
            self.text.remove(at);
        }
    }

    fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.char_count());
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Return the text and clear the line
    fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Display width of the text before the cursor
    fn cursor_column(&self) -> u16 {
        let before = &self.text[..self.byte_offset(self.cursor)];
        UnicodeWidthStr::width(before) as u16
    }
}

/// Stored channel info
#[derive(Clone, Debug)]
struct ChannelEntry {
    name: String,
    secret: [u8; 16],
}
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use tokio::sync::Mutex;

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "meshcore-chat", about = "TUI chat for MeshCore mesh networking")]
struct Args {
    /// Serial port (e.g. /dev/ttyACM0)
    #[arg(short, long)]
    port: Option<String>,

    /// TCP host:port
    #[arg(short, long)]
    tcp: Option<String>,

    /// BLE device name (scans for MeshCore devices if omitted)
    #[arg(long)]
    ble: Option<Option<String>>,

    /// Baud rate
    #[arg(short, long, default_value_t = 115200)]
    baud: u32,

    /// Default channel (0 = public)
    #[arg(short, long, default_value_t = 0)]
    channel: u8,
}

// ── Data ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct ChatMessage {
    timestamp: String,
    sender: String,
    text: String,
    msg_type: MsgType,
}

#[derive(Clone, Debug, PartialEq)]
enum MsgType {
    Incoming,
    Outgoing,
    Channel(u8),
    System,
    Error,
}

#[derive(Clone, Debug)]
struct PacketLogEntry {
    timestamp: String,
    ptype: String,
    hops: u8,
    rssi: i16,
    snr: f32,
    info: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Panel {
    Messages,
    Contacts,
    Packets,
}

struct App {
    input: InputLine,
    panel: Panel,
    messages: Vec<ChatMessage>,
    contacts: Vec<Contact>,
    channels: HashMap<u8, ChannelEntry>,
    /// Maps firmware channel_idx → local channel index for received message lookup
    fw_channel_map: HashMap<u8, u8>,
    packets: Vec<PacketLogEntry>,
    selected_contact: Option<usize>,
    self_info: Option<SelfInfo>,
    battery_level: Option<u16>,
    active_channel: u8,
    port_info: String,
    rx_count: u32,
    tx_count: u32,
    connected: bool,
    should_quit: bool,
}

impl App {
    fn new(channel: u8) -> Self {
        Self {
            input: InputLine::default(),
            panel: Panel::Messages,
            messages: Vec::new(),
            contacts: Vec::new(),
            channels: HashMap::new(),
            fw_channel_map: HashMap::new(),
            packets: Vec::new(),
            selected_contact: None,
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

    fn sys(&mut self, text: &str) {
        self.messages.push(ChatMessage {
            timestamp: now(),
            sender: "SYS".into(),
            text: text.into(),
            msg_type: MsgType::System,
        });
    }

    fn err(&mut self, text: &str) {
        self.messages.push(ChatMessage {
            timestamp: now(),
            sender: "ERR".into(),
            text: text.into(),
            msg_type: MsgType::Error,
        });
    }

    fn device_name(&self) -> String {
        self.self_info
            .as_ref()
            .map(|i| sanitize(&i.name))
            .unwrap_or_else(|| "Unknown".into())
    }

    fn target_label(&self) -> String {
        self.selected_contact
            .and_then(|i| self.contacts.get(i))
            .map(|c| sanitize(&c.adv_name))
            .unwrap_or_else(|| self.channel_label(self.active_channel))
    }

    fn channel_label(&self, ch: u8) -> String {
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

    /// Apply a key press; returns the input line when Enter submits it
    fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Enter if !self.input.is_empty() => return Some(self.input.take()),
            KeyCode::Char(c) => self.input.insert(c),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Tab => {
                self.panel = match self.panel {
                    Panel::Messages => Panel::Contacts,
                    Panel::Contacts => Panel::Packets,
                    Panel::Packets => Panel::Messages,
                };
            }
            KeyCode::Up if self.panel == Panel::Contacts => {
                if let Some(s) = self.selected_contact {
                    if s > 0 {
                        self.selected_contact = Some(s - 1);
                    }
                } else if !self.contacts.is_empty() {
                    self.selected_contact = Some(0);
                }
            }
            KeyCode::Down if self.panel == Panel::Contacts => {
                if let Some(s) = self.selected_contact {
                    if s + 1 < self.contacts.len() {
                        self.selected_contact = Some(s + 1);
                    }
                } else if !self.contacts.is_empty() {
                    self.selected_contact = Some(0);
                }
            }
            KeyCode::F(1) => self.selected_contact = None,
            _ => {}
        }
        None
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

fn now() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

/// Simple pseudo-random byte (no external crate needed)
fn rand_byte() -> u8 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    (t ^ (t >> 8) ^ (t >> 16)) as u8
}

// ── MeshCore Integration ─────────────────────────────────────────────────────

async fn connect(app: &Arc<Mutex<App>>, args: &Args) -> Option<MeshCore> {
    let mc = if let Some(ref tcp_addr) = args.tcp {
        let parts: Vec<&str> = tcp_addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts.get(1).unwrap_or(&"4403").parse().unwrap_or(4403);
        app.lock().await.sys(&format!("Connecting TCP {}:{}...", host, port));
        match MeshCore::tcp(host, port).await {
            Ok(mc) => mc,
            Err(e) => {
                app.lock().await.err(&format!("TCP connect failed: {}", e));
                return None;
            }
        }
    } else if args.ble.is_some() {
        let device_name = args.ble.as_ref().unwrap().as_deref();
        match device_name {
            Some(name) => app.lock().await.sys(&format!("Scanning BLE for '{}'...", name)),
            None => app.lock().await.sys("Scanning for any MeshCore BLE device..."),
        }
        let name = match device_name {
            Some(name) => Ok(name.to_string()),
            None => MeshCore::ble_discover(Duration::from_secs(5))
                .await
                .and_then(|names| {
                    names
                        .into_iter()
                        .next()
                        .ok_or_else(|| meshcore_rs::Error::connection("no MeshCore BLE device found"))
                }),
        };
        let connected = match name {
            Ok(name) => MeshCore::ble_connect(&name).await,
            Err(e) => Err(e),
        };
        match connected {
            Ok(mc) => mc,
            Err(e) => {
                app.lock().await.err(&format!("BLE connect failed: {}", e));
                return None;
            }
        }
    } else {
        let port = args.port.as_deref().unwrap_or("/dev/ttyACM0");
        app.lock().await.port_info = port.to_string();
        app.lock()
            .await
            .sys(&format!("Connecting serial {} @ {}...", port, args.baud));
        match MeshCore::serial(port, args.baud).await {
            Ok(mc) => mc,
            Err(e) => {
                app.lock()
                    .await
                    .err(&format!("Serial connect failed: {}", e));
                return None;
            }
        }
    };

    // AppStart → get SelfInfo
    match mc.commands().lock().await.send_appstart().await {
        Ok(info) => {
            let mut a = app.lock().await;
            a.sys(&format!("Connected: {} (freq: {} MHz, SF{}, CR{}, BW {} kHz)",
                sanitize(&info.name),
                info.radio_freq as f64 / 1_000.0,
                info.sf,
                info.cr,
                info.radio_bw as f64 / 1_000.0,
            ));
            a.connected = true;
            if a.port_info.is_empty() {
                a.port_info = if args.ble.is_some() {
                    format!("BLE: {}", sanitize(&info.name))
                } else {
                    "TCP".into()
                };
            }
            a.self_info = Some(info);
        }
        Err(e) => {
            app.lock().await.err(&format!("AppStart failed: {}", e));
            return None;
        }
    }

    // Sync time
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;
        if let Err(e) = mc.commands().lock().await.set_time(ts).await {
            app.lock()
                .await
                .sys(&format!("Time sync warning: {}", e));
        }
    }

    // Battery
    if let Ok(bat) = mc.commands().lock().await.get_bat().await {
        let mut a = app.lock().await;
        a.battery_level = Some(bat.battery_mv);
        a.sys(&format!("Battery: {:.3}V", bat.battery_mv as f64 / 1000.0));
    }

    // Contacts (BLE needs longer timeout)
    let contacts_timeout = if args.ble.is_some() {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(5)
    };
    match mc.commands().lock().await.get_contacts_with_timeout(0, contacts_timeout).await {
        Ok(contacts) => {
            let mut a = app.lock().await;
            a.sys(&format!("Loaded {} contacts", contacts.len()));
            a.contacts = contacts;
        }
        Err(e) => {
            app.lock()
                .await
                .err(&format!("Failed to load contacts: {}", e));
        }
    }

    // Load channels sequentially
    {
        let mut loaded = 0u8;
        let mut names = Vec::new();
        for idx in 0..16u8 {
            match mc.commands().lock().await.get_channel(idx).await {
                Ok(info) => {
                    let name = sanitize(&info.name);
                    if !name.is_empty() {
                        let mut a = app.lock().await;
                        a.channels.insert(idx, ChannelEntry {
                            name: name.clone(),
                            secret: info.secret,
                        });
                        a.fw_channel_map.insert(info.channel_idx, idx);
                        names.push(name);
                        drop(a);
                        loaded += 1;
                    }
                }
                Err(_) => continue,
            }
        }
        if loaded > 0 {
            app.lock().await.sys(&format!("Loaded {} channels: {}", loaded, names.join(", ")));
        }
    }

    Some(mc)
}

async fn setup_subscriptions(mc: &MeshCore, app: Arc<Mutex<App>>) {
    // Direct messages
    {
        let app = app.clone();
        mc.subscribe(EventType::ContactMsgRecv, HashMap::new(), move |event| {
            if let EventPayload::ContactMessage(msg) = event.payload {
                let app = app.clone();
                tokio::spawn(async move {
                    let mut a = app.lock().await;
                    let sender = a.resolve_sender(&msg.sender_prefix);
                    a.rx_count += 1;
                    a.messages.push(ChatMessage {
                        timestamp: now(),
                        sender,
                        text: sanitize(&msg.text),
                        msg_type: MsgType::Incoming,
                    });
                    // Bell on new message
                    print!("\x07");
                });
            }
        })
        .await;
    }

    // Channel messages
    {
        let app = app.clone();
        mc.subscribe(
            EventType::ChannelMsgRecv,
            HashMap::new(),
            move |event| {
                if let EventPayload::ChannelMessage(msg) = event.payload {
                    let app = app.clone();
                    tokio::spawn(async move {
                        let mut a = app.lock().await;
                        let (sender, text) = match msg.text.split_once(": ") {
                            Some((name, text)) => (sanitize(name), text),
                            None => ("?".to_string(), msg.text.as_str()),
                        };
                        let ch = msg.channel_idx;
                        a.rx_count += 1;
                        a.messages.push(ChatMessage {
                            timestamp: now(),
                            sender,
                            text: sanitize(text),
                            msg_type: MsgType::Channel(ch),
                        });
                        // Bell on new message
                        print!("\x07");
                    });
                }
            },
        )
        .await;
    }

    // Advertisements (0x80)
    {
        let app = app.clone();
        mc.subscribe(
            EventType::Advertisement,
            HashMap::new(),
            move |event| {
                if let EventPayload::Advertisement(adv) = event.payload {
                    let app = app.clone();
                    tokio::spawn(async move {
                        let mut a = app.lock().await;
                        a.packets.push(PacketLogEntry {
                            timestamp: now(),
                            ptype: "ADV".into(),
                            hops: 0,
                            rssi: 0,
                            snr: 0.0,
                            info: format!(
                                "{} @{:.4},{:.4}",
                                sanitize(&adv.name),
                                adv.lat as f64 / 1e6,
                                adv.lon as f64 / 1e6
                            ),
                        });
                        if a.packets.len() > 100 {
                            a.packets.remove(0);
                        }
                    });
                }
            },
        )
        .await;
    }

    // New contacts / adverts (0x8A PushCodeNewAdvert)
    {
        let app = app.clone();
        mc.subscribe(
            EventType::NewContact,
            HashMap::new(),
            move |event| {
                if let EventPayload::Contact(contact) = event.payload {
                    let app = app.clone();
                    tokio::spawn(async move {
                        let mut a = app.lock().await;
                        let name = sanitize(&contact.adv_name);
                        a.packets.push(PacketLogEntry {
                            timestamp: now(),
                            ptype: "ADV".into(),
                            hops: contact.path_len.max(0) as u8,
                            rssi: 0,
                            snr: 0.0,
                            info: format!(
                                "{} [{}] @{:.4},{:.4}",
                                name,
                                contact.prefix_hex(),
                                contact.latitude(),
                                contact.longitude(),
                            ),
                        });
                        if a.packets.len() > 100 {
                            a.packets.remove(0);
                        }
                        // Update contact list
                        a.contacts.retain(|c| c.prefix() != contact.prefix());
                        a.contacts.push(contact);
                    });
                }
            },
        )
        .await;
    }

    // ACKs
    {
        let app = app.clone();
        mc.subscribe(EventType::Ack, HashMap::new(), move |event| {
            if let EventPayload::Ack { tag } = event.payload {
                let app = app.clone();
                tokio::spawn(async move {
                    let mut a = app.lock().await;
                    a.packets.push(PacketLogEntry {
                        timestamp: now(),
                        ptype: "ACK".into(),
                        hops: 0,
                        rssi: 0,
                        snr: 0.0,
                        info: format!("tag:{}", meshcore_rs::parsing::hex_encode(&tag)),
                    });
                    if a.packets.len() > 100 {
                        a.packets.remove(0);
                    }
                });
            }
        })
        .await;
    }

    // Path updates
    {
        let app = app.clone();
        mc.subscribe(EventType::PathUpdate, HashMap::new(), move |event| {
            if let EventPayload::PathUpdate(pu) = event.payload {
                let app = app.clone();
                tokio::spawn(async move {
                    let mut a = app.lock().await;
                    a.packets.push(PacketLogEntry {
                        timestamp: now(),
                        ptype: "PATH".into(),
                        hops: pu.path_len as u8,
                        rssi: 0,
                        snr: 0.0,
                        info: format!(
                            "{} len={}",
                            meshcore_rs::parsing::hex_encode(&pu.prefix),
                            pu.path_len
                        ),
                    });
                    if a.packets.len() > 100 {
                        a.packets.remove(0);
                    }
                });
            }
        })
        .await;
    }

    mc.start_auto_message_fetching().await;
}

async fn send_message(mc: &MeshCore, app: &Arc<Mutex<App>>, text: &str) {
    let (selected, channel) = {
        let a = app.lock().await;
        (a.selected_contact, a.active_channel)
    };

    if let Some(idx) = selected {
        // Direct message to selected contact
        let contact = {
            let a = app.lock().await;
            a.contacts.get(idx).cloned()
        };
        if let Some(contact) = contact {
            let name = sanitize(&contact.adv_name);
            match mc.commands().lock().await.send_msg(&contact, text, None).await {
                Ok(info) => {
                    let mut a = app.lock().await;
                    a.tx_count += 1;
                    a.messages.push(ChatMessage {
                        timestamp: now(),
                        sender: format!("→ {}", name),
                        text: text.into(),
                        msg_type: MsgType::Outgoing,
                    });
                    a.packets.push(PacketLogEntry {
                        timestamp: now(),
                        ptype: "SND".into(),
                        hops: 0,
                        rssi: 0,
                        snr: 0.0,
                        info: format!(
                            "ack:{} timeout:{}ms",
                            meshcore_rs::parsing::hex_encode(&info.expected_ack),
                            info.suggested_timeout
                        ),
                    });
                }
                Err(e) => app.lock().await.err(&format!("Send failed: {}", e)),
            }
        }
    } else {
        // Channel message
        match mc
            .commands()
            .lock()
            .await
            .send_channel_msg(channel, text, None)
            .await
        {
            Ok(_) => {
                let mut a = app.lock().await;
                a.tx_count += 1;
                let label = a.channel_label(channel);
                a.messages.push(ChatMessage {
                    timestamp: now(),
                    sender: format!("\u{2192} {}", label),
                    text: text.into(),
                    msg_type: MsgType::Outgoing,
                });
            }
            Err(e) => app.lock().await.err(&format!("Channel send failed: {}", e)),
        }
    }
}

// ── Slash Commands ───────────────────────────────────────────────────────────

/// Describes a slash command for parsing, usage errors and /help
struct CommandSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    args: &'static str,
    description: &'static str,
}

impl CommandSpec {
    fn usage(&self) -> String {
        format!("Usage: {} {}", self.name, self.args)
    }

    fn help_line(&self) -> String {
        let mut line = self.name.to_string();
        if !self.args.is_empty() {
            line.push(' ');
            line.push_str(self.args);
        }
        line.push_str(" — ");
        line.push_str(self.description);
        if !self.aliases.is_empty() {
            line.push_str(&format!(" ({})", self.aliases.join(", ")));
        }
        line
    }
}

const COMMANDS: &[CommandSpec] = &[
    CommandSpec { name: "/help", aliases: &["/h"], args: "", description: "list commands" },
    CommandSpec { name: "/contacts", aliases: &["/c"], args: "", description: "reload contacts" },
    CommandSpec { name: "/to", aliases: &[], args: "<contact_name>", description: "DM a contact" },
    CommandSpec { name: "/ch", aliases: &["/channel"], args: "<idx>", description: "switch channel" },
    CommandSpec { name: "/public", aliases: &[], args: "", description: "switch to public channel (CH#0)" },
    CommandSpec { name: "/channels", aliases: &["/chs"], args: "", description: "list channels" },
    CommandSpec {
        name: "/setch",
        aliases: &["/setchannel"],
        args: "<idx> <name> [hex_secret_32chars]",
        description: "create channel (random secret if omitted)",
    },
    CommandSpec { name: "/getch", aliases: &["/getchannel"], args: "<idx>", description: "show channel info" },
    CommandSpec { name: "/advert", aliases: &[], args: "", description: "send advert" },
    CommandSpec { name: "/bat", aliases: &[], args: "", description: "battery and storage" },
    CommandSpec { name: "/name", aliases: &[], args: "<new_name>", description: "rename device" },
    CommandSpec { name: "/info", aliases: &["/i"], args: "", description: "device info" },
    CommandSpec { name: "/quit", aliases: &["/q"], args: "", description: "quit" },
];

fn find_command(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Contacts,
    To(String),
    SwitchChannel(u8),
    Public,
    Channels,
    SetChannel {
        idx: u8,
        name: String,
        secret: Option<[u8; 16]>,
    },
    GetChannel(u8),
    Advert,
    Battery,
    Name(String),
    Info,
    Quit,
}

fn parse_channel_index(arg: &str, spec: &CommandSpec) -> Result<u8, String> {
    if arg.is_empty() {
        return Err(spec.usage());
    }
    arg.parse::<u8>()
        .map_err(|_| "Channel index must be a number".to_string())
}

fn parse_secret(hex: &str) -> Result<[u8; 16], String> {
    match meshcore_rs::parsing::hex_decode(hex) {
        Ok(bytes) if bytes.len() >= 16 => {
            let mut secret = [0u8; 16];
            secret.copy_from_slice(&bytes[..16]);
            Ok(secret)
        }
        _ => Err("Secret must be 32 hex chars (16 bytes)".to_string()),
    }
}

/// Parse a slash command line; the error is the text to show the user
fn parse_command(input: &str) -> Result<Command, String> {
    let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
    let name = parts[0].to_lowercase();
    let arg = parts.get(1).copied().unwrap_or("");

    let spec = find_command(&name).ok_or_else(|| format!("Unknown command: {}", name))?;
    match spec.name {
        "/help" => Ok(Command::Help),
        "/contacts" => Ok(Command::Contacts),
        "/to" if arg.is_empty() => Err(spec.usage()),
        "/to" => Ok(Command::To(arg.to_string())),
        "/ch" => parse_channel_index(arg, spec).map(Command::SwitchChannel),
        "/public" => Ok(Command::Public),
        "/channels" => Ok(Command::Channels),
        "/setch" => {
            let parts: Vec<&str> = arg.splitn(3, ' ').collect();
            if parts.len() < 2 {
                return Err(spec.usage());
            }
            let idx = parse_channel_index(parts[0], spec)?;
            let secret = parts.get(2).map(|hex| parse_secret(hex)).transpose()?;
            Ok(Command::SetChannel {
                idx,
                name: parts[1].to_string(),
                secret,
            })
        }
        "/getch" => parse_channel_index(arg, spec).map(Command::GetChannel),
        "/advert" => Ok(Command::Advert),
        "/bat" => Ok(Command::Battery),
        "/name" if arg.is_empty() => Err(spec.usage()),
        "/name" => Ok(Command::Name(arg.to_string())),
        "/info" => Ok(Command::Info),
        "/quit" => Ok(Command::Quit),
        other => Err(format!("Unknown command: {}", other)),
    }
}

impl App {
    /// Run a command that only needs local state; radio commands are returned unhandled
    fn apply_local(&mut self, cmd: Command) -> Option<Command> {
        match cmd {
            Command::Help => {
                for spec in COMMANDS {
                    self.sys(&spec.help_line());
                }
            }
            Command::Channels => {
                if self.channels.is_empty() {
                    self.sys("No channels loaded");
                } else {
                    let mut indices: Vec<u8> = self.channels.keys().copied().collect();
                    indices.sort();
                    let lines: Vec<String> = indices
                        .into_iter()
                        .map(|idx| {
                            let ch = &self.channels[&idx];
                            let secret_hex = meshcore_rs::parsing::hex_encode(&ch.secret);
                            let active = if idx == self.active_channel { " \u{25c0}" } else { "" };
                            format!("  #{}: {} [{}]{}", idx, ch.name, secret_hex, active)
                        })
                        .collect();
                    self.sys("Channels:");
                    for line in lines {
                        self.sys(&line);
                    }
                }
            }
            Command::SwitchChannel(ch) => self.switch_channel(ch),
            Command::Public => self.switch_channel(0),
            Command::To(name) => {
                let needle = name.to_lowercase();
                let found = self
                    .contacts
                    .iter()
                    .position(|c| c.adv_name.to_lowercase().contains(&needle));
                if let Some(idx) = found {
                    let contact_name = sanitize(&self.contacts[idx].adv_name);
                    self.selected_contact = Some(idx);
                    self.sys(&format!("Now chatting with: {}", contact_name));
                } else {
                    self.err(&format!("Contact '{}' not found", name));
                }
            }
            Command::Info => {
                let line = self.self_info.as_ref().map(|info| {
                    format!(
                        "Device: {} | Key: {} | Freq: {:.3} MHz | SF{} CR{} BW {:.1}kHz | TX: {}dBm | Bat: {}",
                        sanitize(&info.name),
                        meshcore_rs::parsing::hex_encode(&info.public_key[..6]),
                        info.radio_freq as f64 / 1e3,
                        info.sf,
                        info.cr,
                        info.radio_bw as f64 / 1e3,
                        info.tx_power,
                        self.battery_level.map(|b| format!("{:.3}V", b as f64 / 1000.0)).unwrap_or("N/A".into()),
                    )
                });
                match line {
                    Some(line) => self.sys(&line),
                    None => self.err("No device info available"),
                }
            }
            Command::Quit => self.should_quit = true,
            radio => return Some(radio),
        }
        None
    }

    fn switch_channel(&mut self, ch: u8) {
        self.active_channel = ch;
        self.selected_contact = None;
        let label = self.channel_label(ch);
        self.sys(&format!("Switched to {}", label));
    }
}

async fn handle_command(input: &str, app: &Arc<Mutex<App>>, mc: Option<&MeshCore>) {
    let cmd = match parse_command(input) {
        Ok(cmd) => cmd,
        Err(e) => {
            app.lock().await.err(&e);
            return;
        }
    };
    let Some(cmd) = app.lock().await.apply_local(cmd) else {
        return;
    };
    let Some(mc) = mc else {
        app.lock().await.err("Not connected");
        return;
    };

    match cmd {
        Command::Contacts => match mc.commands().lock().await.get_contacts(0).await {
            Ok(c) => {
                let mut a = app.lock().await;
                a.sys(&format!("Loaded {} contacts", c.len()));
                a.contacts = c;
            }
            Err(e) => app.lock().await.err(&format!("Contacts: {}", e)),
        },
        Command::Advert => match mc.commands().lock().await.send_advert(true).await {
            Ok(_) => app.lock().await.sys("Advertisement sent"),
            Err(e) => app.lock().await.err(&format!("Advert: {}", e)),
        },
        Command::Battery => match mc.commands().lock().await.get_bat().await {
            Ok(b) => {
                let mut a = app.lock().await;
                a.battery_level = Some(b.battery_mv);
                let storage = match (b.used_kb, b.total_kb) {
                    (Some(used), Some(total)) => format!("{}/{} KB", used, total),
                    _ => "N/A".to_string(),
                };
                a.sys(&format!("Battery: {:.3}V  Storage: {}", b.battery_mv as f64 / 1000.0, storage));
            }
            Err(e) => app.lock().await.err(&format!("Battery: {}", e)),
        },
        Command::Name(name) => match mc.commands().lock().await.set_name(&name).await {
            Ok(_) => app.lock().await.sys(&format!("Name set to: {}", name)),
            Err(e) => app.lock().await.err(&format!("Set name: {}", e)),
        },
        Command::SetChannel { idx, name, secret } => {
            let secret = secret.unwrap_or_else(|| {
                let mut s = [0u8; 16];
                for b in &mut s {
                    *b = rand_byte();
                }
                s
            });
            match mc.commands().lock().await.set_channel(idx, &name, &secret).await {
                Ok(_) => {
                    let secret_hex = meshcore_rs::parsing::hex_encode(&secret);
                    let mut a = app.lock().await;
                    a.sys(&format!(
                        "Channel #{} set: name='{}' secret={}",
                        idx, name, secret_hex
                    ));
                    a.channels.insert(idx, ChannelEntry { name, secret });
                }
                Err(e) => app.lock().await.err(&format!("Set channel: {}", e)),
            }
        }
        Command::GetChannel(idx) => match mc.commands().lock().await.get_channel(idx).await {
            Ok(info) => {
                let name = sanitize(&info.name);
                let secret_hex = meshcore_rs::parsing::hex_encode(&info.secret);
                let mut a = app.lock().await;
                if !name.is_empty() {
                    a.channels.insert(idx, ChannelEntry {
                        name: name.clone(),
                        secret: info.secret,
                    });
                    a.fw_channel_map.insert(info.channel_idx, idx);
                }
                a.sys(&format!(
                    "Channel #{} (fw#{}): name='{}' secret={}",
                    idx, info.channel_idx, name, secret_hex
                ));
            }
            Err(e) => app.lock().await.err(&format!("Get channel: {}", e)),
        },
        // apply_local handles every command that doesn't need the radio
        _ => {}
    }
}

// ── TUI Rendering ────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // status
            Constraint::Min(10),   // main
            Constraint::Length(7), // packets
            Constraint::Length(3), // input
        ])
        .split(f.area());

    draw_status(f, app, chunks[0]);
    draw_main(f, app, chunks[1]);
    draw_packets(f, app, chunks[2]);
    draw_input(f, app, chunks[3]);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let freq_info = app
        .self_info
        .as_ref()
        .map(|i| {
            format!(
                "{:.3} MHz | BW: {:.1} kHz | SF: {} | CR: {}",
                i.radio_freq as f64 / 1e3,
                i.radio_bw as f64 / 1e3,
                i.sf,
                i.cr
            )
        })
        .unwrap_or_default();

    let spans = vec![
        Span::styled("Port: ", Style::default().fg(Color::White)),
        Span::styled(&app.port_info, Style::default().fg(Color::Cyan)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &freq_info,
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("RX: {} | TX: {}", app.rx_count, app.tx_count),
            Style::default().fg(Color::Green),
        ),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if app.connected { "● Connected" } else { "○ Disconnected" },
            Style::default().fg(if app.connected {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ];

    let w = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Status ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(w, area);
}

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(30)])
        .split(area);

    // Messages
    let panel_width = (chunks[0].width as usize).saturating_sub(2); // border
    let items: Vec<ListItem> = app
        .messages
        .iter()
        .map(|m| {
            let (tag, tag_c, name_c) = match &m.msg_type {
                MsgType::Incoming => ("[MSG]".to_string(), Color::Cyan, Color::Cyan),
                MsgType::Outgoing => ("[SND]".to_string(), Color::Green, Color::Green),
                MsgType::Channel(ch) => {
                    let label = app.channel_label(*ch);
                    (format!("[{}]", label), Color::Yellow, Color::Yellow)
                }
                MsgType::System => ("[SYS]".to_string(), Color::DarkGray, Color::DarkGray),
                MsgType::Error => ("[ERR]".to_string(), Color::Red, Color::Red),
            };

            // Build prefix: "HH:MM:SS [TAG] sender: "
            let prefix = format!("{} {} {}: ", m.timestamp, tag, m.sender);
            let prefix_len = prefix.width();
            let text_display_width = m.text.width();

            if prefix_len + text_display_width <= panel_width || panel_width <= prefix_len {
                // Fits on one line or panel too narrow to wrap
                ListItem::new(Line::from(vec![
                    Span::styled(&m.timestamp, Style::default().fg(Color::DarkGray)),
                    Span::raw(" "),
                    Span::styled(tag, Style::default().fg(tag_c).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled(
                        &m.sender,
                        Style::default().fg(name_c).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(": ", Style::default().fg(name_c)),
                    Span::styled(&m.text, Style::default().fg(Color::White)),
                ]))
            } else {
                // Wrap text across multiple lines
                let text_width = panel_width.saturating_sub(prefix_len);
                let continuation_indent = " ".repeat(prefix_len);
                let mut lines: Vec<Line> = Vec::new();
                let chars: Vec<char> = m.text.chars().collect();

                // Split chars into chunks that fit in text_width display columns
                let mut chunk_start = 0;
                let mut first = true;
                while chunk_start < chars.len() {
                    let mut w = 0usize;
                    let mut chunk_end = chunk_start;
                    while chunk_end < chars.len() {
                        let cw = unicode_width::UnicodeWidthChar::width(chars[chunk_end]).unwrap_or(1);
                        if w + cw > text_width {
                            break;
                        }
                        w += cw;
                        chunk_end += 1;
                    }
                    if chunk_end == chunk_start {
                        chunk_end += 1; // always advance at least one char
                    }
                    let chunk_text: String = chars[chunk_start..chunk_end].iter().collect();

                    if first {
                        lines.push(Line::from(vec![
                            Span::styled(&m.timestamp, Style::default().fg(Color::DarkGray)),
                            Span::raw(" "),
                            Span::styled(tag.clone(), Style::default().fg(tag_c).add_modifier(Modifier::BOLD)),
                            Span::raw(" "),
                            Span::styled(
                                &m.sender,
                                Style::default().fg(name_c).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(": ", Style::default().fg(name_c)),
                            Span::styled(chunk_text, Style::default().fg(Color::White)),
                        ]));
                        first = false;
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled(
                                continuation_indent.clone(),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(chunk_text, Style::default().fg(Color::White)),
                        ]));
                    }
                    chunk_start = chunk_end;
                }

                ListItem::new(lines)
            }
        })
        .collect::<Vec<_>>();

    // Take items from the end that fit in visible height
    let visible_height = (chunks[0].height as usize).saturating_sub(2);
    let mut total_lines = 0usize;
    let mut start_idx = items.len();
    for (i, item) in items.iter().enumerate().rev() {
        let h = item.height();
        if total_lines + h > visible_height {
            break;
        }
        total_lines += h;
        start_idx = i;
    }
    let visible_items: Vec<ListItem> = items.into_iter().skip(start_idx).collect();

    let bc = if app.panel == Panel::Messages {
        Color::Yellow
    } else {
        Color::Cyan
    };
    let msg_w = List::new(visible_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} — {} ", app.device_name(), app.target_label()))
            .border_style(Style::default().fg(bc)),
    );
    f.render_widget(msg_w, chunks[0]);

    // Contacts
    let cpanel_width = (chunks[1].width as usize).saturating_sub(2); // minus borders
    let citems: Vec<ListItem> = app
        .contacts
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let sel = app.selected_contact == Some(i);
            let sty = if sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let name = sanitize(&c.adv_name);
            let hex = c.prefix_hex();
            let hex_short = &hex[..8.min(hex.len())];

            // Build the full line and truncate to exactly cpanel_width
            let marker = if sel { "\u{25b6} " } else { "  " };
            let full = format!("{}{} ({})", marker, name, hex_short);
            let line = truncate_to_width(&full, cpanel_width);

            // Pad with spaces to fill the panel (needed for selection highlight)
            let line_w = UnicodeWidthStr::width(line.as_str());
            let padded = if line_w < cpanel_width {
                format!("{}{}", line, " ".repeat(cpanel_width - line_w))
            } else {
                line
            };

            ListItem::new(Span::styled(padded, sty))
        })
        .collect();

    let cc = if app.panel == Panel::Contacts {
        Color::Yellow
    } else {
        Color::Green
    };
    let c_w = List::new(citems).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Contacts ({}) ", app.contacts.len()))
            .border_style(Style::default().fg(cc)),
    );
    f.render_widget(c_w, chunks[1]);
}

fn draw_packets(f: &mut Frame, app: &App, area: Rect) {
    let visible = (area.height as usize).saturating_sub(2);
    let items: Vec<ListItem> = app
        .packets
        .iter()
        .rev()
        .take(visible)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|p| {
            let tc = match p.ptype.as_str() {
                "ADV" => Color::Magenta,
                "ACK" => Color::Cyan,
                "SND" => Color::Green,
                "PATH" => Color::Blue,
                _ => Color::White,
            };
            ListItem::new(Line::from(vec![
                Span::styled(&p.timestamp, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    format!("[{}]", p.ptype),
                    Style::default().fg(tc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" H:{}", p.hops),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    if p.rssi != 0 || p.snr != 0.0 {
                        format!(" R:{}dBm S:{:.1}dB", p.rssi, p.snr)
                    } else {
                        String::new()
                    },
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" | {}", p.info),
                    Style::default().fg(Color::White),
                ),
            ]))
        })
        .collect();

    let pc = if app.panel == Panel::Packets {
        Color::Yellow
    } else {
        Color::Magenta
    };
    let w = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Received Packets ")
            .border_style(Style::default().fg(pc)),
    );
    f.render_widget(w, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let w = Paragraph::new(app.input.as_str())
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " Send to: {} (Enter=send, Tab=panel, ↑↓=contact, F1=deselect, Esc=quit) ",
                    app.target_label()
                ))
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(w, area);
    f.set_cursor_position((area.x + app.input.cursor_column() + 1, area.y + 1));
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Tracing: suppress all output by default (TUI uses the terminal).
    // Set MESHCORE_LOG=1 to write debug logs to meshcore-chat.log
    if std::env::var("MESHCORE_LOG").is_ok() {
        let log_file = std::fs::File::create("meshcore-chat.log").ok();
        if let Some(file) = log_file {
            tracing_subscriber::fmt()
                .with_env_filter("meshcore_rs=debug,btleplug=warn")
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = Arc::new(Mutex::new(App::new(args.channel)));

    {
        let mut a = app.lock().await;
        a.sys("MeshCore Chat — Rust TUI Client v0.1");
        a.sys("Tab=panel  ↑↓=contact  F1=deselect  Enter=send  /help=commands  Esc=quit");
    }

    let mc = connect(&app, &args).await;

    if let Some(ref mc) = mc {
        app.lock().await.sys("Ready! Type a message and press Enter.");
        setup_subscriptions(mc, app.clone()).await;
    } else {
        app.lock()
            .await
            .sys("Offline mode — fix connection and restart.");
    }

    // Main loop
    loop {
        {
            let a = app.lock().await;
            terminal.draw(|f| draw(f, &*a))?;
        }

        if event::poll(Duration::from_millis(50))? {
            if let CEvent::Key(key) = event::read()? {
                let submitted = app.lock().await.handle_key(key);
                if let Some(text) = submitted {
                    if text.starts_with('/') {
                        handle_command(&text, &app, mc.as_ref()).await;
                    } else if let Some(ref mc) = mc {
                        send_message(mc, &app, &text).await;
                    } else {
                        app.lock().await.err("Not connected");
                    }
                }
            }
        }

        if app.lock().await.should_quit {
            break;
        }
    }

    // Cleanup
    if let Some(mc) = mc {
        let _ = mc.disconnect().await;
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    println!("MeshCore Chat terminated.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(name: &str, key_prefix: [u8; 6]) -> Contact {
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

    fn channel(name: &str) -> ChannelEntry {
        ChannelEntry {
            name: name.to_string(),
            secret: [0u8; 16],
        }
    }

    // ── sanitize ─────────────────────────────────────────────────────────────

    #[test]
    fn sanitize_keeps_plain_and_latin_text() {
        assert_eq!(sanitize("hello world"), "hello world");
        assert_eq!(sanitize("Zoë Ångström"), "Zoë Ångström");
    }

    #[test]
    fn sanitize_strips_control_and_replacement_chars() {
        assert_eq!(sanitize("a\u{0}b\tc\u{FFFD}d"), "abcd");
    }

    #[test]
    fn sanitize_strips_emoji_and_symbols_at_or_above_u2600() {
        assert_eq!(sanitize("hi 😀 ☀ there"), "hi   there");
    }

    #[test]
    fn sanitize_strips_cjk() {
        assert_eq!(sanitize("node 日本"), "node");
    }

    #[test]
    fn sanitize_keeps_box_drawing() {
        assert_eq!(sanitize("─│▀"), "─│▀");
    }

    #[test]
    fn sanitize_trims_surrounding_whitespace() {
        assert_eq!(sanitize("  name \n"), "name");
    }

    // ── truncate_to_width ────────────────────────────────────────────────────

    #[test]
    fn truncate_shorter_than_width_is_unchanged() {
        assert_eq!(truncate_to_width("abc", 10), "abc");
    }

    #[test]
    fn truncate_cuts_at_width() {
        assert_eq!(truncate_to_width("abcdef", 4), "abcd");
        assert_eq!(truncate_to_width("abc", 0), "");
    }

    #[test]
    fn truncate_counts_display_columns_not_chars() {
        // Each CJK char is two columns wide; a third would exceed 5 columns.
        assert_eq!(truncate_to_width("日本語", 5), "日本");
    }

    // ── InputLine ────────────────────────────────────────────────────────────

    fn typed(s: &str) -> InputLine {
        let mut line = InputLine::default();
        for c in s.chars() {
            line.insert(c);
        }
        line
    }

    #[test]
    fn input_insert_appends_at_cursor() {
        let mut line = typed("ac");
        line.left();
        line.insert('b');
        assert_eq!(line.as_str(), "abc");
        assert_eq!(line.cursor, 2);
    }

    #[test]
    fn input_backspace_removes_char_before_cursor() {
        let mut line = typed("abc");
        line.left();
        line.backspace();
        assert_eq!(line.as_str(), "ac");
        assert_eq!(line.cursor, 1);
    }

    #[test]
    fn input_backspace_at_start_does_nothing() {
        let mut line = typed("ab");
        line.home();
        line.backspace();
        assert_eq!(line.as_str(), "ab");
        assert_eq!(line.cursor, 0);
    }

    #[test]
    fn input_cursor_movement_is_clamped() {
        let mut line = typed("ab");
        line.right();
        assert_eq!(line.cursor, 2);
        line.home();
        line.left();
        assert_eq!(line.cursor, 0);
        line.end();
        assert_eq!(line.cursor, 2);
    }

    #[test]
    fn input_edits_multibyte_text_without_panicking() {
        let mut line = typed("héllo");
        line.home();
        line.right();
        line.right();
        line.backspace();
        assert_eq!(line.as_str(), "hllo");
        line.insert('ë');
        assert_eq!(line.as_str(), "hëllo");
        line.end();
        line.insert('ü');
        assert_eq!(line.as_str(), "hëlloü");
    }

    #[test]
    fn input_cursor_column_counts_display_width() {
        let mut line = typed("日本x");
        assert_eq!(line.cursor_column(), 5);
        line.left();
        assert_eq!(line.cursor_column(), 4);
        line.home();
        assert_eq!(line.cursor_column(), 0);
    }

    #[test]
    fn input_take_returns_text_and_clears() {
        let mut line = typed("héllo");
        assert_eq!(line.take(), "héllo");
        assert!(line.is_empty());
        assert_eq!(line.cursor, 0);
    }

    // ── Key handling ─────────────────────────────────────────────────────────

    fn press(app: &mut App, code: KeyCode) -> Option<String> {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn app_with_contacts(n: u8) -> App {
        let mut app = App::new(0);
        for i in 0..n {
            app.contacts.push(contact(&format!("c{}", i), [i + 1; 6]));
        }
        app
    }

    #[test]
    fn esc_quits() {
        let mut app = App::new(0);
        press(&mut app, KeyCode::Esc);
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_c_quits_without_typing() {
        let mut app = App::new(0);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
        assert!(app.input.is_empty());
    }

    #[test]
    fn typing_edits_the_input_line() {
        let mut app = App::new(0);
        for c in "hi!".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.input.as_str(), ">hi");
    }

    #[test]
    fn enter_submits_and_clears_input() {
        let mut app = App::new(0);
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(press(&mut app, KeyCode::Enter), Some("x".to_string()));
        assert!(app.input.is_empty());
    }

    #[test]
    fn enter_on_empty_input_submits_nothing() {
        assert_eq!(press(&mut App::new(0), KeyCode::Enter), None);
    }

    #[test]
    fn tab_cycles_panels() {
        let mut app = App::new(0);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.panel, Panel::Contacts);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.panel, Panel::Packets);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.panel, Panel::Messages);
    }

    #[test]
    fn arrows_select_contacts_only_in_contacts_panel() {
        let mut app = app_with_contacts(3);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_contact, None);
        app.panel = Panel::Contacts;
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_contact, Some(0));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_contact, Some(2));
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected_contact, Some(0));
    }

    #[test]
    fn up_with_no_selection_selects_first_contact() {
        let mut app = app_with_contacts(2);
        app.panel = Panel::Contacts;
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected_contact, Some(0));
    }

    #[test]
    fn arrows_with_no_contacts_select_nothing() {
        let mut app = App::new(0);
        app.panel = Panel::Contacts;
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected_contact, None);
    }

    #[test]
    fn f1_clears_contact_selection() {
        let mut app = app_with_contacts(1);
        app.selected_contact = Some(0);
        press(&mut app, KeyCode::F(1));
        assert_eq!(app.selected_contact, None);
    }

    // ── Slash commands ───────────────────────────────────────────────────────

    #[test]
    fn every_command_name_and_alias_is_recognized() {
        for spec in COMMANDS {
            for name in std::iter::once(&spec.name).chain(spec.aliases) {
                if let Err(e) = parse_command(name) {
                    assert!(!e.starts_with("Unknown command"), "{} not recognized", name);
                }
            }
        }
    }

    #[test]
    fn command_names_and_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for spec in COMMANDS {
            for name in std::iter::once(&spec.name).chain(spec.aliases) {
                assert!(seen.insert(*name), "{} listed twice", name);
            }
        }
    }

    #[test]
    fn parse_simple_commands_and_aliases() {
        assert_eq!(parse_command("/help"), Ok(Command::Help));
        assert_eq!(parse_command("/h"), Ok(Command::Help));
        assert_eq!(parse_command("/c"), Ok(Command::Contacts));
        assert_eq!(parse_command("/public"), Ok(Command::Public));
        assert_eq!(parse_command("/chs"), Ok(Command::Channels));
        assert_eq!(parse_command("/advert"), Ok(Command::Advert));
        assert_eq!(parse_command("/bat"), Ok(Command::Battery));
        assert_eq!(parse_command("/i"), Ok(Command::Info));
        assert_eq!(parse_command("/q"), Ok(Command::Quit));
    }

    #[test]
    fn parse_command_name_is_case_insensitive_and_trimmed() {
        assert_eq!(parse_command("  /HELP  "), Ok(Command::Help));
    }

    #[test]
    fn parse_unknown_command() {
        assert_eq!(parse_command("/Nope x"), Err("Unknown command: /nope".to_string()));
    }

    #[test]
    fn parse_to_keeps_the_rest_of_the_line() {
        assert_eq!(parse_command("/to Big Bob"), Ok(Command::To("Big Bob".to_string())));
        assert_eq!(parse_command("/to"), Err("Usage: /to <contact_name>".to_string()));
    }

    #[test]
    fn parse_name_requires_an_argument() {
        assert_eq!(parse_command("/name Base"), Ok(Command::Name("Base".to_string())));
        assert_eq!(parse_command("/name"), Err("Usage: /name <new_name>".to_string()));
    }

    #[test]
    fn parse_channel_index_commands() {
        assert_eq!(parse_command("/ch 3"), Ok(Command::SwitchChannel(3)));
        assert_eq!(parse_command("/channel 3"), Ok(Command::SwitchChannel(3)));
        assert_eq!(parse_command("/getch 2"), Ok(Command::GetChannel(2)));
        assert_eq!(parse_command("/ch"), Err("Usage: /ch <idx>".to_string()));
        assert_eq!(parse_command("/getch x"), Err("Channel index must be a number".to_string()));
        assert_eq!(parse_command("/ch 256"), Err("Channel index must be a number".to_string()));
    }

    #[test]
    fn parse_setch_without_secret() {
        assert_eq!(
            parse_command("/setch 1 Ops"),
            Ok(Command::SetChannel { idx: 1, name: "Ops".to_string(), secret: None })
        );
    }

    #[test]
    fn parse_setch_with_secret() {
        let expected: [u8; 16] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
                                  0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        assert_eq!(
            parse_command("/setchannel 1 Ops 00112233445566778899aabbccddeeff"),
            Ok(Command::SetChannel { idx: 1, name: "Ops".to_string(), secret: Some(expected) })
        );
    }

    #[test]
    fn parse_setch_errors() {
        let usage = Err("Usage: /setch <idx> <name> [hex_secret_32chars]".to_string());
        assert_eq!(parse_command("/setch"), usage);
        assert_eq!(parse_command("/setch 1"), usage);
        assert_eq!(parse_command("/setch x Ops"), Err("Channel index must be a number".to_string()));
        let bad_secret = Err("Secret must be 32 hex chars (16 bytes)".to_string());
        assert_eq!(parse_command("/setch 1 Ops abcd"), bad_secret);
        assert_eq!(parse_command("/setch 1 Ops zz112233445566778899aabbccddeeff"), bad_secret);
    }

    fn last_message(app: &App) -> &str {
        &app.messages.last().expect("no messages").text
    }

    #[test]
    fn help_lists_every_command() {
        let mut app = App::new(0);
        assert_eq!(app.apply_local(Command::Help), None);
        assert_eq!(app.messages.len(), COMMANDS.len());
        for (msg, spec) in app.messages.iter().zip(COMMANDS) {
            assert!(msg.text.starts_with(spec.name), "{} missing from help", spec.name);
        }
    }

    #[test]
    fn help_line_shows_args_and_aliases() {
        let spec = find_command("/getch").unwrap();
        assert_eq!(spec.help_line(), "/getch <idx> — show channel info (/getchannel)");
    }

    #[test]
    fn switch_channel_clears_contact_selection() {
        let mut app = app_with_contacts(1);
        app.selected_contact = Some(0);
        app.channels.insert(2, channel("Ops"));
        app.apply_local(Command::SwitchChannel(2));
        assert_eq!(app.active_channel, 2);
        assert_eq!(app.selected_contact, None);
        assert_eq!(last_message(&app), "Switched to Ops");
    }

    #[test]
    fn public_switches_to_channel_zero() {
        let mut app = App::new(4);
        app.apply_local(Command::Public);
        assert_eq!(app.active_channel, 0);
        assert_eq!(last_message(&app), "Switched to CH#0");
    }

    #[test]
    fn to_selects_first_case_insensitive_partial_match() {
        let mut app = App::new(0);
        app.contacts.push(contact("Alice", [1; 6]));
        app.contacts.push(contact("Bobby", [2; 6]));
        app.apply_local(Command::To("BOB".to_string()));
        assert_eq!(app.selected_contact, Some(1));
        assert_eq!(last_message(&app), "Now chatting with: Bobby");
    }

    #[test]
    fn to_unknown_contact_is_an_error() {
        let mut app = App::new(0);
        app.apply_local(Command::To("Zed".to_string()));
        assert_eq!(app.selected_contact, None);
        assert_eq!(app.messages.last().unwrap().msg_type, MsgType::Error);
        assert_eq!(last_message(&app), "Contact 'Zed' not found");
    }

    #[test]
    fn channels_lists_sorted_with_active_marker() {
        let mut app = App::new(1);
        app.channels.insert(1, channel("B"));
        app.channels.insert(0, channel("A"));
        app.apply_local(Command::Channels);
        let texts: Vec<&str> = app.messages.iter().map(|m| m.text.as_str()).collect();
        let zeros = "00".repeat(16);
        assert_eq!(texts, vec![
            "Channels:".to_string(),
            format!("  #0: A [{}]", zeros),
            format!("  #1: B [{}] \u{25c0}", zeros),
        ]);
    }

    #[test]
    fn channels_when_none_loaded() {
        let mut app = App::new(0);
        app.apply_local(Command::Channels);
        assert_eq!(last_message(&app), "No channels loaded");
    }

    #[test]
    fn info_without_device_is_an_error() {
        let mut app = App::new(0);
        app.apply_local(Command::Info);
        assert_eq!(last_message(&app), "No device info available");
    }

    #[test]
    fn info_summarizes_device() {
        let mut app = App::new(0);
        let mut public_key = [0u8; 32];
        public_key[..6].copy_from_slice(&[0xab, 0xcd, 0xef, 0x01, 0x23, 0x45]);
        app.self_info = Some(SelfInfo {
            name: "Base".to_string(),
            public_key,
            radio_freq: 869_618,
            radio_bw: 62_500,
            sf: 8,
            cr: 5,
            tx_power: 22,
            ..Default::default()
        });
        app.battery_level = Some(4012);
        app.apply_local(Command::Info);
        assert_eq!(
            last_message(&app),
            "Device: Base | Key: abcdef012345 | Freq: 869.618 MHz | SF8 CR5 BW 62.5kHz | TX: 22dBm | Bat: 4.012V"
        );
    }

    #[test]
    fn quit_sets_should_quit() {
        let mut app = App::new(0);
        app.apply_local(Command::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn radio_commands_are_returned_unhandled() {
        let mut app = App::new(0);
        for cmd in [Command::Contacts, Command::Advert, Command::Battery, Command::GetChannel(1),
                    Command::Name("x".to_string())] {
            let expected = format!("{:?}", cmd);
            let returned = app.apply_local(cmd).expect("radio command was handled locally");
            assert_eq!(format!("{:?}", returned), expected);
        }
        assert!(app.messages.is_empty());
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
        app.selected_contact = Some(0);
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
        app.selected_contact = Some(4);
        assert_eq!(app.target_label(), "CH#0");
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
