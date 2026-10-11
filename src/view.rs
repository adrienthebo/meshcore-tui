use crate::input::InputLine;

/// The views the app can show; app state stays on App whichever one is showing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Chat, contacts, packets and the radio
    Overview,
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
