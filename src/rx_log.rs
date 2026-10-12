use std::collections::VecDeque;

use meshcore_rs::events::{LogData, MeshPacketHeader};
use meshcore_rs::parsing::hex_encode;
use meshcore_rs::{PayloadType, RouteType};

/// Packets kept for the RX Log tab
pub const RX_LOG_LEN: usize = 5000;

/// Bytes in a packet's header byte, transport code and path length byte
const HEADER_BYTE_LEN: usize = 1;
const TRANSPORT_CODE_LEN: usize = 4;
const PATH_BYTE_LEN: usize = 1;

/// Where the header byte keeps the payload type and version (bits 0-1 are the route)
const PAYLOAD_TYPE_SHIFT: u8 = 2;
const PAYLOAD_VERSION_SHIFT: u8 = 6;
/// Where the path byte keeps the hop hash size less one (bits 0-5 are the hop count)
const HASH_SIZE_SHIFT: u8 = 6;

/// A packet the radio received over the air, as its RX log push reported it
#[derive(Clone, Debug)]
pub struct RxPacket {
    /// Counts up from 0 in arrival order, so a packet keeps its id as older ones drop out
    pub id: u64,
    pub timestamp: String,
    pub log: LogData,
}

impl RxPacket {
    /// The packet's length on air; 0 when the push carried no packet
    pub fn len(&self) -> usize {
        let header = self.log.header.as_ref().map_or(0, |h| {
            let transport = if h.transport_code.is_some() { TRANSPORT_CODE_LEN } else { 0 };
            HEADER_BYTE_LEN + transport + PATH_BYTE_LEN + h.path.len()
        });
        header + self.log.payload.len()
    }
}

/// The most recent packets the radio received, oldest first
#[derive(Debug, Default)]
pub struct RxLog {
    pub packets: VecDeque<RxPacket>,
    next_id: u64,
}

impl RxLog {
    /// Keep a received packet, dropping the oldest beyond RX_LOG_LEN
    pub fn record(&mut self, timestamp: String, log: LogData) {
        if self.packets.len() == RX_LOG_LEN {
            self.packets.pop_front();
        }
        self.packets.push_back(RxPacket { id: self.next_id, timestamp, log });
        self.next_id += 1;
    }
}

/// Payload types by their names in the firmware's PAYLOAD_TYPE_* (Packet.h);
/// "?" is a type meshcore-rs doesn't know
pub fn payload_type_label(payload_type: PayloadType) -> &'static str {
    match payload_type {
        PayloadType::Req => "REQ",
        PayloadType::Response => "RESPONSE",
        PayloadType::TextMsg => "TXT_MSG",
        PayloadType::Ack => "ACK",
        PayloadType::Advert => "ADVERT",
        PayloadType::GroupText => "GRP_TXT",
        PayloadType::GroupData => "GRP_DATA",
        PayloadType::AnonReq => "ANON_REQ",
        PayloadType::Path => "PATH",
        PayloadType::Trace => "TRACE",
        PayloadType::Multipart => "MULTIPART",
        PayloadType::Control => "CONTROL",
        PayloadType::RawCustom => "RAW_CUSTOM",
        PayloadType::Unknown => "?",
    }
}

/// Routes, with "t-" for the ones that carry a transport code
pub fn route_label(route_type: RouteType) -> &'static str {
    match route_type {
        RouteType::Flood => "flood",
        RouteType::Direct => "direct",
        RouteType::TransportFlood => "t-flood",
        RouteType::TransportDirect => "t-direct",
    }
}

/// A run of a packet's bytes and what they mean
#[derive(Debug, PartialEq)]
pub struct Field {
    pub name: &'static str,
    /// Where the field starts in the packet
    pub offset: usize,
    pub len: usize,
    pub value: String,
}

/// A packet's bytes as the radio received them, and the fields they hold
#[derive(Debug, Default, PartialEq)]
pub struct PacketLayout {
    pub bytes: Vec<u8>,
    pub fields: Vec<Field>,
}

impl PacketLayout {
    /// Rebuild a packet from what meshcore-rs decoded of it. None when the push
    /// carried no packet, or one whose payload type meshcore-rs doesn't know,
    /// since then the header byte can't be rebuilt.
    pub fn of(log: &LogData) -> Option<Self> {
        let header = log.header.as_ref()?;
        if header.payload_type == PayloadType::Unknown {
            return None;
        }
        let mut layout = Self::default();
        layout.push("header", &[header_byte(header)], format!(
            "route {} · type {} · ver {}",
            route_label(header.route_type),
            payload_type_label(header.payload_type),
            header.payload_version
        ));
        if let Some(code) = header.transport_code {
            layout.push("transport code", &code, hex_encode(&code));
        }
        let path_byte = ((header.path_hash_size - 1) << HASH_SIZE_SHIFT) | header.path_len;
        layout.push("path len", &[path_byte], format!(
            "{} × {}",
            plural(header.path_len.into(), "hop"),
            plural(header.path_hash_size.into(), "byte")
        ));
        let hops: Vec<String> = header.path.chunks(header.path_hash_size.into()).map(hex_encode).collect();
        layout.push("path", &header.path, hops.join(" → "));
        layout.push("payload", &log.payload, plural(log.payload.len(), "byte"));
        Some(layout)
    }

    /// Append a field; one with no bytes is left out
    fn push(&mut self, name: &'static str, bytes: &[u8], value: String) {
        if bytes.is_empty() {
            return;
        }
        self.fields.push(Field { name, offset: self.bytes.len(), len: bytes.len(), value });
        self.bytes.extend_from_slice(bytes);
    }
}

fn header_byte(header: &MeshPacketHeader) -> u8 {
    (header.payload_version << PAYLOAD_VERSION_SHIFT)
        | ((header.payload_type as u8) << PAYLOAD_TYPE_SHIFT)
        | header.route_type as u8
}

/// "1 hop", "3 hops"
fn plural(n: usize, unit: &str) -> String {
    if n == 1 {
        format!("1 {}", unit)
    } else {
        format!("{} {}s", n, unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    fn log(rssi: i16) -> LogData {
        LogData { rssi, ..Default::default() }
    }

    #[test]
    fn record_numbers_packets_in_arrival_order() {
        let mut rx_log = RxLog::default();
        rx_log.record("12:00:00".to_string(), log(-90));
        rx_log.record("12:00:01".to_string(), log(-80));
        let kept: Vec<(u64, &str, i16)> =
            rx_log.packets.iter().map(|p| (p.id, p.timestamp.as_str(), p.log.rssi)).collect();
        assert_eq!(kept, vec![(0, "12:00:00", -90), (1, "12:00:01", -80)]);
    }

    #[test]
    fn len_counts_the_header_transport_code_path_and_payload() {
        let mut rx_log = RxLog::default();
        rx_log.record(String::new(), group_text_log(&[0xa1, 0x7f, 0x3c], 50));
        rx_log.record(String::new(), log(0));
        assert_eq!(rx_log.packets[0].len(), 1 + 1 + 3 + 50);
        assert_eq!(rx_log.packets[1].len(), 0);
        let mut transport = group_text_log(&[], 10);
        transport.header.as_mut().unwrap().transport_code = Some([1, 2, 3, 4]);
        assert_eq!(RxPacket { id: 0, timestamp: String::new(), log: transport }.len(), 1 + 4 + 1 + 10);
    }

    /// What meshcore-rs decodes from a packet as the radio received it
    fn decoded(packet: &[u8]) -> LogData {
        let (header, payload) = meshcore_rs::parsing::parse_mesh_packet_header(packet).unwrap();
        LogData { header: Some(header), payload: payload.to_vec(), ..Default::default() }
    }

    #[test]
    fn layout_rebuilds_the_bytes_meshcore_rs_decoded() {
        let packets: [&[u8]; 3] = [
            // Flood channel message over 3 one-byte hops
            &[0x15, 0x03, 0xa1, 0x7f, 0x3c, 0xd9, 0x4e, 0x0b],
            // Version 1 transport-direct ack with a transport code and 2 two-byte hops
            &[0x4f, 1, 2, 3, 4, 0x42, 0xaa, 0xbb, 0xcc, 0xdd, 0xee],
            // Flood advert with no hops yet
            &[0x11, 0x00, 0xab, 0xcd],
        ];
        for packet in packets {
            assert_eq!(PacketLayout::of(&decoded(packet)).unwrap().bytes, packet);
        }
    }

    #[test]
    fn layout_names_each_field_where_it_starts() {
        let layout = PacketLayout::of(&decoded(&[0x4f, 1, 2, 3, 4, 0x42, 0xaa, 0xbb, 0xcc, 0xdd, 0xee])).unwrap();
        let fields: Vec<(&str, usize, usize, &str)> =
            layout.fields.iter().map(|f| (f.name, f.offset, f.len, f.value.as_str())).collect();
        assert_eq!(fields, [
            ("header", 0, 1, "route t-direct · type ACK · ver 1"),
            ("transport code", 1, 4, "01020304"),
            ("path len", 5, 1, "2 hops × 2 bytes"),
            ("path", 6, 4, "aabb → ccdd"),
            ("payload", 10, 1, "1 byte"),
        ]);
    }

    #[test]
    fn layout_leaves_out_an_empty_path() {
        let layout = PacketLayout::of(&decoded(&[0x11, 0x00, 0xab, 0xcd])).unwrap();
        let names: Vec<&str> = layout.fields.iter().map(|f| f.name).collect();
        assert_eq!(names, ["header", "path len", "payload"]);
        assert_eq!(layout.fields[1].value, "0 hops × 1 byte");
    }

    #[test]
    fn layout_needs_a_packet_of_a_known_type() {
        assert_eq!(PacketLayout::of(&log(0)), None);
        // Payload type 12, which the firmware doesn't define yet
        assert_eq!(PacketLayout::of(&decoded(&[12 << 2 | 1, 0x00])), None);
    }

    #[test]
    fn record_drops_the_oldest_beyond_the_limit() {
        let mut rx_log = RxLog::default();
        for _ in 0..RX_LOG_LEN + 2 {
            rx_log.record(String::new(), log(0));
        }
        assert_eq!(rx_log.packets.len(), RX_LOG_LEN);
        assert_eq!(rx_log.packets.front().unwrap().id, 2);
        assert_eq!(rx_log.packets.back().unwrap().id, RX_LOG_LEN as u64 + 1);
    }
}
