use std::cell::RefCell;

use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::events::Contact;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Row, Table, TableState},
    Frame,
};

use crate::app::{AdvertEntry, App, Route};
use crate::text::{format_duration, sanitize};
use crate::theme::Theme;
use crate::ui::panel;
use crate::update::Action;
use crate::view::{KeyResult, View};

/// Every contact the radio knows, newest advert first, above the adverts heard
#[derive(Debug, Default)]
pub struct ContactsTab {
    focus: Section,
    /// The browsed row and how far the table has scrolled; drawing scrolls it
    /// to keep the browsed row in view
    table: RefCell<TableState>,
    /// How many of the newest adverts are scrolled past
    adverts_scroll: usize,
}

/// The part of the Contacts tab that has keyboard focus
#[derive(Debug, Default, PartialEq)]
enum Section {
    #[default]
    Table,
    Adverts,
}

impl View for ContactsTab {
    fn title(&self) -> &str {
        "Contacts"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_contacts_tab(f, app, self, theme, Utc::now().timestamp(), area);
    }

    /// Tab moves focus between the table and the adverts; ↑↓ browse contacts or
    /// scroll adverts, and Enter messages the browsed contact
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match (key.code, &self.focus) {
            (KeyCode::Tab, Section::Table) => self.focus = Section::Adverts,
            (KeyCode::Tab, Section::Adverts) => self.focus = Section::Table,
            (KeyCode::Up, Section::Table) => self.browse(app, Step::Up),
            (KeyCode::Down, Section::Table) => self.browse(app, Step::Down),
            (KeyCode::Enter, Section::Table) => {
                return match self.browsed_contact(app) {
                    Some(contact) => KeyResult::Action(Action::MessageContact(contact.public_key)),
                    None => KeyResult::Ignored,
                };
            }
            (KeyCode::Up, Section::Adverts) => {
                self.adverts_scroll = self.adverts_scroll.saturating_sub(1)
            }
            (KeyCode::Down, Section::Adverts) => {
                self.adverts_scroll =
                    (self.adverts_scroll + 1).min(app.adverts.len().saturating_sub(1));
            }
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }

    fn key_hints(&self) -> Vec<&'static str> {
        match self.focus {
            Section::Table => vec!["↑↓=browse", "Enter=message", "Tab=panel"],
            Section::Adverts => vec!["↑↓=scroll", "Tab=panel"],
        }
    }
}

enum Step {
    Up,
    Down,
}

impl ContactsTab {
    /// Move the browsed row, starting from the first when none is browsed
    fn browse(&mut self, app: &App, step: Step) {
        let Some(last) = app.contacts.len().checked_sub(1) else {
            return;
        };
        let table = self.table.get_mut();
        let row = match (table.selected(), step) {
            (None, _) => 0,
            (Some(row), Step::Up) => row.saturating_sub(1),
            (Some(row), Step::Down) => (row + 1).min(last),
        };
        table.select(Some(row));
    }

    fn browsed_contact<'a>(&self, app: &'a App) -> Option<&'a Contact> {
        let row = self.table.borrow().selected()?;
        by_newest_advert(app).get(row).copied()
    }
}

/// The contacts, most recently heard first
fn by_newest_advert(app: &App) -> Vec<&Contact> {
    let mut contacts: Vec<&Contact> = app.contacts.iter().collect();
    contacts.sort_by_key(|c| std::cmp::Reverse(c.last_advert));
    contacts
}

/// Contact types from the firmware's ADV_TYPE_* (helpers/AdvertDataHelpers.h)
pub fn contact_type_label(contact_type: u8) -> &'static str {
    match contact_type {
        1 => "Companion",
        2 => "Repeater",
        3 => "Room",
        4 => "Sensor",
        _ => "?",
    }
}

/// How long ago a contact advertised, to the minute, from the advert's own
/// timestamp; it's the sender's clock, so a clock ahead of ours reads as
/// "<1m ago". Both times are cut to whole minutes, so every age in the table
/// ticks over together once a minute.
fn advert_age(last_advert: u32, now: i64) -> String {
    if last_advert == 0 {
        return "—".to_string();
    }
    let minutes = (now / 60 - last_advert as i64 / 60).clamp(0, (u32::MAX / 60) as i64) as u32;
    if minutes == 0 {
        return "<1m ago".to_string();
    }
    format!("{} ago", format_duration(minutes * 60))
}

/// "flood" when the radio has no route to this contact, else the hop count
fn hops_label(path_len: i8) -> String {
    match Route::from_path_len(path_len) {
        Route::Flood => "flood".to_string(),
        Route::Hops { count, .. } => count.to_string(),
    }
}

/// Position in decimal degrees; 0,0 means the contact didn't advertise one
fn position_label(contact: &Contact) -> String {
    if contact.adv_lat == 0 && contact.adv_lon == 0 {
        "—".to_string()
    } else {
        format!("{:.4}, {:.4}", contact.latitude(), contact.longitude())
    }
}

fn draw_contacts_tab(
    f: &mut Frame,
    app: &App,
    tab: &ContactsTab,
    theme: &Theme,
    now: i64,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(10)])
        .split(area);
    draw_contact_table(f, app, tab, theme, now, chunks[0]);
    draw_adverts(f, &app.adverts, tab, theme, chunks[1]);
}

fn draw_contact_table(
    f: &mut Frame,
    app: &App,
    tab: &ContactsTab,
    theme: &Theme,
    now: i64,
    area: Rect,
) {
    let rows: Vec<Row> = by_newest_advert(app)
        .into_iter()
        .map(|c| {
            Row::new([
                sanitize(&c.adv_name),
                contact_type_label(c.contact_type).to_string(),
                c.prefix_hex()[..8].to_string(),
                advert_age(c.last_advert, now),
                hops_label(c.path_len),
                position_label(c),
            ])
        })
        .collect();
    let header = Row::new(["Name", "Type", "Key", "Last advert", "Hops", "Position"]).style(
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    );
    let widths = [
        Constraint::Fill(1),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(11),
        Constraint::Length(5),
        Constraint::Length(19),
    ];
    let border = if tab.focus == Section::Table {
        theme.focus
    } else {
        theme.contacts_border
    };
    let table = Table::new(rows, widths)
        .header(header)
        .style(Style::default().fg(theme.text))
        .row_highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg))
        .block(panel(
            theme,
            format!(" Contacts ({}) ", app.contacts.len()),
            border,
        ));
    f.render_stateful_widget(table, area, &mut tab.table.borrow_mut());
}

/// Adverts, newest first from the scroll position; a new contact's shows what it added
fn draw_adverts(
    f: &mut Frame,
    adverts: &[AdvertEntry],
    tab: &ContactsTab,
    theme: &Theme,
    area: Rect,
) {
    let visible = (area.height as usize).saturating_sub(2);
    let items: Vec<ListItem> = adverts
        .iter()
        .rev()
        .skip(tab.adverts_scroll)
        .take(visible)
        .map(|a| {
            let (kind, detail) = match &a.new_contact {
                Some(c) => (
                    "new  ",
                    format!(
                        " [{}] {} hops {}",
                        c.prefix_hex(),
                        hops_label(c.path_len),
                        position_label(c)
                    ),
                ),
                None => ("again", String::new()),
            };
            ListItem::new(Line::from(vec![
                Span::styled(&a.timestamp, Style::default().fg(theme.muted)),
                Span::raw(" "),
                Span::styled(
                    kind,
                    Style::default()
                        .fg(theme.packet_advert)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(&a.name, Style::default().fg(theme.text)),
                Span::styled(detail, Style::default().fg(theme.muted)),
            ]))
        })
        .collect();
    let border = if tab.focus == Section::Adverts {
        theme.focus
    } else {
        theme.packets_border
    };
    f.render_widget(
        List::new(items).block(panel(theme, " Adverts ", border)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use meshcore_rs::events::{AdvertisementData, EventPayload};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// 2026-10-11 12:00:00 UTC
    const NOW: i64 = 1_791_720_000;

    fn render(app: &App, tab: &ContactsTab, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| draw_contacts_tab(f, app, tab, &Theme::CLASSIC, NOW, f.area()))
            .unwrap();
        terminal.backend().clone()
    }

    fn app_with_heard_contacts() -> App {
        let mut app = App::new(0);
        let mut alice = contact("Alice", [0xa1; 6]);
        alice.last_advert = (NOW - 300) as u32;
        alice.path_len = 2;
        alice.adv_lat = 52_008_900;
        alice.adv_lon = 4_353_100;
        let mut hilltop = contact("Hilltop", [0xb2; 6]);
        hilltop.contact_type = 2;
        hilltop.last_advert = (NOW - 3_780) as u32;
        hilltop.path_len = -1;
        app.contacts = vec![hilltop, contact("Never heard", [0xc3; 6]), alice];
        let mut alice_key = [0; 32];
        alice_key[..6].copy_from_slice(&[0xa1; 6]);
        app.apply_radio_event(EventPayload::Contact(contact("Bob", [0xd4; 6])));
        app.apply_radio_event(EventPayload::Advertisement(AdvertisementData {
            public_key: alice_key,
        }));
        for advert in &mut app.adverts {
            advert.timestamp = "12:34:56".to_string();
        }
        app
    }

    #[test]
    fn contacts_and_adverts_newest_first() {
        insta::assert_snapshot!(render(
            &app_with_heard_contacts(),
            &ContactsTab::default(),
            80,
            16
        ));
    }

    fn press(tab: &mut ContactsTab, app: &App, code: KeyCode) -> KeyResult {
        tab.handle_key(
            KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
            app,
        )
    }

    fn browsed_name(tab: &ContactsTab, app: &App) -> Option<String> {
        tab.browsed_contact(app).map(|c| c.adv_name.clone())
    }

    #[test]
    fn arrows_browse_the_table_in_its_order_within_bounds() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        assert_eq!(browsed_name(&tab, &app), None);
        assert_eq!(press(&mut tab, &app, KeyCode::Down), KeyResult::Handled);
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Alice"));
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Hilltop"));
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Down);
        }
        assert_eq!(tab.table.borrow().selected(), Some(app.contacts.len() - 1));
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Up);
        }
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Alice"));
    }

    #[test]
    fn arrows_with_no_contacts_browse_nothing() {
        let (mut tab, app) = (ContactsTab::default(), App::new(0));
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(tab.table.borrow().selected(), None);
    }

    #[test]
    fn enter_messages_the_browsed_contact() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Down);
        let hilltop = app
            .contacts
            .iter()
            .find(|c| c.adv_name == "Hilltop")
            .unwrap();
        assert_eq!(
            press(&mut tab, &app, KeyCode::Enter),
            KeyResult::Action(Action::MessageContact(hilltop.public_key))
        );
    }

    #[test]
    fn tab_moves_focus_to_the_adverts_which_scroll_within_bounds() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        assert_eq!(tab.key_hints(), ["↑↓=browse", "Enter=message", "Tab=panel"]);
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!(tab.key_hints(), ["↑↓=scroll", "Tab=panel"]);
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Down);
        }
        assert_eq!(tab.adverts_scroll, app.adverts.len() - 1);
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Up);
        }
        assert_eq!(tab.adverts_scroll, 0);
        // The table keeps its browsed row while the adverts have focus
        assert_eq!(tab.table.borrow().selected(), None);
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!(tab.focus, Section::Table);
    }

    #[test]
    fn advert_age_counts_back_from_now() {
        assert_eq!(advert_age(0, NOW), "—");
        assert_eq!(advert_age((NOW - 3_780) as u32, NOW), "1h3m ago");
        assert_eq!(advert_age((NOW - 3_789) as u32, NOW), "1h4m ago");
        assert_eq!(advert_age((NOW - 30) as u32, NOW), "1m ago");
        assert_eq!(advert_age(NOW as u32, NOW + 30), "<1m ago");
        // The sender's clock is ahead of ours
        assert_eq!(advert_age((NOW + 60) as u32, NOW), "<1m ago");
    }

    #[test]
    fn advert_age_ticks_over_on_the_minute() {
        let heard = (NOW - 3_789) as u32;
        assert_eq!(advert_age(heard, NOW + 59), advert_age(heard, NOW));
        assert_eq!(advert_age(heard, NOW + 60), "1h5m ago");
    }

    #[test]
    fn contact_types_follow_the_firmware() {
        let labels: Vec<&str> = (0..=5).map(contact_type_label).collect();
        assert_eq!(
            labels,
            ["?", "Companion", "Repeater", "Room", "Sensor", "?"]
        );
    }

    #[test]
    fn hops_and_position_have_placeholders() {
        assert_eq!(hops_label(-1), "flood");
        assert_eq!(hops_label(3), "3");
        // Three hops named by 2-byte hashes
        assert_eq!(hops_label(0x43), "3");
        assert_eq!(position_label(&contact("Nowhere", [1; 6])), "—");
    }
}
