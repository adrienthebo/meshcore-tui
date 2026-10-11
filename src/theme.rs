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

    /// The vaporwave palette from mmtp's TUI (pkg/tui/shared/styles.go), on the
    /// terminal's own background and text colour; needs a truecolor terminal
    pub const VAPORWAVE: Theme = Theme {
        text: Color::Reset,
        muted: MARDI_GRAS,
        focus: CERISE_PINK,
        info: AQUAMARINE,
        value: CERISE_PINK,
        good: AQUAMARINE,
        bad: PERSIAN_ROSE,

        status_border: MAYA_BLUE,
        messages_border: MAYA_BLUE,
        contacts_border: MAYA_BLUE,
        packets_border: MAYA_BLUE,
        radio_border: MAYA_BLUE,
        input_border: MAYA_BLUE,

        msg_incoming: AQUAMARINE,
        msg_outgoing: LAVENDER_INDIGO,
        msg_channel: RAJAH,

        packet_advert: PINK_LAVENDER,
        packet_ack: AQUAMARINE,
        packet_sent: LAVENDER_INDIGO,
        packet_path: RAJAH,

        selected_fg: Color::Reset,
        selected_bg: RUSSIAN_VIOLET,

        graph_low: AQUAMARINE,
        graph_mid: LAVENDER_INDIGO,
        graph_high: PERSIAN_ROSE,
    };
}

const RUSSIAN_VIOLET: Color = Color::Rgb(0x30, 0x03, 0x50);
const MARDI_GRAS: Color = Color::Rgb(0x94, 0x16, 0x7f);
const CERISE_PINK: Color = Color::Rgb(0xe9, 0x34, 0x79);
const RAJAH: Color = Color::Rgb(0xf9, 0xac, 0x53);
const PERSIAN_ROSE: Color = Color::Rgb(0xf6, 0x2e, 0x97);
const LAVENDER_INDIGO: Color = Color::Rgb(0x9c, 0x5b, 0xff);
const AQUAMARINE: Color = Color::Rgb(0x65, 0xf4, 0xda);
const PINK_LAVENDER: Color = Color::Rgb(0xef, 0xb7, 0xff);
const MAYA_BLUE: Color = Color::Rgb(0x5b, 0xc8, 0xf5);

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
