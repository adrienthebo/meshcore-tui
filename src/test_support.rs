use meshcore_rs::events::{
    BatteryInfo, ChannelMessage, Contact, ContactMessage, CoreStatsData, PacketStatsData, RadioStatsData,
};

use crate::app::{App, ChannelEntry};
use crate::stats::{ClockReading, DiagnosticsReading};
use crate::theme::Theme;
use crate::view::Ui;

pub fn contact_message(prefix: [u8; 6], text: &str) -> ContactMessage {
    ContactMessage {
        sender_prefix: prefix,
        path_len: 0,
        txt_type: 0,
        sender_timestamp: 0,
        text: text.to_string(),
        snr: None,
        signature: None,
    }
}

pub fn channel_message(channel_idx: u8, text: &str) -> ChannelMessage {
    ChannelMessage {
        channel_idx,
        path_len: 0,
        txt_type: 0,
        sender_timestamp: 0,
        text: text.to_string(),
        snr: None,
    }
}

pub fn radio_stats(noise_floor: i16) -> RadioStatsData {
    RadioStatsData {
        noise_floor,
        last_rssi: -90,
        last_snr: 5.25,
        tx_air_secs: 12,
        rx_air_secs: 340,
    }
}

/// A diagnostics poll where every read succeeded: up 3d4h, battery 4.02 V
pub fn diagnostics_reading() -> DiagnosticsReading {
    DiagnosticsReading {
        core: Ok(CoreStatsData { battery_mv: 4020, uptime_secs: 273_600, errors: 0, queue_len: 0 }),
        packets: Ok(PacketStatsData {
            recv: 1292,
            sent: 49,
            flood_tx: 37,
            direct_tx: 12,
            flood_rx: 1204,
            direct_rx: 88,
            recv_errors: Some(3),
        }),
        battery: Ok(BatteryInfo { battery_mv: 4020, used_kb: Some(112), total_kb: Some(1024) }),
        clock: Ok(ClockReading { radio: 1_791_719_995, host: 1_791_720_000 }),
    }
}

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

/// The UI in the classic theme, showing the Overview
pub fn test_ui() -> Ui {
    Ui::new(Theme::CLASSIC)
}
