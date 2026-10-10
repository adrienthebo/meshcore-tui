use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use meshcore_rs::events::EventPayload;
use meshcore_rs::{EventType, MeshCore};
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

/// Subscribe to a radio event type and apply its payload to the app state
async fn on_radio_event(
    mc: &MeshCore,
    app: &Arc<Mutex<App>>,
    event_type: EventType,
    apply: fn(&mut App, EventPayload),
) {
    let app = app.clone();
    mc.subscribe(event_type, HashMap::new(), move |event| {
        let app = app.clone();
        tokio::spawn(async move {
            apply(&mut *app.lock().await, event.payload);
        });
    })
    .await;
}

/// Ring the terminal bell
fn bell() {
    print!("\x07");
}

pub async fn setup_subscriptions(mc: &MeshCore, app: Arc<Mutex<App>>) {
    on_radio_event(mc, &app, EventType::ContactMsgRecv, |a, payload| {
        if let EventPayload::ContactMessage(msg) = payload {
            a.on_contact_message(&msg);
            bell();
        }
    })
    .await;

    on_radio_event(mc, &app, EventType::ChannelMsgRecv, |a, payload| {
        if let EventPayload::ChannelMessage(msg) = payload {
            a.on_channel_message(&msg);
            bell();
        }
    })
    .await;

    // Advertisements (0x80)
    on_radio_event(mc, &app, EventType::Advertisement, |a, payload| {
        if let EventPayload::Advertisement(adv) = payload {
            a.on_advertisement(&adv);
        }
    })
    .await;

    // New contacts / adverts (0x8A PushCodeNewAdvert)
    on_radio_event(mc, &app, EventType::NewContact, |a, payload| {
        if let EventPayload::Contact(contact) = payload {
            a.on_new_contact(contact);
        }
    })
    .await;

    on_radio_event(mc, &app, EventType::Ack, |a, payload| {
        if let EventPayload::Ack { tag } = payload {
            a.on_ack(&tag);
        }
    })
    .await;

    on_radio_event(mc, &app, EventType::PathUpdate, |a, payload| {
        if let EventPayload::PathUpdate(pu) = payload {
            a.on_path_update(&pu);
        }
    })
    .await;

    mc.start_auto_message_fetching().await;
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
