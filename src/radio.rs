use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use meshcore_rs::events::EventPayload;
use meshcore_rs::{EventType, MeshCore};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::app::App;
use crate::commands::{run_radio_command, Outcome};
use crate::stats::{ClockReading, DiagnosticsReading};
use crate::update::{Effect, Recipient};
use crate::text::sanitize;
use crate::Args;

// ── MeshCore Integration ─────────────────────────────────────────────────────

#[cfg(feature = "ble")]
async fn connect_ble(app: &mut App, device_name: Option<&str>) -> Option<MeshCore> {
    match device_name {
        Some(name) => app.sys(&format!("Scanning BLE for '{}'...", name)),
        None => app.sys("Scanning for any MeshCore BLE device..."),
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
        Ok(mc) => Some(mc),
        Err(e) => {
            app.err(&format!("BLE connect failed: {}", e));
            None
        }
    }
}

#[cfg(not(feature = "ble"))]
async fn connect_ble(app: &mut App, _device_name: Option<&str>) -> Option<MeshCore> {
    app.err("BLE connect failed: built without BLE support");
    None
}

pub async fn connect(app: &mut App, args: &Args) -> Option<MeshCore> {
    let mc = if let Some(ref tcp_addr) = args.tcp {
        let parts: Vec<&str> = tcp_addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts.get(1).unwrap_or(&"4403").parse().unwrap_or(4403);
        app.sys(&format!("Connecting TCP {}:{}...", host, port));
        match MeshCore::tcp(host, port).await {
            Ok(mc) => mc,
            Err(e) => {
                app.err(&format!("TCP connect failed: {}", e));
                return None;
            }
        }
    } else if let Some(ref device_name) = args.ble {
        connect_ble(app, device_name.as_deref()).await?
    } else {
        let port = args.port.as_deref().unwrap_or("/dev/ttyACM0");
        app.port_info = port.to_string();
        app.sys(&format!("Connecting serial {} @ {}...", port, args.baud));
        match MeshCore::serial(port, args.baud).await {
            Ok(mc) => mc,
            Err(e) => {
                app.err(&format!("Serial connect failed: {}", e));
                return None;
            }
        }
    };

    // AppStart → get SelfInfo
    match mc.commands().lock().await.send_appstart().await {
        Ok(info) => {
            app.sys(&format!("Connected: {} (freq: {} MHz, SF{}, CR{}, BW {} kHz)",
                sanitize(&info.name),
                info.radio_freq as f64 / 1_000.0,
                info.sf,
                info.cr,
                info.radio_bw as f64 / 1_000.0,
            ));
            app.connected = true;
            if app.port_info.is_empty() {
                app.port_info = if args.ble.is_some() {
                    format!("BLE: {}", sanitize(&info.name))
                } else {
                    "TCP".into()
                };
            }
            app.self_info = Some(info);
        }
        Err(e) => {
            app.err(&format!("AppStart failed: {}", e));
            return None;
        }
    }

    // Firmware and model
    match mc.commands().lock().await.send_device_query().await {
        Ok(info) => app.device_info = Some(info),
        Err(e) => app.sys(&format!("Device query warning: {}", e)),
    }

    // Sync time
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;
        if let Err(e) = mc.commands().lock().await.set_time(ts).await {
            app.sys(&format!("Time sync warning: {}", e));
        }
    }

    // Battery
    if let Ok(bat) = mc.commands().lock().await.get_bat().await {
        app.battery_level = Some(bat.battery_mv);
        app.sys(&format!("Battery: {:.3}V", bat.battery_mv as f64 / 1000.0));
    }

    // Contacts (BLE needs longer timeout)
    let contacts_timeout = if args.ble.is_some() {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(5)
    };
    match mc.commands().lock().await.get_contacts_with_timeout(0, contacts_timeout).await {
        Ok(contacts) => {
            app.sys(&format!("Loaded {} contacts", contacts.len()));
            app.set_contacts(contacts);
        }
        Err(e) => {
            app.err(&format!("Failed to load contacts: {}", e));
        }
    }

    // Load channels sequentially
    {
        let mut names = Vec::new();
        for idx in 0..16u8 {
            match mc.commands().lock().await.get_channel(idx).await {
                Ok(info) => names.extend(app.store_channel(idx, &info)),
                Err(_) => continue,
            }
        }
        if !names.is_empty() {
            app.sys(&format!("Loaded {} channels: {}", names.len(), names.join(", ")));
        }
    }

    Some(mc)
}

/// Forward the radio events the app displays into `events`, for the main loop to apply
pub async fn setup_subscriptions(mc: &MeshCore, events: UnboundedSender<EventPayload>) {
    let event_types = [
        EventType::ContactMsgRecv,
        EventType::ChannelMsgRecv,
        // Advertisements (0x80)
        EventType::Advertisement,
        // New contacts / adverts (0x8A PushCodeNewAdvert)
        EventType::NewContact,
        EventType::Ack,
        EventType::PathUpdate,
        // RF log (0x88): the signal of every packet the radio receives
        EventType::LogData,
    ];
    for event_type in event_types {
        let events = events.clone();
        mc.subscribe(event_type, HashMap::new(), move |event| {
            // Sending only fails once the main loop has exited, when events no longer matter
            let _ = events.send(event.payload);
        })
        .await;
    }

    mc.start_auto_message_fetching().await;
}

/// Run radio effects one at a time in arrival order, reporting each result on `outcomes`
pub async fn run_radio_effects(
    mc: Arc<MeshCore>,
    mut effects: UnboundedReceiver<Effect>,
    outcomes: UnboundedSender<Result<Outcome, String>>,
) {
    while let Some(effect) = effects.recv().await {
        let outcome = match effect {
            Effect::Radio(cmd) => run_radio_command(&mc, cmd).await,
            Effect::Send { to, text } => send_text(&mc, to, text).await,
            Effect::PollRadioStats => read_radio_stats(&mc).await,
            Effect::PollDiagnostics => Ok(Outcome::Diagnostics(read_diagnostics(&mc).await)),
            // The main loop handles alerts itself
            Effect::Alert => continue,
        };
        if outcomes.send(outcome).is_err() {
            // The main loop has exited
            break;
        }
    }
}

/// Read the radio's stats; a failed read is part of the outcome so polling can report it once
async fn read_radio_stats(mc: &MeshCore) -> Result<Outcome, String> {
    let stats = mc.commands().lock().await.get_radio_stats().await.map_err(|e| e.to_string());
    Ok(Outcome::RadioStats(stats))
}

/// Read the Radio tab's diagnostics one after another; each read can fail on its own
async fn read_diagnostics(mc: &MeshCore) -> DiagnosticsReading {
    let err = |e: meshcore_rs::Error| e.to_string();
    let core = mc.commands().lock().await.get_core_stats().await.map_err(err);
    let packets = mc.commands().lock().await.get_packet_stats().await.map_err(err);
    let battery = mc.commands().lock().await.get_bat().await.map_err(err);
    let clock = mc.commands().lock().await.get_time().await.map_err(err)
        .map(|radio| ClockReading { radio, host: Utc::now().timestamp() });
    DiagnosticsReading { core, packets, battery, clock }
}

/// Send a chat message; the error is the text to show the user
async fn send_text(mc: &MeshCore, to: Recipient, text: String) -> Result<Outcome, String> {
    match to {
        Recipient::Contact(contact) => mc.commands().lock().await.send_msg(&contact, &text, None).await
            .map(|info| Outcome::DirectSent { contact, text, info })
            .map_err(|e| format!("Send failed: {}", e)),
        Recipient::Channel(channel) => mc.commands().lock().await.send_channel_msg(channel, &text, None).await
            .map(|_| Outcome::ChannelSent { channel, text })
            .map_err(|e| format!("Channel send failed: {}", e)),
    }
}
