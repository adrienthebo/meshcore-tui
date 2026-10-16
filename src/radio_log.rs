use std::fmt::{Debug, Write};

use tokio::sync::mpsc::UnboundedSender;
use tracing::{
    field::{Field, Visit},
    Event, Level, Subscriber,
};
use tracing_subscriber::{filter::Targets, layer::Context, Layer};

/// A warning or error that meshcore-rs logged
#[derive(Debug, PartialEq)]
pub struct RadioLogLine {
    pub level: Level,
    pub text: String,
}

/// The events worth showing in the message log: meshcore-rs warnings and
/// errors, such as a frame it couldn't parse
pub fn shown() -> Targets {
    Targets::new().with_target("meshcore_rs", Level::WARN)
}

/// A tracing layer that sends each event's message to the main loop, which
/// shows it in the message log
pub struct RadioLog {
    lines: UnboundedSender<RadioLogLine>,
}

impl RadioLog {
    pub fn new(lines: UnboundedSender<RadioLogLine>) -> Self {
        Self { lines }
    }
}

impl<S: Subscriber> Layer<S> for RadioLog {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut text = String::new();
        event.record(&mut MessageText(&mut text));
        // Sending fails only once the main loop has stopped, when there's no
        // message log left to show the line in
        let _ = self.lines.send(RadioLogLine {
            level: *event.metadata().level(),
            text,
        });
    }
}

/// Writes an event's message field into a string
struct MessageText<'a>(&'a mut String);

impl Visit for MessageText<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tracing_subscriber::layer::SubscriberExt;

    /// The lines the layer sends while `log` runs
    fn lines_logged(log: impl FnOnce()) -> Vec<RadioLogLine> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let subscriber =
            tracing_subscriber::registry().with(RadioLog::new(tx).with_filter(shown()));
        tracing::subscriber::with_default(subscriber, log);
        let mut lines = Vec::new();
        while let Ok(line) = rx.try_recv() {
            lines.push(line);
        }
        lines
    }

    #[test]
    fn meshcore_rs_warnings_and_errors_are_sent_with_their_level() {
        let lines = lines_logged(|| {
            tracing::error!(target: "meshcore_rs::meshcore", "Error handling message: {}", "too short");
            tracing::warn!(target: "meshcore_rs::meshcore::ble", "Connection attempt 1 failed");
        });
        assert_eq!(
            lines,
            [
                RadioLogLine {
                    level: Level::ERROR,
                    text: "Error handling message: too short".to_string(),
                },
                RadioLogLine {
                    level: Level::WARN,
                    text: "Connection attempt 1 failed".to_string(),
                },
            ]
        );
    }

    #[test]
    fn quieter_events_and_other_crates_are_not_sent() {
        let lines = lines_logged(|| {
            tracing::info!(target: "meshcore_rs::meshcore::ble", "Connected to MeshCore device");
            tracing::debug!(target: "meshcore_rs::reader", "Unknown packet type");
            tracing::error!(target: "btleplug", "adapter error");
        });
        assert_eq!(lines, []);
    }
}
