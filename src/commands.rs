use std::sync::Arc;

use meshcore_rs::MeshCore;
use tokio::sync::Mutex;

use crate::app::{App, ChannelEntry};
use crate::text::sanitize;

/// Generate a channel secret from the operating system's secure random source
fn random_secret() -> Result<[u8; 16], getrandom::Error> {
    let mut secret = [0u8; 16];
    getrandom::fill(&mut secret)?;
    Ok(secret)
}

// ── Slash Commands ───────────────────────────────────────────────────────────

/// Describes a slash command for parsing, usage errors and /help
pub struct CommandSpec {
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
pub enum Command {
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
    meshcore_rs::parsing::hex_decode(hex)
        .ok()
        .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
        .ok_or_else(|| "Secret must be 32 hex chars (16 bytes)".to_string())
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

/// Program a channel, generating a secret if none was given; returns the secret used
async fn set_channel(mc: &MeshCore, idx: u8, name: &str, secret: Option<[u8; 16]>) -> Result<[u8; 16], String> {
    let secret = secret
        .map_or_else(random_secret, Ok)
        .map_err(|e| format!("Could not generate channel secret: {}", e))?;
    mc.commands()
        .lock()
        .await
        .set_channel(idx, name, &secret)
        .await
        .map_err(|e| format!("Set channel: {}", e))?;
    Ok(secret)
}

pub async fn handle_command(input: &str, app: &Arc<Mutex<App>>, mc: Option<&MeshCore>) {
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
        Command::SetChannel { idx, name, secret } => match set_channel(mc, idx, &name, secret).await {
            Ok(secret) => {
                let secret_hex = meshcore_rs::parsing::hex_encode(&secret);
                let mut a = app.lock().await;
                a.sys(&format!(
                    "Channel #{} set: name='{}' secret={}",
                    idx, name, secret_hex
                ));
                a.channels.insert(idx, ChannelEntry { name, secret });
            }
            Err(e) => app.lock().await.err(&e),
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::app::MsgType;
    use meshcore_rs::events::SelfInfo;

    // ── Slash commands ───────────────────────────────────────────────────────

    #[test]
    fn random_secret_is_not_a_repeated_byte() {
        let secret = random_secret().unwrap();
        assert!(secret.iter().any(|b| *b != secret[0]), "secret {:02x?} is one repeated byte", secret);
    }

    #[test]
    fn random_secrets_differ_between_calls() {
        assert_ne!(random_secret().unwrap(), random_secret().unwrap());
    }

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
        assert_eq!(parse_command("/setch 1 Ops 00112233445566778899aabbccddeeff00"), bad_secret);
        assert_eq!(parse_command("/setch 1 Ops 00112233445566778899aabbccddeef"), bad_secret);
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
}
