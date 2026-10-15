use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::events::Contact;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use crate::app::{App, Role};
use crate::commands::REPEATER;
use crate::contacts_tab::Step;
use crate::text::sanitize;
use crate::theme::Theme;
use crate::ui::panel;
use crate::view::{KeyResult, View};

/// The repeaters we're logged in to, beside the shown one's status
#[derive(Debug, Default)]
pub struct RepeatersTab {
    /// The browsed repeater's public key; the first repeater is shown while
    /// none is browsed or we're no longer logged in to it
    browsed: Option<[u8; 32]>,
    /// How far the list has scrolled; drawing selects the shown repeater's
    /// row and scrolls to keep it in view
    list: RefCell<TableState>,
}

/// Columns for the list of repeaters: a name and a role
const LIST_WIDTH: u16 = 28;

const NOT_LOGGED_IN: &str = "Not logged in to any repeater. Log in from a repeater's menu in \
    Contacts, or with /login <name> [password].";

impl View for RepeatersTab {
    fn title(&self) -> &str {
        "Repeaters"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_repeaters_tab(f, app, self, theme, area);
    }

    /// ↑↓ browse the repeaters
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match key.code {
            KeyCode::Up => self.browse(app, Step::Up),
            KeyCode::Down => self.browse(app, Step::Down),
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }

    fn key_hints(&self) -> Vec<&'static str> {
        vec!["↑↓=browse"]
    }
}

impl RepeatersTab {
    /// Show the repeater with this public key, while we're logged in to it
    pub fn show(&mut self, key: [u8; 32]) {
        self.browsed = Some(key);
    }

    /// Move the browsed row from the one shown
    fn browse(&mut self, app: &App, step: Step) {
        let repeaters = logged_in(app);
        let Some(row) = self.shown_row(&repeaters) else {
            return;
        };
        let row = match step {
            Step::Up => row.saturating_sub(1),
            Step::Down => (row + 1).min(repeaters.len() - 1),
        };
        self.browsed = Some(repeaters[row].0.public_key);
    }

    /// The browsed repeater's row, else the first; None when there are none
    fn shown_row(&self, repeaters: &[(&Contact, Role)]) -> Option<usize> {
        let browsed = self
            .browsed
            .and_then(|key| repeaters.iter().position(|(c, _)| c.public_key == key));
        browsed.or((!repeaters.is_empty()).then_some(0))
    }
}

/// The repeaters we're logged in to, by name, with our role on each
fn logged_in(app: &App) -> Vec<(&Contact, Role)> {
    let mut repeaters: Vec<(&Contact, Role)> = app
        .contacts
        .iter()
        .filter(|c| c.contact_type == REPEATER)
        .filter_map(|c| Some((c, *app.logins.get(&c.public_key)?)))
        .collect();
    repeaters.sort_by(|(a, _), (b, _)| a.adv_name.cmp(&b.adv_name));
    repeaters
}

fn draw_repeaters_tab(f: &mut Frame, app: &App, tab: &RepeatersTab, theme: &Theme, area: Rect) {
    let repeaters = logged_in(app);
    let row = tab.shown_row(&repeaters);
    let [list, status] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LIST_WIDTH), Constraint::Fill(1)])
        .areas(area);
    draw_list(f, &repeaters, row, tab, theme, list);
    draw_status(f, row.map(|row| repeaters[row]), theme, status);
}

fn draw_list(
    f: &mut Frame,
    repeaters: &[(&Contact, Role)],
    row: Option<usize>,
    tab: &RepeatersTab,
    theme: &Theme,
    area: Rect,
) {
    let rows: Vec<Row> = repeaters
        .iter()
        .map(|(c, role)| Row::new([sanitize(&c.adv_name), role.label().to_string()]))
        .collect();
    let table = Table::new(rows, [Constraint::Fill(1), Constraint::Length(10)])
        .style(Style::default().fg(theme.text))
        .row_highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg))
        .block(panel(
            theme,
            format!(" Logged in ({}) ", repeaters.len()),
            theme.focus,
        ));
    let mut state = tab.list.borrow_mut();
    state.select(row);
    f.render_stateful_widget(table, area, &mut state);
}

/// The shown repeater's name, key and our role, above its status; without
/// one, how to log in
fn draw_status(f: &mut Frame, shown: Option<(&Contact, Role)>, theme: &Theme, area: Rect) {
    let (title, text) = match shown {
        Some((repeater, role)) => (
            format!(
                " {} [{}] · {} ",
                sanitize(&repeater.adv_name),
                &repeater.prefix_hex()[..8],
                role.label()
            ),
            "No status yet.",
        ),
        None => (" Status ".to_string(), NOT_LOGGED_IN),
    };
    let text = Paragraph::new(text)
        .style(Style::default().fg(theme.muted))
        .wrap(Wrap { trim: true })
        .block(panel(theme, title, theme.radio_border));
    f.render_widget(text, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ROOM;
    use crate::test_support::*;
    use crossterm::event::KeyModifiers;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(app: &App, tab: &RepeatersTab) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal
            .draw(|f| draw_repeaters_tab(f, app, tab, &Theme::CLASSIC, f.area()))
            .unwrap();
        terminal.backend().clone()
    }

    fn server(name: &str, byte: u8, contact_type: u8) -> Contact {
        let mut server = contact(name, [byte; 6]);
        server.contact_type = contact_type;
        server
    }

    /// Logged in to Hilltop as admin and Beacon as a guest, and to a room;
    /// not logged in to Valley
    fn app_with_logins() -> App {
        let mut app = App::new(0);
        app.contacts = vec![
            server("Hilltop", 0xb2, REPEATER),
            server("Valley", 0xc3, REPEATER),
            server("Hill Room", 0xd4, ROOM),
            server("Beacon", 0xe5, REPEATER),
        ];
        for (byte, role) in [
            (0xb2, Role::Admin),
            (0xd4, Role::ReadWrite),
            (0xe5, Role::Guest),
        ] {
            let mut key = [0; 32];
            key[..6].copy_from_slice(&[byte; 6]);
            app.logins.insert(key, role);
        }
        app
    }

    fn shown_name(tab: &RepeatersTab, app: &App) -> String {
        let repeaters = logged_in(app);
        let row = tab.shown_row(&repeaters).unwrap();
        repeaters[row].0.adv_name.clone()
    }

    fn press(tab: &mut RepeatersTab, app: &App, code: KeyCode) -> KeyResult {
        tab.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app)
    }

    #[test]
    fn without_logins_says_how_to_log_in() {
        insta::assert_snapshot!(render(&App::new(0), &RepeatersTab::default()));
    }

    #[test]
    fn lists_the_repeaters_logged_in_to_beside_the_first() {
        insta::assert_snapshot!(render(&app_with_logins(), &RepeatersTab::default()));
    }

    #[test]
    fn arrows_browse_the_repeaters_and_stop_at_the_ends() {
        let app = app_with_logins();
        let mut tab = RepeatersTab::default();
        assert_eq!(shown_name(&tab, &app), "Beacon");
        assert_eq!(press(&mut tab, &app, KeyCode::Down), KeyResult::Handled);
        assert_eq!(shown_name(&tab, &app), "Hilltop");
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(shown_name(&tab, &app), "Hilltop");
        press(&mut tab, &app, KeyCode::Up);
        press(&mut tab, &app, KeyCode::Up);
        assert_eq!(shown_name(&tab, &app), "Beacon");
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn show_picks_the_repeater_to_show() {
        let app = app_with_logins();
        let mut tab = RepeatersTab::default();
        tab.show(app.contacts[0].public_key);
        assert_eq!(shown_name(&tab, &app), "Hilltop");
    }

    #[test]
    fn a_repeater_logged_out_of_shows_the_first() {
        let mut app = app_with_logins();
        let mut tab = RepeatersTab::default();
        press(&mut tab, &app, KeyCode::Down);
        app.logins.retain(|_, role| *role != Role::Admin);
        assert_eq!(shown_name(&tab, &app), "Beacon");
    }
}
