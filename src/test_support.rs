use meshcore_rs::events::{
    BatteryInfo, ChannelMessage, Contact, ContactMessage, CoreStatsData, LogData, MeshPacketHeader, PacketStatsData,
    RadioStatsData, RawAdvertisement,
};
use meshcore_rs::{PayloadType, RouteType};

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

/// A flood packet's header with one-byte hop hashes
pub fn flood_header(payload_type: PayloadType, path: &[u8]) -> MeshPacketHeader {
    MeshPacketHeader {
        route_type: RouteType::Flood,
        payload_type,
        payload_version: 0,
        transport_code: None,
        path_len: path.len() as u8,
        path_hash_size: 1,
        path: path.to_vec(),
    }
}

/// A channel message heard over `path`: channel hash 0xd9, then `payload_len - 1` more bytes
pub fn group_text_log(path: &[u8], payload_len: usize) -> LogData {
    let mut payload = vec![0xd9];
    payload.resize(payload_len, 0x5a);
    LogData {
        snr: 9.25,
        rssi: -28,
        header: Some(flood_header(PayloadType::GroupText, path)),
        advertisement: None,
        payload,
    }
}

/// An advert from `name`, heard over two hops
pub fn advert_log(name: &str) -> LogData {
    LogData {
        snr: -2.75,
        rssi: -112,
        header: Some(flood_header(PayloadType::Advert, &[0x11, 0x22])),
        advertisement: Some(RawAdvertisement {
            public_key: [0xab; 32],
            timestamp: 0,
            signature: [0; 64],
            adv_type: 2,
            lat: None,
            lon: None,
            name: Some(name.to_string()),
        }),
        payload: vec![0; 101],
    }
}

pub fn last_message(app: &App) -> &str {
    &app.messages.last().expect("no messages").text
}

/// The UI in the classic theme, showing the Overview
pub fn test_ui() -> Ui {
    Ui::new(Theme::CLASSIC)
}
