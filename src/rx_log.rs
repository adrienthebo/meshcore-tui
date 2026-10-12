use std::collections::VecDeque;

use meshcore_rs::events::LogData;

/// Packets kept for the RX Log tab
pub const RX_LOG_LEN: usize = 5000;

/// Bytes in a packet's header byte, transport code and path length byte
const HEADER_BYTE_LEN: usize = 1;
const TRANSPORT_CODE_LEN: usize = 4;
const PATH_BYTE_LEN: usize = 1;

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
