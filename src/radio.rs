use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use meshcore_rs::events::EventPayload;
use meshcore_rs::{EventType, MeshCore};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

use crate::app::{App, ChannelEntry};
use crate::text::sanitize;
use crate::Args;

// ── MeshCore Integration ─────────────────────────────────────────────────────

pub async fn connect(app: &Arc<Mutex<App>>, args: &Args) -> Option<MeshCore> {
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

/// Apply queued radio events to the app; returns true if a chat message arrived
pub fn apply_queued_events(app: &mut App, events: &mut UnboundedReceiver<EventPayload>) -> bool {
    let mut message_arrived = false;
    while let Ok(payload) = events.try_recv() {
        message_arrived |= app.apply_radio_event(payload);
    }
    message_arrived
}

pub async fn send_message(mc: &MeshCore, app: &Arc<Mutex<App>>, text: &str) {
    let (contact, channel) = {
        let a = app.lock().await;
        (
            a.selected_contact.and_then(|idx| a.contacts.get(idx).cloned()),
            a.active_channel,
        )
    };

    if let Some(contact) = contact {
        // Direct message to selected contact
        match mc.commands().lock().await.send_msg(&contact, text, None).await {
            Ok(info) => app.lock().await.on_direct_sent(&contact, text, &info),
            Err(e) => app.lock().await.err(&format!("Send failed: {}", e)),
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
            Ok(_) => app.lock().await.on_channel_sent(channel, text),
            Err(e) => app.lock().await.err(&format!("Channel send failed: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use tokio::sync::mpsc;

    #[test]
    fn queued_events_are_applied_in_arrival_order() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(EventPayload::ChannelMessage(channel_message(0, "Alice: first"))).unwrap();
        tx.send(EventPayload::ChannelMessage(channel_message(0, "Bob: second"))).unwrap();
        let mut app = App::new(0);
        assert!(apply_queued_events(&mut app, &mut rx));
        let texts: Vec<&str> = app.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second"]);
    }

    #[test]
    fn queued_events_without_chat_messages_do_not_alert() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(EventPayload::Ack { tag: [0; 4] }).unwrap();
        let mut app = App::new(0);
        assert!(!apply_queued_events(&mut app, &mut rx));
        assert_eq!(app.packets.len(), 1);
    }

    #[test]
    fn empty_queue_changes_nothing() {
        let (_tx, mut rx) = mpsc::unbounded_channel();
        let mut app = App::new(0);
        assert!(!apply_queued_events(&mut app, &mut rx));
        assert!(app.messages.is_empty());
    }
}
