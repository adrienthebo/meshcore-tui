use std::collections::VecDeque;

use meshcore_rs::events::RadioStatsData;

/// Readings kept for the noise floor graph: 20 minutes at one poll every 5 seconds
pub const NOISE_HISTORY_LEN: usize = 240;

/// The radio's latest diagnostics and its recent noise floor readings
#[derive(Debug, Default)]
pub struct RadioStats {
    /// Most recent successful reading
    pub latest: Option<RadioStatsData>,
    /// Noise floor in dBm per poll, oldest first; None marks a failed poll
    pub noise_history: VecDeque<Option<i16>>,
    /// A poll was sent and its result hasn't arrived
    pending: bool,
    /// The most recent poll failed
    failing: bool,
}

/// A change in polling the user should hear about
#[derive(Debug, PartialEq)]
pub enum PollNotice {
    Failed(String),
    Recovered,
}

impl RadioStats {
    /// Start a poll unless one is already in flight; returns whether to send it
    pub fn start_poll(&mut self) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        true
    }

    /// Record a poll result; returns a notice when polling starts failing or recovers
    pub fn record(&mut self, result: Result<RadioStatsData, String>) -> Option<PollNotice> {
        self.pending = false;
        self.noise_history.push_back(result.as_ref().ok().map(|s| s.noise_floor));
        if self.noise_history.len() > NOISE_HISTORY_LEN {
            self.noise_history.pop_front();
        }
        let was_failing = self.failing;
        self.failing = result.is_err();
        match result {
            Ok(stats) => {
                self.latest = Some(stats);
                was_failing.then_some(PollNotice::Recovered)
            }
            Err(e) => (!was_failing).then_some(PollNotice::Failed(e)),
        }
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
}
