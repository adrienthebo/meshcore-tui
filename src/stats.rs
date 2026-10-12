use std::collections::VecDeque;

use meshcore_rs::events::{BatteryInfo, CoreStatsData, PacketStatsData, RadioStatsData};

/// How far back the noise floor and packet signal histories go
pub const HISTORY_SECS: u64 = 20 * 60;

/// Readings kept for the noise floor graph: one per stats poll over the history
pub const NOISE_HISTORY_LEN: usize = (HISTORY_SECS / crate::STATS_POLL_INTERVAL.as_secs()) as usize;

/// The signal of one packet the radio received
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PacketSignal {
    /// When it arrived, in Unix seconds
    pub at: i64,
    pub snr: f32,
    pub rssi: i16,
}

/// The radio's latest diagnostics and its recent noise floor readings
#[derive(Debug, Default)]
pub struct RadioStats {
    /// Most recent successful reading
    pub latest: Option<RadioStatsData>,
    /// Noise floor in dBm per poll, oldest first; None marks a failed poll
    pub noise_history: VecDeque<Option<i16>>,
    /// Packets received over the history, oldest first
    pub packet_signals: VecDeque<PacketSignal>,
    poll: Poll,
}

/// A change in polling the user should hear about
#[derive(Debug, PartialEq)]
pub enum PollNotice {
    Failed(String),
    Recovered,
}

/// A repeating poll: one request in flight at a time, and a failure reported
/// once until polling recovers
#[derive(Debug, Default)]
struct Poll {
    /// A poll was sent and its result hasn't arrived
    pending: bool,
    /// The most recent poll failed
    failing: bool,
}

impl Poll {
    /// Start a poll unless one is already in flight; returns whether to send it
    fn start(&mut self) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        true
    }

    /// Finish the poll in flight, with its error if it failed; returns a notice
    /// when polling starts failing or recovers
    fn finish(&mut self, error: Option<String>) -> Option<PollNotice> {
        self.pending = false;
        let was_failing = self.failing;
        self.failing = error.is_some();
        match error {
            Some(e) => (!was_failing).then_some(PollNotice::Failed(e)),
            None => was_failing.then_some(PollNotice::Recovered),
        }
    }
}

impl RadioStats {
    /// Start a poll unless one is already in flight; returns whether to send it
    pub fn start_poll(&mut self) -> bool {
        self.poll.start()
    }

    /// Record a poll result; returns a notice when polling starts failing or recovers
    pub fn record(&mut self, result: Result<RadioStatsData, String>) -> Option<PollNotice> {
        self.noise_history.push_back(result.as_ref().ok().map(|s| s.noise_floor));
        if self.noise_history.len() > NOISE_HISTORY_LEN {
            self.noise_history.pop_front();
        }
        let notice = self.poll.finish(result.as_ref().err().cloned());
        if let Ok(stats) = result {
            self.latest = Some(stats);
        }
        notice
    }

    /// Record a received packet's signal, dropping packets older than the history
    pub fn record_packet(&mut self, signal: PacketSignal) {
        let oldest = signal.at - HISTORY_SECS as i64;
        while self.packet_signals.front().is_some_and(|p| p.at < oldest) {
            self.packet_signals.pop_front();
        }
        self.packet_signals.push_back(signal);
    }
}

/// The radio's clock, and ours when it was read
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockReading {
    /// Unix seconds by the radio
    pub radio: u32,
    /// Unix seconds by this computer
    pub host: i64,
}

/// One diagnostics poll's results; each read can fail on its own
#[derive(Debug)]
pub struct DiagnosticsReading {
    pub core: Result<CoreStatsData, String>,
    pub packets: Result<PacketStatsData, String>,
    pub battery: Result<BatteryInfo, String>,
    pub clock: Result<ClockReading, String>,
}

/// What the Radio tab shows beyond the noise floor, polled only while it's on screen
#[derive(Debug, Default)]
pub struct Diagnostics {
    /// Each field is the most recent successful read
    pub core: Option<CoreStatsData>,
    pub packets: Option<PacketStatsData>,
    pub battery: Option<BatteryInfo>,
    pub clock: Option<ClockReading>,
    poll: Poll,
}

impl Diagnostics {
    /// Start a poll unless one is already in flight; returns whether to send it
    pub fn start_poll(&mut self) -> bool {
        self.poll.start()
    }

    /// Record a poll's results, keeping the last good value of each read that
    /// failed; returns a notice when polling starts failing or recovers
    pub fn record(&mut self, reading: DiagnosticsReading) -> Option<PollNotice> {
        let errors = [
            reading.core.as_ref().err(),
            reading.packets.as_ref().err(),
            reading.battery.as_ref().err(),
            reading.clock.as_ref().err(),
        ];
        let notice = self.poll.finish(errors.into_iter().flatten().next().cloned());
        self.core = reading.core.ok().or(self.core);
        self.packets = reading.packets.ok().or(self.packets);
        self.battery = reading.battery.ok().or(self.battery.take());
        self.clock = reading.clock.ok().or(self.clock);
        notice
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn only_one_poll_is_in_flight() {
        let mut stats = RadioStats::default();
        assert!(stats.start_poll());
        assert!(!stats.start_poll());
        stats.record(Ok(radio_stats(-110)));
        assert!(stats.start_poll());
    }

    #[test]
    fn readings_and_failures_are_recorded_in_order() {
        let mut stats = RadioStats::default();
        stats.record(Ok(radio_stats(-110)));
        stats.record(Err("timeout".to_string()));
        stats.record(Ok(radio_stats(-104)));
        assert_eq!(stats.noise_history, vec![Some(-110), None, Some(-104)]);
        assert_eq!(stats.latest.unwrap().noise_floor, -104);
    }

    #[test]
    fn history_keeps_the_newest_readings() {
        let mut stats = RadioStats::default();
        for i in 0..NOISE_HISTORY_LEN + 1 {
            stats.record(Ok(radio_stats(-(i as i16))));
        }
        assert_eq!(stats.noise_history.len(), NOISE_HISTORY_LEN);
        assert_eq!(stats.noise_history.front(), Some(&Some(-1)));
    }

    #[test]
    fn failures_are_reported_once_until_recovery() {
        let mut stats = RadioStats::default();
        assert_eq!(stats.record(Ok(radio_stats(-110))), None);
        assert_eq!(stats.record(Err("timeout".to_string())), Some(PollNotice::Failed("timeout".to_string())));
        assert_eq!(stats.record(Err("timeout".to_string())), None);
        assert_eq!(stats.record(Ok(radio_stats(-110))), Some(PollNotice::Recovered));
        assert_eq!(stats.record(Ok(radio_stats(-110))), None);
    }

    #[test]
    fn packet_signals_keep_only_the_history() {
        let mut stats = RadioStats::default();
        let signal = |at| PacketSignal { at, snr: 5.0, rssi: -90 };
        stats.record_packet(signal(0));
        stats.record_packet(signal(10));
        stats.record_packet(signal(HISTORY_SECS as i64 + 5));
        let kept: Vec<i64> = stats.packet_signals.iter().map(|p| p.at).collect();
        assert_eq!(kept, [10, HISTORY_SECS as i64 + 5]);
    }

    #[test]
    fn only_one_diagnostics_poll_is_in_flight() {
        let mut diagnostics = Diagnostics::default();
        assert!(diagnostics.start_poll());
        assert!(!diagnostics.start_poll());
        diagnostics.record(diagnostics_reading());
        assert!(diagnostics.start_poll());
    }

    #[test]
    fn failed_reads_keep_their_last_value_and_report_once() {
        let mut diagnostics = Diagnostics::default();
        assert_eq!(diagnostics.record(diagnostics_reading()), None);
        let failed_battery = || DiagnosticsReading { battery: Err("timeout".to_string()), ..diagnostics_reading() };
        assert_eq!(diagnostics.record(failed_battery()), Some(PollNotice::Failed("timeout".to_string())));
        assert_eq!(diagnostics.record(failed_battery()), None);
        assert_eq!(diagnostics.battery.as_ref().unwrap().battery_mv, 4020);
        assert_eq!(diagnostics.core.unwrap().uptime_secs, 273_600);
        assert_eq!(diagnostics.record(diagnostics_reading()), Some(PollNotice::Recovered));
    }
}
