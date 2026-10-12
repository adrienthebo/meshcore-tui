use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::{PayloadType, RouteType};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Row, Table, TableState},
    Frame,
};

use crate::app::App;
use crate::rx_log::RxPacket;
use crate::text::sanitize;
use crate::theme::Theme;
use crate::ui::panel;
use crate::view::{KeyResult, View};

/// Shown in place of a part of the packet the radio didn't send
const UNKNOWN: &str = "—";

/// Every packet the radio received over the air, newest first
#[derive(Debug, Default)]
pub struct RxLogTab {
    /// The browsed packet's id; it stays browsed as newer packets push it down
    browsed: Option<u64>,
    /// How far the table has scrolled; drawing scrolls it to keep the browsed packet in view
    table: RefCell<TableState>,
}

impl View for RxLogTab {
    fn title(&self) -> &str {
        "RX Log"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_packets(f, app, self, theme, area);
    }

    /// ↑↓ browse the packets
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

enum Step {
    Up,
    Down,
}

impl RxLogTab {
    /// Move the browsed packet, starting from the newest when none is browsed
    fn browse(&mut self, app: &App, step: Step) {
        let Some(last) = app.rx_log.packets.len().checked_sub(1) else {
            return;
        };
        let row = match (self.browsed_row(app), step) {
            (None, _) => 0,
            (Some(row), Step::Up) => row.saturating_sub(1),
            (Some(row), Step::Down) => (row + 1).min(last),
        };
        self.browsed = newest_first(app).nth(row).map(|p| p.id);
    }

    /// The browsed packet's row; None when none is browsed or it has dropped out of the log
    fn browsed_row(&self, app: &App) -> Option<usize> {
        let id = self.browsed?;
        newest_first(app).position(|p| p.id == id)
    }
}

fn newest_first(app: &App) -> impl Iterator<Item = &RxPacket> {
    app.rx_log.packets.iter().rev()
}

/// Payload types by their names in the firmware's PAYLOAD_TYPE_* (Packet.h);
/// "?" is a type meshcore-rs doesn't know
fn payload_type_label(payload_type: PayloadType) -> &'static str {
    match payload_type {
        PayloadType::Req => "REQ",
        PayloadType::Response => "RESPONSE",
        PayloadType::TextMsg => "TXT_MSG",
        PayloadType::Ack => "ACK",
        PayloadType::Advert => "ADVERT",
        PayloadType::GroupText => "GRP_TXT",
        PayloadType::GroupData => "GRP_DATA",
        PayloadType::AnonReq => "ANON_REQ",
        PayloadType::Path => "PATH",
        PayloadType::Trace => "TRACE",
        PayloadType::Multipart => "MULTIPART",
        PayloadType::Control => "CONTROL",
        PayloadType::RawCustom => "RAW_CUSTOM",
        PayloadType::Unknown => "?",
    }
}

/// Routes, with "t-" for the ones that carry a transport code
fn route_label(route_type: RouteType) -> &'static str {
    match route_type {
        RouteType::Flood => "flood",
        RouteType::Direct => "direct",
        RouteType::TransportFlood => "t-flood",
        RouteType::TransportDirect => "t-direct",
    }
}

/// What the packet is about, where the radio decoded it: an advert's name, a channel message's channel hash
fn summary(packet: &RxPacket) -> String {
    let log = &packet.log;
    match log.header.as_ref().map(|h| h.payload_type) {
        Some(PayloadType::Advert) => log
            .advertisement
            .as_ref()
            .and_then(|a| a.name.as_deref())
            .map(sanitize)
            .unwrap_or_default(),
        Some(PayloadType::GroupText) => log.payload.first().map(|hash| format!("ch {:02x}", hash)).unwrap_or_default(),
        _ => String::new(),
    }
}

fn packet_row(packet: &RxPacket) -> Row<'static> {
    let header = packet.log.header.as_ref();
    Row::new([
        packet.timestamp.clone(),
        format!("{:>6.2}", packet.log.snr),
        format!("{:>4}", packet.log.rssi),
        header.map_or(UNKNOWN, |h| payload_type_label(h.payload_type)).to_string(),
        header.map_or(UNKNOWN, |h| route_label(h.route_type)).to_string(),
        header.map_or(format!("{:>4}", UNKNOWN), |h| format!("{:>4}", h.path_len)),
        format!("{:>4}", packet.len()),
        summary(packet),
    ])
}

fn draw_packets(f: &mut Frame, app: &App, tab: &RxLogTab, theme: &Theme, area: Rect) {
    let rows: Vec<Row> = newest_first(app).map(packet_row).collect();
    let header = Row::new(["Time", "   SNR", "RSSI", "Type", "Route", "Hops", " Len", "Summary"])
        .style(Style::default().fg(theme.muted).add_modifier(Modifier::BOLD));
    let widths = [
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Fill(1),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .style(Style::default().fg(theme.text))
        .row_highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg))
        .block(panel(theme, format!(" Packets ({}) ", app.rx_log.packets.len()), theme.focus));
    let mut state = tab.table.borrow_mut();
    state.select(tab.browsed_row(app));
    f.render_stateful_widget(table, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use meshcore_rs::events::LogData;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(app: &App, tab: &RxLogTab, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_packets(f, app, tab, &Theme::CLASSIC, f.area())).unwrap();
        terminal.backend().clone()
    }

    /// A channel message, an advert, and a push that carried no packet, oldest first
    fn app_with_packets() -> App {
        let mut app = App::new(0);
        app.rx_log.record("12:00:01".to_string(), group_text_log(&[0xa1, 0x7f, 0x3c], 50));
        app.rx_log.record("12:00:02".to_string(), advert_log("Hilltop 🖖"));
        app.rx_log.record("12:00:03".to_string(), LogData { snr: 0.5, rssi: -120, ..Default::default() });
        app
    }

    fn press(tab: &mut RxLogTab, app: &App, code: KeyCode) -> KeyResult {
        tab.handle_key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE), app)
    }

    #[test]
    fn packets_newest_first() {
        insta::assert_snapshot!(render(&app_with_packets(), &RxLogTab::default(), 80, 7));
    }

    #[test]
    fn browsed_packet_is_highlighted() {
        let (mut tab, app) = (RxLogTab::default(), app_with_packets());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Down);
        render(&app, &tab, 80, 7);
        assert_eq!(tab.table.borrow().selected(), Some(1));
    }

    #[test]
    fn arrows_browse_from_the_newest_within_bounds() {
        let (mut tab, app) = (RxLogTab::default(), app_with_packets());
        assert_eq!(press(&mut tab, &app, KeyCode::Down), KeyResult::Handled);
        assert_eq!(tab.browsed, Some(2));
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Down);
        }
        assert_eq!(tab.browsed, Some(0));
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Up);
        }
        assert_eq!(tab.browsed, Some(2));
        assert_eq!(press(&mut tab, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    #[test]
    fn arrows_with_no_packets_browse_nothing() {
        let (mut tab, app) = (RxLogTab::default(), App::new(0));
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(tab.browsed, None);
    }

    #[test]
    fn browsed_packet_stays_browsed_as_newer_ones_arrive() {
        let (mut tab, mut app) = (RxLogTab::default(), app_with_packets());
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(tab.browsed_row(&app), Some(0));
        app.rx_log.record("12:00:04".to_string(), LogData::default());
        assert_eq!(tab.browsed_row(&app), Some(1));
        press(&mut tab, &app, KeyCode::Up);
        assert_eq!(tab.browsed, Some(3));
    }

    #[test]
    fn summary_names_adverts_and_channel_hashes() {
        let app = app_with_packets();
        let summaries: Vec<String> = newest_first(&app).map(summary).collect();
        assert_eq!(summaries, ["", "Hilltop 🖖", "ch d9"]);
    }
}
