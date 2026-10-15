use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::events::{Contact, StatusData};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use crate::app::{App, RepeaterStatus, Role, StatusRequest};
use crate::commands::REPEATER;
use crate::contacts_tab::Step;
use crate::radio_tab::{duty_bar, fraction, reading, value, with_separators, UNKNOWN};
use crate::text::{format_duration, sanitize};
use crate::theme::Theme;
use crate::ui::panel;
use crate::update::Action;
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

    /// ↑↓ browse the repeaters; r asks the shown one for its status
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match key.code {
            KeyCode::Up => self.browse(app, Step::Up),
            KeyCode::Down => self.browse(app, Step::Down),
            KeyCode::Char('r') => {
                let repeaters = logged_in(app);
                return match self.shown_row(&repeaters) {
                    Some(row) => {
                        KeyResult::Action(Action::RequestStatus(repeaters[row].0.public_key))
                    }
                    None => KeyResult::Handled,
                };
            }
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }

    fn key_hints(&self) -> Vec<&'static str> {
        vec!["↑↓=browse", "r=status"]
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
    let shown = row.map(|row| repeaters[row]);
    let reported = shown.and_then(|(repeater, _)| app.statuses.get(&repeater.public_key));
    draw_status(f, shown, reported, theme, status);
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

/// The shown repeater's name, key and our role, with where its status
/// request stands, above its latest status; without one, how to log in
fn draw_status(
    f: &mut Frame,
    shown: Option<(&Contact, Role)>,
    reported: Option<&RepeaterStatus>,
    theme: &Theme,
    area: Rect,
) {
    let muted = Style::default().fg(theme.muted);
    let Some((repeater, role)) = shown else {
        let text = Paragraph::new(NOT_LOGGED_IN)
            .style(muted)
            .wrap(Wrap { trim: true })
            .block(panel(theme, " Status ", theme.radio_border));
        return f.render_widget(text, area);
    };
    let title = format!(
        " {} [{}] · {} ",
        sanitize(&repeater.adv_name),
        &repeater.prefix_hex()[..8],
        role.label()
    );
    let latest = reported.and_then(|r| r.latest.as_ref());
    let notice = match (reported.and_then(|r| r.request), latest) {
        (Some(StatusRequest::Waiting), _) => "Requesting status…".to_string(),
        (Some(StatusRequest::Unanswered), Some((_, at))) => {
            format!("No reply to the last request. Showing {at}.")
        }
        (Some(StatusRequest::Unanswered), None) => "No reply to the last request.".to_string(),
        (None, Some((_, at))) => format!("Status as of {at}."),
        (None, None) => "No status yet. Press r to request it.".to_string(),
    };
    let [header, metrics] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Fill(1)])
        .areas(area);
    let header_text =
        Paragraph::new(notice)
            .style(muted)
            .block(panel(theme, title, theme.radio_border));
    f.render_widget(header_text, header);
    if let Some((status, _)) = latest {
        draw_metrics(f, status, theme, metrics);
    }
}

/// Width of the status's labels, so their values line up
const STATUS_LABEL_WIDTH: usize = 10;

/// A repeater's status in panels: Device and Signal, Packets, then Airtime
/// and Errors
fn draw_metrics(f: &mut Frame, status: &StatusData, theme: &Theme, area: Rect) {
    let [health, packets, usage, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Fill(1),
        ])
        .areas(area);
    let [device, signal] = two_columns(health);
    let device_readings = [
        ("Uptime", format_duration(status.uptime, 3)),
        (
            "Battery",
            format!("{:.2} V", status.battery_mv as f64 / 1000.0),
        ),
        ("TX queue", status.tx_queue_len.to_string()),
    ];
    draw_readings(f, theme, " Device ", &device_readings, device);
    let signal_readings = [
        ("Noise", format!("{} dBm", status.noise_floor)),
        ("RSSI", format!("{} dBm", status.last_rssi)),
        ("SNR", format!("{:.1} dB", status.snr)),
    ];
    draw_readings(f, theme, " Signal ", &signal_readings, signal);
    draw_packets(f, status, theme, packets);
    let [airtime, errors] = two_columns(usage);
    let totals = [status.airtime, status.rx_airtime].map(|secs| format_duration(secs, 3));
    // Wide enough for the longer total, so the shares line up
    let width = totals.iter().map(String::len).max().unwrap_or(0) + 1;
    let on_air = |total: &str, air_secs: u32| match fraction(air_secs, status.uptime) {
        Some(share) => format!(
            "{total:<width$}{:>5.1}% {}",
            share * 100.0,
            duty_bar(share, 4)
        ),
        None => total.to_string(),
    };
    let airtime_readings = [
        ("TX", on_air(&totals[0], status.airtime)),
        ("RX", on_air(&totals[1], status.rx_airtime)),
    ];
    draw_readings(f, theme, " Airtime ", &airtime_readings, airtime);
    // Each packet the radio tried to decode was either received or an error
    let recv_errors = status.recv_errors.map_or(UNKNOWN.to_string(), |errors| {
        with_share(errors, status.nb_recv.saturating_add(errors))
    });
    let error_readings = [
        ("RX errors", recv_errors),
        ("Flags", format!("0x{:04x}", status.err_events)),
    ];
    draw_readings(f, theme, " Errors ", &error_readings, errors);
}

/// A count and its share of `whole`, e.g. "412 (5.0%)"; just the count when
/// `whole` is zero
fn with_share(count: u32, whole: u32) -> String {
    match fraction(count, whole) {
        Some(share) => format!("{} ({:.1}%)", with_separators(count), share * 100.0),
        None => with_separators(count),
    }
}

/// Two panels side by side, equal width
fn two_columns(area: Rect) -> [Rect; 2] {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1); 2])
        .areas(area)
}

/// A panel of readings, one per line; labels are padded to the status's
/// label width, or wider to keep two spaces after the longest
fn draw_readings(
    f: &mut Frame,
    theme: &Theme,
    title: &str,
    readings: &[(&str, String)],
    area: Rect,
) {
    let longest = readings.iter().map(|(label, _)| label.len()).max();
    let width = STATUS_LABEL_WIDTH.max(longest.unwrap_or(0) + 2);
    let lines: Vec<Line> = readings
        .iter()
        .map(|(label, text)| {
            let text = value(theme, text.as_str());
            Line::from_iter(reading(theme, label, width, text))
        })
        .collect();
    let readings = Paragraph::new(lines).block(panel(theme, title, theme.radio_border));
    f.render_widget(readings, area);
}

/// Packets sent, received and duplicated, by flood and direct; duplicates
/// are counted among the packets received
fn draw_packets(f: &mut Frame, status: &StatusData, theme: &Theme, area: Rect) {
    let muted = Style::default().fg(theme.muted);
    let row = |label: &'static str, sent: u32, received: u32, duplicates: u16| {
        let counts = format!(
            "{:<10}{:<11}{}",
            with_separators(sent),
            with_separators(received),
            with_share(duplicates.into(), received)
        );
        Line::from_iter(reading(
            theme,
            label,
            STATUS_LABEL_WIDTH,
            value(theme, counts),
        ))
    };
    let rows = vec![
        Line::styled(
            format!(
                "{:<STATUS_LABEL_WIDTH$}{:<10}{:<11}{}",
                "", "Sent", "Received", "Duplicates"
            ),
            muted,
        ),
        row(
            "flood",
            status.flood_sent,
            status.flood_recv,
            status.flood_dups,
        ),
        row(
            "direct",
            status.direct_sent,
            status.direct_recv,
            status.direct_dups,
        ),
    ];
    let packets = Paragraph::new(rows).block(panel(theme, " Packets ", theme.radio_border));
    f.render_widget(packets, area);
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
        render_sized(app, tab, 80, 8)
    }

    fn render_sized(app: &App, tab: &RepeatersTab, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
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

    /// Logged in as app_with_logins, with Beacon's status request in this state
    fn app_with_status(latest: bool, request: Option<StatusRequest>) -> App {
        let mut app = app_with_logins();
        let mut key = [0; 32];
        key[..6].copy_from_slice(&[0xe5; 6]);
        let latest = latest.then(|| (repeater_status([0xe5; 6]), "14:02:37".to_string()));
        app.statuses.insert(key, RepeaterStatus { latest, request });
        app
    }

    fn render_status(app: &App) -> TestBackend {
        render_sized(app, &RepeatersTab::default(), 90, 17)
    }

    #[test]
    fn shows_the_shown_repeaters_status() {
        insta::assert_snapshot!(render_status(&app_with_status(true, None)));
    }

    #[test]
    fn says_while_a_status_request_waits() {
        let app = app_with_status(false, Some(StatusRequest::Waiting));
        insta::assert_snapshot!(render_status(&app));
    }

    #[test]
    fn says_when_a_status_request_goes_unanswered_and_keeps_the_last() {
        let app = app_with_status(true, Some(StatusRequest::Unanswered));
        insta::assert_snapshot!(render_status(&app));
    }

    #[test]
    fn airtime_shares_line_up_past_a_long_total() {
        let mut app = app_with_status(true, None);
        let status = app.statuses.values_mut().next().unwrap();
        let (latest, _) = status.latest.as_mut().unwrap();
        latest.airtime = 58_837;
        latest.rx_airtime = 80_282;
        insta::assert_snapshot!(render_status(&app));
    }

    #[test]
    fn a_share_needs_a_whole() {
        assert_eq!(with_share(412, 8311), "412 (5.0%)");
        assert_eq!(with_share(1204, 1204), "1,204 (100.0%)");
        assert_eq!(with_share(0, 0), "0");
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
    fn r_requests_the_shown_repeaters_status() {
        let app = app_with_logins();
        let mut tab = RepeatersTab::default();
        tab.show(app.contacts[0].public_key);
        assert_eq!(
            press(&mut tab, &app, KeyCode::Char('r')),
            KeyResult::Action(Action::RequestStatus(app.contacts[0].public_key))
        );
        assert_eq!(
            press(
                &mut RepeatersTab::default(),
                &App::new(0),
                KeyCode::Char('r')
            ),
            KeyResult::Handled
        );
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
