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

use crate::app::{is_favorite, AdvertEntry, App, DiscoveredNode, Route};
use crate::contact_menu::ContactMenu;
use crate::text::{format_duration, sanitize};
use crate::theme::Theme;
use crate::ui::panel;
use crate::update::Action;
use crate::view::{KeyResult, View};

/// Every contact the radio knows, newest advert first, above the nodes heard
/// that aren't contacts and the adverts heard
#[derive(Debug, Default)]
pub struct ContactsTab {
    focus: Section,
    /// The browsed contact's public key; no row is browsed while it isn't in
    /// the contact list
    browsed: Option<[u8; 32]>,
    /// How far the table has scrolled; drawing selects the browsed contact's
    /// row and scrolls to keep it in view
    table: RefCell<TableState>,
    /// The browsed discovered node's public key; no row is browsed while it
    /// isn't in the discovered list
    discovered_browsed: Option<[u8; 32]>,
    /// How far the discovered table has scrolled; drawing selects the browsed
    /// node's row and scrolls to keep it in view
    discovered_table: RefCell<TableState>,
    /// The seq of the advert at the top of the scrolled adverts; None follows
    /// the newest
    adverts_top: Option<u64>,
    /// The browsed contact's menu, while it's open; it takes keys before the tab
    menu: Option<ContactMenu>,
}

/// The part of the Contacts tab that has keyboard focus
#[derive(Debug, Default, PartialEq)]
enum Section {
    #[default]
    Table,
    Discovered,
    Adverts,
}

impl View for ContactsTab {
    fn title(&self) -> &str {
        "Contacts"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_contacts_tab(f, app, self, theme, Utc::now().timestamp(), area);
    }

    /// Tab moves focus between the table, the discovered nodes and the adverts;
    /// ↑↓ browse contacts or discovered nodes or scroll adverts, Enter opens
    /// the browsed contact's menu and `a` adds the browsed discovered node.
    /// An open menu takes every key, and a choice or an Esc the menu doesn't
    /// use closes it; one whose contact has gone closes on the next key, which
    /// goes to the tab.
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        if let Some(menu) = &mut self.menu {
            if menu.contact(app).is_some() {
                let result = menu.handle_key(key, app);
                match result {
                    KeyResult::Action(_) => self.menu = None,
                    KeyResult::Ignored if key.code == KeyCode::Esc => {
                        self.menu = None;
                        return KeyResult::Handled;
                    }
                    KeyResult::Ignored | KeyResult::Handled => {}
                }
                return result;
            }
            self.menu = None;
        }
        match (key.code, &self.focus) {
            (KeyCode::Tab, Section::Table) => self.focus = Section::Discovered,
            (KeyCode::Tab, Section::Discovered) => self.focus = Section::Adverts,
            (KeyCode::Tab, Section::Adverts) => self.focus = Section::Table,
            (KeyCode::Up, Section::Table) => self.browse(app, Step::Up),
            (KeyCode::Down, Section::Table) => self.browse(app, Step::Down),
            (KeyCode::Enter, Section::Table) => match self.browsed_contact(app) {
                Some(contact) => self.menu = Some(ContactMenu::new(contact)),
                None => return KeyResult::Ignored,
            },
            (KeyCode::Up, Section::Discovered) => self.browse_discovered(app, Step::Up),
            (KeyCode::Down, Section::Discovered) => self.browse_discovered(app, Step::Down),
            (KeyCode::Char('a'), Section::Discovered) => {
                return match self.browsed_discovered(app) {
                    Some(node) => KeyResult::Action(Action::AddDiscovered(node.contact.public_key)),
                    None => KeyResult::Ignored,
                };
            }
            (KeyCode::Up, Section::Adverts) => self.scroll_adverts(app, Step::Up),
            (KeyCode::Down, Section::Adverts) => self.scroll_adverts(app, Step::Down),
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }

    fn key_hints(&self) -> Vec<&'static str> {
        if let Some(menu) = &self.menu {
            return menu.key_hints();
        }
        match self.focus {
            Section::Table => vec!["↑↓=browse", "Enter=actions", "Tab=panel"],
            Section::Discovered => vec!["↑↓=browse", "a=add", "Tab=panel"],
            Section::Adverts => vec!["↑↓=scroll", "Tab=panel"],
        }
    }
}

pub enum Step {
    Up,
    Down,
}

impl ContactsTab {
    /// Move the browsed row, starting from the first when none is browsed
    fn browse(&mut self, app: &App, step: Step) {
        let contacts = by_newest_advert(app);
        let Some(last) = contacts.len().checked_sub(1) else {
            return;
        };
        let row = match (self.browsed_row(&contacts), step) {
            (None, _) => 0,
            (Some(row), Step::Up) => row.saturating_sub(1),
            (Some(row), Step::Down) => (row + 1).min(last),
        };
        self.browsed = Some(contacts[row].public_key);
    }

    /// The browsed contact's row in the table's order
    fn browsed_row(&self, contacts: &[&Contact]) -> Option<usize> {
        let key = self.browsed?;
        contacts.iter().position(|c| c.public_key == key)
    }

    fn browsed_contact<'a>(&self, app: &'a App) -> Option<&'a Contact> {
        let key = self.browsed?;
        app.contacts.iter().find(|c| c.public_key == key)
    }

    /// Scroll the adverts one entry; scrolling back to the newest follows the
    /// adverts as they arrive
    fn scroll_adverts(&mut self, app: &App, step: Step) {
        let row = self.adverts_row(&app.adverts);
        let row = match step {
            Step::Up => row.saturating_sub(1),
            Step::Down => (row + 1).min(app.adverts.len().saturating_sub(1)),
        };
        self.adverts_top = match row {
            0 => None,
            row => app.adverts.iter().rev().nth(row).map(|a| a.seq),
        };
    }

    /// How many of the newest adverts are scrolled past; the oldest left when
    /// the top advert has been dropped
    fn adverts_row(&self, adverts: &[AdvertEntry]) -> usize {
        let Some(top) = self.adverts_top else {
            return 0;
        };
        adverts
            .iter()
            .rev()
            .position(|a| a.seq == top)
            .unwrap_or(adverts.len().saturating_sub(1))
    }

    /// Move the browsed discovered node, starting from the first when none is browsed
    fn browse_discovered(&mut self, app: &App, step: Step) {
        let Some(last) = app.discovered.len().checked_sub(1) else {
            return;
        };
        let row = match (self.discovered_row(&app.discovered), step) {
            (None, _) => 0,
            (Some(row), Step::Up) => row.saturating_sub(1),
            (Some(row), Step::Down) => (row + 1).min(last),
        };
        self.discovered_browsed = Some(app.discovered[row].contact.public_key);
    }

    /// The browsed discovered node's row, newest first
    fn discovered_row(&self, discovered: &[DiscoveredNode]) -> Option<usize> {
        let key = self.discovered_browsed?;
        discovered.iter().position(|d| d.contact.public_key == key)
    }

    fn browsed_discovered<'a>(&self, app: &'a App) -> Option<&'a DiscoveredNode> {
        let key = self.discovered_browsed?;
        app.discovered.iter().find(|d| d.contact.public_key == key)
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
/// timestamp; it's the sender's clock, so a clock ahead of ours gives a
/// negative age ("-3d"). Both times are cut to whole minutes, so every age in
/// the table ticks over together once a minute.
fn advert_age(last_advert: u32, now: i64) -> String {
    if last_advert == 0 {
        return "—".to_string();
    }
    let minutes = now / 60 - last_advert as i64 / 60;
    let size = format_duration(
        (minutes.unsigned_abs() * 60).try_into().unwrap_or(u32::MAX),
        2,
    );
    match minutes {
        0 => "<1m ago".to_string(),
        ..0 => format!("-{}", size),
        _ => format!("{} ago", size),
    }
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
        .constraints([
            Constraint::Min(6),
            Constraint::Length(10),
            Constraint::Length(10),
        ])
        .split(area);
    draw_contact_table(f, app, tab, theme, now, chunks[0]);
    draw_discovered(f, &app.discovered, tab, theme, now, chunks[1]);
    draw_adverts(f, &app.adverts, tab, theme, chunks[2]);
    if let Some(menu) = &tab.menu {
        menu.draw(f, area, app, theme, true);
    }
}

/// A contact's table row; `heard` is when it advertised, as unix time
fn contact_row(c: &Contact, heard: u32, now: i64) -> Row<'static> {
    Row::new([
        if is_favorite(c) { FAVORITE } else { "" }.to_string(),
        sanitize(&c.adv_name),
        contact_type_label(c.contact_type).to_string(),
        c.prefix_hex()[..8].to_string(),
        advert_age(heard, now),
        hops_label(c.path_len),
        position_label(c),
    ])
}

/// The column titles above contact rows, naming the favorites and age columns
fn contact_header(theme: &Theme, favorite: &'static str, age: &'static str) -> Row<'static> {
    Row::new([favorite, "Name", "Type", "Key", age, "Hops", "Position"]).style(
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    )
}

/// Marks a favorite, and heads the column of marks
const FAVORITE: &str = "★";

const CONTACT_WIDTHS: [Constraint; 7] = [
    Constraint::Length(1),
    Constraint::Fill(1),
    Constraint::Length(9),
    Constraint::Length(8),
    Constraint::Length(11),
    Constraint::Length(5),
    Constraint::Length(19),
];
/// Columns between contact table cells, shared so both tables line up
const CONTACT_SPACING: u16 = 2;

fn draw_contact_table(
    f: &mut Frame,
    app: &App,
    tab: &ContactsTab,
    theme: &Theme,
    now: i64,
    area: Rect,
) {
    let contacts = by_newest_advert(app);
    let browsed_row = tab.browsed_row(&contacts);
    let rows: Vec<Row> = contacts
        .into_iter()
        .map(|c| contact_row(c, c.last_advert, now))
        .collect();
    let border = if tab.focus == Section::Table {
        theme.focus
    } else {
        theme.contacts_border
    };
    let table = Table::new(rows, CONTACT_WIDTHS)
        .column_spacing(CONTACT_SPACING)
        .header(contact_header(theme, FAVORITE, "Last advert"))
        .style(Style::default().fg(theme.text))
        .row_highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg))
        .block(panel(
            theme,
            format!(" Contacts ({}) ", app.contacts.len()),
            border,
        ));
    let mut state = tab.table.borrow_mut();
    state.select(browsed_row);
    f.render_stateful_widget(table, area, &mut state);
}

/// Nodes heard that aren't contacts, newest first, dimmed; each one's age is
/// when we heard it, by our clock
fn draw_discovered(
    f: &mut Frame,
    discovered: &[DiscoveredNode],
    tab: &ContactsTab,
    theme: &Theme,
    now: i64,
    area: Rect,
) {
    let rows: Vec<Row> = discovered
        .iter()
        .map(|d| contact_row(&d.contact, u32::try_from(d.last_heard).unwrap_or(0), now))
        .collect();
    let border = if tab.focus == Section::Discovered {
        theme.focus
    } else {
        theme.contacts_border
    };
    let table = Table::new(rows, CONTACT_WIDTHS)
        .column_spacing(CONTACT_SPACING)
        // Discovered nodes aren't contacts, so none is a favorite
        .header(contact_header(theme, "", "Heard"))
        .style(Style::default().fg(theme.muted))
        .row_highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg))
        .block(panel(
            theme,
            format!(" Discovered ({}) ", discovered.len()),
            border,
        ));
    let mut state = tab.discovered_table.borrow_mut();
    state.select(tab.discovered_row(discovered));
    f.render_stateful_widget(table, area, &mut state);
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
        .skip(tab.adverts_row(adverts))
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
        hilltop.flags = 1;
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
        app.discovered[0].last_heard = NOW - 120;
        app
    }

    #[test]
    fn contacts_discovered_and_adverts_newest_first() {
        insta::assert_snapshot!(render(
            &app_with_heard_contacts(),
            &ContactsTab::default(),
            80,
            26
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
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Never heard"));
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Up);
        }
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Alice"));
    }

    #[test]
    fn arrows_with_no_contacts_browse_nothing() {
        let (mut tab, app) = (ContactsTab::default(), App::new(0));
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(tab.browsed, None);
    }

    #[test]
    fn the_browsed_row_follows_its_contact_when_the_list_changes() {
        let (mut tab, mut app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Down);
        render(&app, &tab, 80, 16);
        assert_eq!(tab.table.borrow().selected(), Some(0));
        // A contact heard more recently sorts above Alice
        let mut carol = contact("Carol", [0xe5; 6]);
        carol.last_advert = (NOW - 60) as u32;
        let mut reloaded = app.contacts.clone();
        reloaded.push(carol);
        reloaded.reverse();
        app.set_contacts(reloaded);
        render(&app, &tab, 80, 16);
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Alice"));
        assert_eq!(tab.table.borrow().selected(), Some(1));
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Hilltop"));
    }

    #[test]
    fn a_browsed_contact_that_leaves_the_list_browses_nothing() {
        let (mut tab, mut app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Down);
        app.contacts.retain(|c| c.adv_name != "Alice");
        render(&app, &tab, 80, 16);
        assert_eq!(browsed_name(&tab, &app), None);
        assert_eq!(tab.table.borrow().selected(), None);
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Hilltop"));
    }

    #[test]
    fn enter_opens_the_browsed_contacts_menu_which_messages_it() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Handled);
        assert_eq!(tab.key_hints(), ["↑↓=choose", "Enter=select", "Esc=close"]);
        let alice = app.contacts.iter().find(|c| c.adv_name == "Alice").unwrap();
        assert_eq!(
            press(&mut tab, &app, KeyCode::Enter),
            KeyResult::Action(Action::MessageContact(alice.public_key))
        );
        // Choosing closes the menu
        assert!(tab.menu.is_none());
    }

    #[test]
    fn contact_menu_shows_over_the_tab() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Enter);
        insta::assert_snapshot!(render(&app, &tab, 80, 26));
    }

    #[test]
    fn an_open_menu_takes_the_keys_and_esc_closes_it() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Enter);
        // Neither moves focus nor browses the table
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(tab.focus, Section::Table);
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Alice"));
        assert_eq!(press(&mut tab, &app, KeyCode::Esc), KeyResult::Handled);
        assert!(tab.menu.is_none());
        // With the menu closed, Esc is no longer the tab's
        assert_eq!(press(&mut tab, &app, KeyCode::Esc), KeyResult::Ignored);
    }

    #[test]
    fn esc_while_asking_to_delete_goes_back_to_the_menu_then_closes_it() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Enter);
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Enter);
        insta::assert_snapshot!(render(&app, &tab, 80, 26));
        assert_eq!(press(&mut tab, &app, KeyCode::Esc), KeyResult::Handled);
        assert_eq!(tab.key_hints(), ["↑↓=choose", "Enter=select", "Esc=close"]);
        assert_eq!(press(&mut tab, &app, KeyCode::Esc), KeyResult::Handled);
        assert!(tab.menu.is_none());
    }

    #[test]
    fn logging_in_from_the_menu_masks_the_password_then_closes_it() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Enter);
        press(&mut tab, &app, KeyCode::Enter);
        press(&mut tab, &app, KeyCode::Char('p'));
        press(&mut tab, &app, KeyCode::Char('w'));
        insta::assert_snapshot!(render(&app, &tab, 80, 26));
        assert!(matches!(
            press(&mut tab, &app, KeyCode::Enter),
            KeyResult::Action(Action::Login { password, .. }) if password == "pw"
        ));
        assert!(tab.menu.is_none());
    }

    #[test]
    fn a_menu_whose_contact_has_gone_closes_on_the_next_key() {
        let (mut tab, mut app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Enter);
        app.contacts.retain(|c| c.adv_name != "Alice");
        press(&mut tab, &app, KeyCode::Down);
        assert!(tab.menu.is_none());
        assert_eq!(browsed_name(&tab, &app).as_deref(), Some("Hilltop"));
    }

    #[test]
    fn tab_moves_focus_to_the_adverts_which_scroll_within_bounds() {
        let (mut tab, app) = (ContactsTab::default(), app_with_heard_contacts());
        assert_eq!(tab.key_hints(), ["↑↓=browse", "Enter=actions", "Tab=panel"]);
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!(tab.key_hints(), ["↑↓=scroll", "Tab=panel"]);
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Down);
        }
        assert_eq!(tab.adverts_row(&app.adverts), app.adverts.len() - 1);
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Up);
        }
        assert_eq!(tab.adverts_top, None);
        // The table keeps its browsed row while the adverts have focus
        assert_eq!(tab.browsed, None);
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!(tab.focus, Section::Table);
    }

    /// The names of the adverts the tab shows from its scroll position, newest first
    fn shown_adverts(tab: &ContactsTab, app: &App) -> Vec<String> {
        let row = tab.adverts_row(&app.adverts);
        app.adverts
            .iter()
            .rev()
            .skip(row)
            .map(|a| a.name.clone())
            .collect()
    }

    #[test]
    fn scrolled_adverts_stay_put_as_new_ones_arrive() {
        let (mut tab, mut app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Down);
        let shown = shown_adverts(&tab, &app);
        app.apply_radio_event(EventPayload::Contact(contact("Carol", [0xe5; 6])));
        assert_eq!(shown_adverts(&tab, &app), shown);
        // Back at the top, the newest advert shows as it arrives
        press(&mut tab, &app, KeyCode::Up);
        press(&mut tab, &app, KeyCode::Up);
        app.apply_radio_event(EventPayload::Contact(contact("Dave", [0xf6; 6])));
        assert_eq!(shown_adverts(&tab, &app)[0], "Dave");
    }

    #[test]
    fn scrolled_adverts_stop_at_the_oldest_left_when_theirs_is_dropped() {
        let (mut tab, mut app) = (ContactsTab::default(), app_with_heard_contacts());
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Down);
        app.adverts.remove(0);
        assert_eq!(tab.adverts_row(&app.adverts), app.adverts.len() - 1);
    }

    /// An app with the given discovered nodes, newest first
    fn app_with_discovered(names: &[&str]) -> App {
        let mut app = App::new(0);
        for (i, name) in names.iter().enumerate().rev() {
            app.apply_radio_event(EventPayload::Contact(contact(name, [0xe0 + i as u8; 6])));
        }
        app
    }

    fn browsed_discovered_name(tab: &ContactsTab, app: &App) -> Option<String> {
        tab.browsed_discovered(app)
            .map(|d| d.contact.adv_name.clone())
    }

    #[test]
    fn tab_moves_focus_to_the_discovered_nodes_which_browse_within_bounds() {
        let (mut tab, app) = (
            ContactsTab::default(),
            app_with_discovered(&["Bob", "Ridge"]),
        );
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!(tab.focus, Section::Discovered);
        assert_eq!(tab.key_hints(), ["↑↓=browse", "a=add", "Tab=panel"]);
        assert_eq!(browsed_discovered_name(&tab, &app), None);
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(browsed_discovered_name(&tab, &app).as_deref(), Some("Bob"));
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Down);
        }
        assert_eq!(
            browsed_discovered_name(&tab, &app).as_deref(),
            Some("Ridge")
        );
        press(&mut tab, &app, KeyCode::Up);
        assert_eq!(browsed_discovered_name(&tab, &app).as_deref(), Some("Bob"));
        // The contact table keeps its own browsed row
        assert_eq!(tab.table.borrow().selected(), None);
    }

    #[test]
    fn a_adds_the_browsed_discovered_node() {
        let (mut tab, app) = (
            ContactsTab::default(),
            app_with_discovered(&["Bob", "Ridge"]),
        );
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!(
            press(&mut tab, &app, KeyCode::Char('a')),
            KeyResult::Ignored
        );
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(
            press(&mut tab, &app, KeyCode::Char('a')),
            KeyResult::Action(Action::AddDiscovered(app.discovered[1].contact.public_key))
        );
        // Enter doesn't add
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn a_adds_nothing_outside_the_discovered_nodes() {
        let (mut tab, app) = (ContactsTab::default(), app_with_discovered(&["Bob"]));
        assert_eq!(
            press(&mut tab, &app, KeyCode::Char('a')),
            KeyResult::Ignored
        );
    }

    #[test]
    fn the_browsed_discovered_row_follows_its_node_as_others_are_heard() {
        let (mut tab, mut app) = (
            ContactsTab::default(),
            app_with_discovered(&["Bob", "Ridge"]),
        );
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Down);
        render(&app, &tab, 80, 26);
        assert_eq!(tab.discovered_table.borrow().selected(), Some(0));
        app.apply_radio_event(EventPayload::Contact(contact("Carol", [0xf6; 6])));
        render(&app, &tab, 80, 26);
        assert_eq!(browsed_discovered_name(&tab, &app).as_deref(), Some("Bob"));
        assert_eq!(tab.discovered_table.borrow().selected(), Some(1));
    }

    #[test]
    fn an_added_node_stays_discovered() {
        let (mut tab, mut app) = (ContactsTab::default(), app_with_discovered(&["Bob"]));
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Down);
        let bob = app.discovered[0].contact.clone();
        app.set_contacts(vec![bob]);
        assert_eq!(browsed_discovered_name(&tab, &app).as_deref(), Some("Bob"));
    }

    #[test]
    fn advert_age_counts_back_from_now() {
        assert_eq!(advert_age(0, NOW), "—");
        assert_eq!(advert_age((NOW - 3_780) as u32, NOW), "1h3m ago");
        assert_eq!(advert_age((NOW - 3_789) as u32, NOW), "1h4m ago");
        assert_eq!(advert_age((NOW - 30) as u32, NOW), "1m ago");
        assert_eq!(advert_age(NOW as u32, NOW + 30), "<1m ago");
        // The sender's clock is ahead of ours
        assert_eq!(advert_age((NOW + 30) as u32, NOW), "<1m ago");
        assert_eq!(advert_age((NOW + 60) as u32, NOW), "-1m");
        assert_eq!(advert_age((NOW + 3 * 86_400) as u32, NOW), "-3d");
    }

    #[test]
    fn long_advert_ages_fit_their_column() {
        // A sender whose clock was never set
        let age = advert_age(1, NOW);
        assert_eq!(age, "2962w3d ago");
        assert!(age.len() <= 11);
    }

    #[test]
    fn adverts_from_the_future_sort_first() {
        let mut app = app_with_heard_contacts();
        let mut ahead = contact("Ahead", [0xe5; 6]);
        ahead.last_advert = (NOW + 3 * 86_400) as u32;
        app.contacts.push(ahead);
        let names: Vec<&str> = by_newest_advert(&app)
            .iter()
            .map(|c| c.adv_name.as_str())
            .collect();
        assert_eq!(names, ["Ahead", "Alice", "Hilltop", "Never heard"]);
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
