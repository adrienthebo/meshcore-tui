use ratatui::style::Color;

/// The colour for each role in the UI
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Body text and labels
    pub text: Color,
    /// Timestamps, separators and other secondary text
    pub muted: Color,
    /// Border of the panel that has keyboard focus
    pub focus: Color,
    /// Connection details
    pub info: Color,
    /// Radio parameters
    pub value: Color,
    /// Counters and a live connection
    pub good: Color,
    /// Errors and a lost connection
    pub bad: Color,

    pub status_border: Color,
    pub messages_border: Color,
    pub contacts_border: Color,
    pub packets_border: Color,
    pub radio_border: Color,
    pub input_border: Color,

    pub msg_incoming: Color,
    pub msg_outgoing: Color,
    pub msg_channel: Color,

    pub packet_advert: Color,
    pub packet_ack: Color,
    pub packet_sent: Color,
    pub packet_path: Color,

    /// The selected contact
    pub selected_fg: Color,
    pub selected_bg: Color,

    /// Noise floor bars, from the bottom of the graph's range to the top
    pub graph_low: Color,
    pub graph_mid: Color,
    pub graph_high: Color,
}

impl Theme {
    /// The terminal's named colours
    pub const CLASSIC: Theme = Theme {
        text: Color::White,
        muted: Color::DarkGray,
        focus: Color::Yellow,
        info: Color::Cyan,
        value: Color::Yellow,
        good: Color::Green,
        bad: Color::Red,

        status_border: Color::Cyan,
        messages_border: Color::Cyan,
        contacts_border: Color::Green,
        packets_border: Color::Magenta,
        radio_border: Color::Magenta,
        input_border: Color::Cyan,

        msg_incoming: Color::Cyan,
        msg_outgoing: Color::Green,
        msg_channel: Color::Yellow,

        packet_advert: Color::Magenta,
        packet_ack: Color::Cyan,
        packet_sent: Color::Green,
        packet_path: Color::Blue,

        selected_fg: Color::Black,
        selected_bg: Color::Cyan,

        graph_low: Color::Green,
        graph_mid: Color::Green,
        graph_high: Color::Green,
    };
}
