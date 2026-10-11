use crate::input::InputLine;

/// The views the app can show; app state stays on App whichever one is showing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Chat, contacts, packets and the radio
    Overview,
}

impl Tab {
    /// Every tab, in tab bar order
    pub const ALL: [Tab; 1] = [Tab::Overview];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
        }
    }

    /// The tab Alt+`digit` shows: '1' is the first in the tab bar
    pub fn for_digit(digit: char) -> Option<Tab> {
        Tab::ALL.into_iter().zip('1'..='9').find(|(_, d)| *d == digit).map(|(tab, _)| tab)
    }
}

/// The Overview panel that has keyboard focus
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    Messages,
    Contacts,
    Packets,
}

/// What the Overview tab is showing and editing
#[derive(Debug, Default)]
pub struct Overview {
    pub input: InputLine,
    pub focus: Panel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_number_the_tabs_from_one() {
        assert_eq!(Tab::for_digit('1'), Some(Tab::Overview));
        assert_eq!(Tab::for_digit('0'), None);
        assert_eq!(Tab::for_digit('9'), None);
        assert_eq!(Tab::for_digit('x'), None);
    }
}
