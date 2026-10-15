use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{layout::Rect, Frame};

use crate::app::App;
use crate::contacts_tab::ContactsTab;
use crate::overview::Overview;
use crate::radio_tab::RadioTab;
use crate::repeaters_tab::RepeatersTab;
use crate::rx_log_tab::RxLogTab;
use crate::theme::Theme;
use crate::update::Action;

/// A part of the screen with its own view state. It reads the App to draw and to
/// interpret keys, and changes the App only by returning an Action.
pub trait View {
    fn title(&self) -> &str;
    /// Draw into `area`; `focused` views get keys first and show it, e.g. in their border
    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool);
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult;
    /// The keys this view uses right now, e.g. "Enter=send"
    fn key_hints(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// Whether the radio's diagnostics are polled while this view is on screen
    fn shows_diagnostics(&self) -> bool {
        false
    }
}

/// What a view did with a key
#[derive(Debug, PartialEq)]
pub enum KeyResult {
    /// Not a key this view uses
    Ignored,
    /// Used to change the view's own state
    Handled,
    /// Used, and the App should change
    Action(Action),
}

/// What's on screen and how it looks; the radio, contacts and messages are on App
pub struct Ui {
    /// The tabs before the Repeaters tab, in tab bar order
    pub tabs: Vec<Box<dyn View>>,
    /// The last tab, typed so other views can show a repeater on it
    repeaters: RepeatersTab,
    /// Index of the tab on screen; the Repeaters tab's is `tabs.len()`
    pub active: usize,
    pub theme: Theme,
}

impl Ui {
    pub fn new(theme: Theme) -> Self {
        Self {
            tabs: vec![
                Box::new(Overview::default()),
                Box::new(ContactsTab::default()),
                Box::new(RadioTab),
                Box::new(RxLogTab::default()),
            ],
            repeaters: RepeatersTab::default(),
            active: 0,
            theme,
        }
    }

    /// Show the Overview, which Ui::new puts first
    pub fn show_overview(&mut self) {
        self.active = 0;
    }

    pub fn active_tab(&self) -> &dyn View {
        match self.tabs.get(self.active) {
            Some(tab) => tab.as_ref(),
            None => &self.repeaters,
        }
    }

    fn active_tab_mut(&mut self) -> &mut dyn View {
        match self.tabs.get_mut(self.active) {
            Some(tab) => tab.as_mut(),
            None => &mut self.repeaters,
        }
    }

    /// Every tab's title, in tab bar order
    pub fn tab_titles(&self) -> Vec<&str> {
        let tabs = self.tabs.iter().map(|tab| tab.title());
        tabs.chain([self.repeaters.title()]).collect()
    }

    /// Apply a key press; Ctrl-C and the F-keys come first, the rest go to the
    /// tab on screen, and Esc quits only when the tab doesn't use it
    pub fn handle_key(&mut self, key: KeyEvent, app: &App) -> Option<Action> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Quit)
            }
            KeyCode::F(number) => {
                if let Some(tab) = self.tab_for_f_key(number) {
                    self.active = tab;
                }
                None
            }
            _ => match self.active_tab_mut().handle_key(key, app) {
                KeyResult::Action(action) => Some(action),
                KeyResult::Ignored if key.code == KeyCode::Esc => Some(Action::Quit),
                KeyResult::Handled | KeyResult::Ignored => None,
            },
        }
    }

    /// The keys that work right now: the tab's, then the ones every tab shares;
    /// Esc quits unless the tab names a use for it
    pub fn key_hints(&self) -> Vec<&'static str> {
        let mut hints = self.active_tab().key_hints();
        let tab_uses_esc = hints.iter().any(|hint| hint.starts_with("Esc="));
        hints.push("F#=tab");
        if !tab_uses_esc {
            hints.push("Esc=quit");
        }
        hints
    }

    /// Index of the tab an F-key shows: F1 is the first in the tab bar
    fn tab_for_f_key(&self, number: u8) -> Option<usize> {
        (number as usize)
            .checked_sub(1)
            .filter(|&tab| tab <= self.tabs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    /// A tab that uses only the 'x' key, submitting "x"
    struct Probe;

    impl View for Probe {
        fn title(&self) -> &str {
            "Probe"
        }
        fn draw(&self, _: &mut Frame, _: Rect, _: &App, _: &Theme, _: bool) {}
        fn handle_key(&mut self, key: KeyEvent, _: &App) -> KeyResult {
            match key.code {
                KeyCode::Char('x') => KeyResult::Action(Action::Submit("x".to_string())),
                _ => KeyResult::Ignored,
            }
        }
    }

    /// The Overview, then the Probe
    fn ui_with_probe() -> Ui {
        let mut ui = test_ui();
        ui.tabs = vec![Box::new(Overview::default()), Box::new(Probe)];
        ui
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn esc_and_ctrl_c_quit_from_any_tab() {
        let mut ui = ui_with_probe();
        let app = App::new(0);
        for tab in 0..2 {
            ui.active = tab;
            assert_eq!(
                ui.handle_key(key(KeyCode::Esc, KeyModifiers::NONE), &app),
                Some(Action::Quit)
            );
            assert_eq!(
                ui.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &app),
                Some(Action::Quit)
            );
        }
    }

    /// A tab that uses Esc, as one with a popover open does
    struct EscProbe;

    impl View for EscProbe {
        fn title(&self) -> &str {
            "EscProbe"
        }
        fn draw(&self, _: &mut Frame, _: Rect, _: &App, _: &Theme, _: bool) {}
        fn handle_key(&mut self, key: KeyEvent, _: &App) -> KeyResult {
            match key.code {
                KeyCode::Esc => KeyResult::Handled,
                _ => KeyResult::Ignored,
            }
        }
        fn key_hints(&self) -> Vec<&'static str> {
            vec!["Esc=close"]
        }
    }

    #[test]
    fn a_tab_that_uses_esc_keeps_it_from_quitting() {
        let mut ui = test_ui();
        ui.tabs = vec![Box::new(EscProbe)];
        let app = App::new(0);
        assert_eq!(
            ui.handle_key(key(KeyCode::Esc, KeyModifiers::NONE), &app),
            None
        );
        assert_eq!(
            ui.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), &app),
            Some(Action::Quit)
        );
    }

    #[test]
    fn key_hints_leave_out_quit_when_the_tab_uses_esc() {
        let mut ui = test_ui();
        ui.tabs = vec![Box::new(EscProbe)];
        assert_eq!(ui.key_hints(), ["Esc=close", "F#=tab"]);
    }

    #[test]
    fn f_keys_number_the_tabs_from_one() {
        let mut ui = ui_with_probe();
        let app = App::new(0);
        ui.handle_key(key(KeyCode::F(2), KeyModifiers::NONE), &app);
        assert_eq!(ui.active_tab().title(), "Probe");
        ui.handle_key(key(KeyCode::F(3), KeyModifiers::NONE), &app);
        assert_eq!(ui.active_tab().title(), "Repeaters");
        ui.handle_key(key(KeyCode::F(1), KeyModifiers::NONE), &app);
        assert_eq!(ui.active_tab().title(), "Overview");
    }

    #[test]
    fn the_repeaters_tab_is_last_and_gets_keys() {
        let mut ui = test_ui();
        let app = App::new(0);
        assert_eq!(
            ui.tab_titles(),
            ["Overview", "Contacts", "Radio", "RX Log", "Repeaters"]
        );
        ui.handle_key(key(KeyCode::F(5), KeyModifiers::NONE), &app);
        assert_eq!(ui.key_hints(), ["↑↓=browse", "F#=tab", "Esc=quit"]);
        assert_eq!(
            ui.handle_key(key(KeyCode::Esc, KeyModifiers::NONE), &app),
            Some(Action::Quit)
        );
    }

    #[test]
    fn f_keys_without_a_tab_change_nothing() {
        let mut ui = ui_with_probe();
        let app = App::new(0);
        for number in [0, 4, 12] {
            assert_eq!(
                ui.handle_key(key(KeyCode::F(number), KeyModifiers::NONE), &app),
                None
            );
            assert_eq!(ui.active, 0);
        }
    }

    #[test]
    fn key_hints_add_the_shared_keys_to_the_tabs() {
        let mut ui = ui_with_probe();
        assert_eq!(
            ui.key_hints(),
            ["Enter=send", "Tab=panel", "F#=tab", "Esc=quit"]
        );
        ui.active = 1;
        assert_eq!(ui.key_hints(), ["F#=tab", "Esc=quit"]);
    }

    #[test]
    fn other_keys_go_to_the_tab_on_screen() {
        let mut ui = ui_with_probe();
        let app = App::new(0);
        ui.active = 1;
        assert_eq!(
            ui.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE), &app),
            Some(Action::Submit("x".to_string()))
        );
        assert_eq!(
            ui.handle_key(key(KeyCode::Char('y'), KeyModifiers::NONE), &app),
            None
        );
    }
}
