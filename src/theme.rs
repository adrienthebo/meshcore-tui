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

    /// Neon pastels on the terminal's own background; needs a truecolor terminal
    pub const VAPORWAVE: Theme = Theme {
        text: WHITE,
        muted: DUSTY_VIOLET,
        focus: HOT_PINK,
        info: CYAN,
        value: PALE_YELLOW,
        good: MINT,
        bad: CORAL,

        status_border: LAVENDER,
        messages_border: LAVENDER,
        contacts_border: LAVENDER,
        packets_border: LAVENDER,
        radio_border: LAVENDER,
        input_border: CYAN,

        msg_incoming: CYAN,
        msg_outgoing: MINT,
        msg_channel: PALE_YELLOW,

        packet_advert: HOT_PINK,
        packet_ack: CYAN,
        packet_sent: MINT,
        packet_path: LAVENDER,

        selected_fg: DEEP_PURPLE,
        selected_bg: HOT_PINK,

        graph_low: CYAN,
        graph_mid: LAVENDER,
        graph_high: HOT_PINK,
    };
}

const HOT_PINK: Color = Color::Rgb(0xff, 0x71, 0xce);
const LAVENDER: Color = Color::Rgb(0xb9, 0x67, 0xff);
const CYAN: Color = Color::Rgb(0x01, 0xcd, 0xfe);
const MINT: Color = Color::Rgb(0x05, 0xff, 0xa1);
const PALE_YELLOW: Color = Color::Rgb(0xff, 0xfb, 0x96);
const CORAL: Color = Color::Rgb(0xff, 0x4f, 0x81);
const DUSTY_VIOLET: Color = Color::Rgb(0x8c, 0x7b, 0xa8);
const WHITE: Color = Color::Rgb(0xf4, 0xf0, 0xff);
const DEEP_PURPLE: Color = Color::Rgb(0x1a, 0x10, 0x33);

/// The themes selectable with --theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ThemeName {
    Vaporwave,
    Classic,
}

impl ThemeName {
    pub fn theme(self) -> Theme {
        match self {
            ThemeName::Vaporwave => Theme::VAPORWAVE,
            ThemeName::Classic => Theme::CLASSIC,
        }
    }
}
