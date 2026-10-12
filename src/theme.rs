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
    /// Panel titles; None draws each title in its border's colour
    pub title: Option<Color>,
    /// Connection details
    pub info: Color,
    /// Radio parameters
    pub value: Color,
    /// Counters
    pub good: Color,
    /// Errors and a lost connection
    pub bad: Color,
    /// The connection indicator while connected
    pub connected: Color,

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
    /// The RX Log's selected field and its bytes, set apart from the selected packet
    pub selected_bg_alt: Color,

    /// Signal readings (RSSI, SNR, noise floor) from poor through fair to good
    pub quality_poor: Color,
    pub quality_fair: Color,
    pub quality_good: Color,
}

impl Theme {
    /// The terminal's named colours
    pub const CLASSIC: Theme = Theme {
        text: Color::White,
        muted: Color::DarkGray,
        focus: Color::Yellow,
        title: None,
        info: Color::Cyan,
        value: Color::Yellow,
        good: Color::Green,
        bad: Color::Red,
        connected: Color::Green,

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
        selected_bg_alt: Color::Cyan,

        quality_poor: Color::Red,
        quality_fair: Color::Yellow,
        quality_good: Color::Green,
    };

    /// The vaporwave palette from mmtp's TUI (pkg/tui/shared/styles.go), plus a
    /// burnt orange for the RX Log's field highlight, on the terminal's own
    /// background and text colour; needs a truecolor terminal
    pub const VAPORWAVE: Theme = Theme {
        text: Color::Reset,
        muted: MARDI_GRAS,
        focus: CERISE_PINK,
        title: Some(LAVENDER_INDIGO),
        info: AQUAMARINE,
        value: CERISE_PINK,
        good: AQUAMARINE,
        bad: PERSIAN_ROSE,
        connected: NEON_GREEN,

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
        selected_bg_alt: BURNT_ORANGE,

        quality_poor: PERSIAN_ROSE,
        quality_fair: RAJAH,
        quality_good: AQUAMARINE,
    };

    /// Colour for a reading's quality, from 0.0 (poor) to 1.0 (good). RGB colours
    /// blend smoothly; named colours can't be blended, so they step in thirds.
    pub fn quality(&self, quality: f32) -> Color {
        let quality = quality.clamp(0.0, 1.0);
        match (self.quality_poor, self.quality_fair, self.quality_good) {
            (poor @ Color::Rgb(..), fair @ Color::Rgb(..), good @ Color::Rgb(..)) => {
                if quality < 0.5 {
                    blend(poor, fair, quality * 2.0)
                } else {
                    blend(fair, good, quality * 2.0 - 1.0)
                }
            }
            _ if quality < 1.0 / 3.0 => self.quality_poor,
            _ if quality < 2.0 / 3.0 => self.quality_fair,
            _ => self.quality_good,
        }
    }
}

/// The colour `t` of the way from `start` to `end`; both must be RGB
fn blend(start: Color, end: Color, t: f32) -> Color {
    let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (start, end) else {
        return start;
    };
    let mix = |a: u8, b: u8| (a as f32 + t * (b as f32 - a as f32)).round() as u8;
    Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
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
const NEON_GREEN: Color = Color::Rgb(0x26, 0xff, 0x4e);
/// Not from mmtp's palette: the RX Log's field highlight, which Russian violet left too faint
const BURNT_ORANGE: Color = Color::Rgb(0xcc, 0x55, 0x00);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaporwave_quality_blends_from_poor_through_fair_to_good() {
        let theme = Theme::VAPORWAVE;
        assert_eq!(theme.quality(0.0), PERSIAN_ROSE);
        assert_eq!(theme.quality(0.5), RAJAH);
        assert_eq!(theme.quality(1.0), AQUAMARINE);
        // Halfway from Persian rose (f6 2e 97) to Rajah (f9 ac 53)
        assert_eq!(theme.quality(0.25), Color::Rgb(0xf8, 0x6d, 0x75));
    }

    #[test]
    fn quality_outside_the_range_is_clamped() {
        let theme = Theme::VAPORWAVE;
        assert_eq!(theme.quality(-1.0), PERSIAN_ROSE);
        assert_eq!(theme.quality(2.0), AQUAMARINE);
    }

    #[test]
    fn classic_quality_steps_through_named_colours() {
        let theme = Theme::CLASSIC;
        assert_eq!(theme.quality(0.1), Color::Red);
        assert_eq!(theme.quality(0.5), Color::Yellow);
        assert_eq!(theme.quality(0.9), Color::Green);
    }
}
