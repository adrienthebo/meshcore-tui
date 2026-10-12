use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::PayloadType;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::app::App;
use crate::rx_log::{payload_type_label, route_label, Field, PacketLayout, RxPacket};
use crate::text::sanitize;
use crate::theme::Theme;
use crate::ui::{panel, rssi_quality, snr_quality};
use crate::view::{KeyResult, View};

/// Shown in place of a part of the packet the radio didn't send
const UNKNOWN: &str = "—";

/// Bytes per hex dump row
const HEX_ROW_BYTES: usize = 16;

/// Shortest Packets table worth showing: borders, its header and three packets
const PACKETS_MIN: u16 = 6;

/// Every packet the radio received over the air, newest first, above the
/// fields and a hex dump of the browsed packet, or of the newest when none is
#[derive(Debug, Default)]
pub struct RxLogTab {
    focus: Section,
    /// The browsed packet's id; it stays browsed as newer packets push it down
    browsed: Option<u64>,
    /// How far the table has scrolled; drawing scrolls it to keep the browsed packet in view
    table: RefCell<TableState>,
    /// The selected field of the browsed packet, highlighted in the hex dump
    field: usize,
}

/// The part of the RX Log tab that has keyboard focus
#[derive(Debug, Default, PartialEq)]
enum Section {
    #[default]
    Packets,
    Fields,
}

impl View for RxLogTab {
    fn title(&self) -> &str {
        "RX Log"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_rx_log_tab(f, app, self, theme, area);
    }

    /// ↑↓ browse packets, or pick a field of the browsed packet; ↑ past the
    /// newest packet or Home goes back to following the newest. Tab moves
    /// between packets and fields, browsing the packet on show.
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        match (key.code, &self.focus) {
            (KeyCode::Home, _) => self.follow_newest(),
            (KeyCode::Tab, Section::Packets) => {
                // Hold the packet on show still, so its fields don't change under the selection
                let Some(packet) = self.shown_packet(app) else {
                    return KeyResult::Ignored;
                };
                self.browsed = Some(packet.id);
                self.focus = Section::Fields;
            }
            (KeyCode::Tab, Section::Fields) => self.focus = Section::Packets,
            (KeyCode::Up, Section::Packets) => self.browse(app, Step::Up),
            (KeyCode::Down, Section::Packets) => self.browse(app, Step::Down),
            (KeyCode::Up, Section::Fields) => self.field = self.field.saturating_sub(1),
            (KeyCode::Down, Section::Fields) => {
                let fields = self.shown_layout(app).map_or(0, |layout| layout.fields.len());
                self.field = (self.field + 1).min(fields.saturating_sub(1));
            }
            _ => return KeyResult::Ignored,
        }
        KeyResult::Handled
    }

    fn key_hints(&self) -> Vec<&'static str> {
        match (&self.focus, self.browsed) {
            (Section::Packets, None) => vec!["↓=browse", "Tab=panel"],
            (Section::Packets, Some(_)) => vec!["↑↓=browse", "Home=newest", "Tab=panel"],
            (Section::Fields, _) => vec!["↑↓=field", "Home=newest", "Tab=panel"],
        }
    }
}

enum Step {
    Up,
    Down,
}

impl RxLogTab {
    /// Move the browsed packet: ↓ from following the newest browses it, and ↑
    /// from the newest goes back to following. A newly browsed packet starts at
    /// its first field.
    fn browse(&mut self, app: &App, step: Step) {
        let Some(last) = app.rx_log.packets.len().checked_sub(1) else {
            return;
        };
        let row = match (self.browsed_row(app), step) {
            (None, Step::Up) => return,
            (None, Step::Down) => 0,
            (Some(0), Step::Up) => return self.follow_newest(),
            (Some(row), Step::Up) => row - 1,
            (Some(row), Step::Down) => (row + 1).min(last),
        };
        self.browsed = newest_first(app).nth(row).map(|p| p.id);
        self.field = 0;
    }

    /// Stop browsing, so the fields show each packet as it arrives
    fn follow_newest(&mut self) {
        self.browsed = None;
        self.focus = Section::Packets;
        self.field = 0;
    }

    /// The browsed packet's row; None when none is browsed or it has dropped out of the log
    fn browsed_row(&self, app: &App) -> Option<usize> {
        let id = self.browsed?;
        newest_first(app).position(|p| p.id == id)
    }

    fn browsed_packet<'a>(&self, app: &'a App) -> Option<&'a RxPacket> {
        newest_first(app).nth(self.browsed_row(app)?)
    }

    /// The packet the fields show: the browsed one, else the newest
    fn shown_packet<'a>(&self, app: &'a App) -> Option<&'a RxPacket> {
        self.browsed_packet(app).or_else(|| app.rx_log.packets.back())
    }

    fn shown_layout(&self, app: &App) -> Option<PacketLayout> {
        PacketLayout::of(&self.shown_packet(app)?.log)
    }
}

fn newest_first(app: &App) -> impl Iterator<Item = &RxPacket> {
    app.rx_log.packets.iter().rev()
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

/// A packet's row, its SNR and RSSI coloured on the same scales as the Radio tab
fn packet_row(packet: &RxPacket, theme: &Theme) -> Row<'static> {
    let header = packet.log.header.as_ref();
    let (snr, rssi) = (packet.log.snr, packet.log.rssi);
    Row::new([
        Cell::from(packet.timestamp.clone()),
        Cell::from(Span::styled(format!("{:>6.2}", snr), Style::default().fg(theme.quality(snr_quality(snr))))),
        Cell::from(Span::styled(format!("{:>4}", rssi), Style::default().fg(theme.quality(rssi_quality(rssi))))),
        Cell::from(header.map_or(UNKNOWN, |h| payload_type_label(h.payload_type))),
        Cell::from(header.map_or(UNKNOWN, |h| route_label(h.route_type))),
        Cell::from(header.map_or(format!("{:>4}", UNKNOWN), |h| format!("{:>4}", h.path_len))),
        Cell::from(format!("{:>4}", packet.len())),
        Cell::from(summary(packet)),
    ])
}

/// The Packets table above the Fields of the browsed packet, or of the newest
/// when none is browsed, then its Hex dump when there's room for it
fn draw_rx_log_tab(f: &mut Frame, app: &App, tab: &RxLogTab, theme: &Theme, area: Rect) {
    let Some(packet) = tab.shown_packet(app) else {
        return draw_packets(f, app, tab, theme, area);
    };
    let layout = PacketLayout::of(&packet.log);
    // Following the newest packet selects no field, since the fields change with each packet
    let browsing = tab.browsed_packet(app).is_some();
    let selected = layout.as_ref().filter(|_| browsing).and_then(|l| l.fields.get(tab.field));
    // Borders and the header row around the fields, or the one line saying they can't be shown
    let fields_height = layout.as_ref().map_or(1, |l| l.fields.len() as u16 + 1) + 2;
    let hex_height = layout
        .as_ref()
        .map(|l| l.bytes.len().div_ceil(HEX_ROW_BYTES) as u16 + 2)
        .filter(|&height| area.height >= PACKETS_MIN + fields_height + height);
    let mut constraints = vec![Constraint::Min(PACKETS_MIN), Constraint::Length(fields_height)];
    constraints.extend(hex_height.map(Constraint::Length));
    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    draw_packets(f, app, tab, theme, chunks[0]);
    let title = if browsing { " Fields " } else { " Fields · newest " };
    draw_fields(f, layout.as_ref(), browsing.then_some(tab.field), title, tab, theme, chunks[1]);
    if let (Some(layout), Some(_)) = (&layout, hex_height) {
        draw_hex(f, layout, selected, theme, chunks[2]);
    }
}

fn draw_packets(f: &mut Frame, app: &App, tab: &RxLogTab, theme: &Theme, area: Rect) {
    let rows: Vec<Row> = newest_first(app).map(|packet| packet_row(packet, theme)).collect();
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
        .block(panel(
            theme,
            format!(" Packets ({}) ", app.rx_log.packets.len()),
            focus_border(tab, Section::Packets, theme.packets_border, theme),
        ));
    let mut state = tab.table.borrow_mut();
    state.select(tab.browsed_row(app));
    f.render_stateful_widget(table, area, &mut state);
}

fn focus_border(tab: &RxLogTab, section: Section, unfocused: Color, theme: &Theme) -> Color {
    if tab.focus == section { theme.focus } else { unfocused }
}

/// A field's first bytes in hex, with "…" when there are more
fn bytes_preview(bytes: &[u8]) -> String {
    const SHOWN: usize = 3;
    let mut preview: Vec<String> = bytes.iter().take(SHOWN).map(|b| format!("{:02X}", b)).collect();
    if bytes.len() > SHOWN {
        preview.push("…".to_string());
    }
    preview.join(" ")
}

fn draw_fields(
    f: &mut Frame,
    layout: Option<&PacketLayout>,
    selected: Option<usize>,
    title: &str,
    tab: &RxLogTab,
    theme: &Theme,
    area: Rect,
) {
    let block = panel(theme, title, focus_border(tab, Section::Fields, theme.radio_border, theme));
    let Some(layout) = layout else {
        let text = "Can't show its bytes: no packet, or a payload type meshcore-rs doesn't know";
        f.render_widget(Paragraph::new(text).style(Style::default().fg(theme.muted)).block(block), area);
        return;
    };
    let rows: Vec<Row> = layout
        .fields
        .iter()
        .map(|field| {
            Row::new([
                format!("{:>4}", field.offset),
                bytes_preview(&layout.bytes[field.offset..field.offset + field.len]),
                field.name.to_string(),
                field.value.clone(),
            ])
        })
        .collect();
    let header = Row::new([" Off", "Bytes", "Field", "Value"])
        .style(Style::default().fg(theme.muted).add_modifier(Modifier::BOLD));
    let widths = [Constraint::Length(4), Constraint::Length(12), Constraint::Length(14), Constraint::Fill(1)];
    let table = Table::new(rows, widths)
        .header(header)
        .style(Style::default().fg(theme.text))
        .row_highlight_style(Style::default().fg(theme.selected_fg).bg(theme.selected_bg))
        .block(block);
    f.render_stateful_widget(table, area, &mut TableState::default().with_selected(selected));
}

/// The packet in hex and ASCII, 16 bytes a row, with the selected field's bytes highlighted
fn draw_hex(f: &mut Frame, layout: &PacketLayout, field: Option<&Field>, theme: &Theme, area: Rect) {
    let highlight = Style::default().fg(theme.selected_fg).bg(theme.selected_bg);
    let selected = field.map_or(0..0, |field| field.offset..field.offset + field.len);
    let style = |at: usize| if selected.contains(&at) { highlight } else { Style::default().fg(theme.text) };
    let lines: Vec<Line> = layout
        .bytes
        .chunks(HEX_ROW_BYTES)
        .enumerate()
        .map(|(row, chunk)| {
            let start = row * HEX_ROW_BYTES;
            let mut spans = vec![Span::styled(format!("{:04x}  ", start), Style::default().fg(theme.muted))];
            for (i, byte) in chunk.iter().enumerate() {
                let at = start + i;
                if i > 0 {
                    // An extra space splits the row in halves; a gap inside the field is highlighted with it
                    let gap = if i == HEX_ROW_BYTES / 2 { "  " } else { " " };
                    let in_field = selected.contains(&(at - 1)) && selected.contains(&at);
                    spans.push(Span::styled(gap, if in_field { highlight } else { Style::default() }));
                }
                spans.push(Span::styled(format!("{:02X}", byte), style(at)));
            }
            // Line a short last row's ASCII up with the rows above
            let missing = HEX_ROW_BYTES - chunk.len();
            let split = if chunk.len() <= HEX_ROW_BYTES / 2 { 1 } else { 0 };
            spans.push(Span::raw(" ".repeat(missing * 3 + split + 2)));
            for (i, byte) in chunk.iter().enumerate() {
                let c = if byte.is_ascii_graphic() || *byte == b' ' { *byte as char } else { '.' };
                spans.push(Span::styled(c.to_string(), style(start + i)));
            }
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(panel(theme, " Hex ", theme.radio_border)), area);
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
        terminal.draw(|f| draw_rx_log_tab(f, app, tab, &Theme::CLASSIC, f.area())).unwrap();
        terminal.backend().clone()
    }

    fn screen_text(backend: &TestBackend) -> String {
        backend.buffer().content().iter().map(|cell| cell.symbol()).collect()
    }

    /// Browsing the oldest packet, the channel message, with its path field selected
    fn tab_on_channel_message_path(app: &App) -> RxLogTab {
        let mut tab = RxLogTab::default();
        for _ in 0..3 {
            press(&mut tab, app, KeyCode::Down);
        }
        press(&mut tab, app, KeyCode::Tab);
        press(&mut tab, app, KeyCode::Down);
        press(&mut tab, app, KeyCode::Down);
        tab
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
        let mut terminal = Terminal::new(TestBackend::new(80, 7)).unwrap();
        let app = app_with_packets();
        terminal.draw(|f| draw_packets(f, &app, &RxLogTab::default(), &Theme::CLASSIC, f.area())).unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn snr_and_rssi_are_coloured_by_quality() {
        let mut terminal = Terminal::new(TestBackend::new(80, 7)).unwrap();
        let app = app_with_packets();
        terminal.draw(|f| draw_packets(f, &app, &RxLogTab::default(), &Theme::CLASSIC, f.area())).unwrap();
        let buffer = terminal.backend().buffer();
        let theme = Theme::CLASSIC;
        // Rows 2-4 are the packets newest first; SNR ends at column 15 and RSSI at 20
        for (y, snr, rssi) in [(3, -2.75, -112), (4, 9.25, -28)] {
            assert_eq!(buffer[(15, y)].fg, theme.quality(snr_quality(snr)), "SNR on row {}", y);
            assert_eq!(buffer[(20, y)].fg, theme.quality(rssi_quality(rssi)), "RSSI on row {}", y);
        }
        assert_ne!(buffer[(15, 3)].fg, buffer[(15, 4)].fg);
    }

    #[test]
    fn fields_follow_the_newest_packet_until_one_is_browsed() {
        let (tab, mut app) = (RxLogTab::default(), app_with_packets());
        app.rx_log.record("12:00:04".to_string(), group_text_log(&[0xa1], 20));
        let backend = render(&app, &tab, 80, 24);
        let text = screen_text(&backend);
        assert!(text.contains(" Fields · newest "));
        assert!(text.contains("channel hash"));
        // Nothing is selected while following: no packet row, field or bytes
        assert!(backend.buffer().content().iter().all(|cell| cell.bg != Theme::CLASSIC.selected_bg));
        app.rx_log.record("12:00:05".to_string(), advert_log("Hilltop"));
        assert!(screen_text(&render(&app, &tab, 80, 24)).contains("public key"));
    }

    #[test]
    fn home_and_up_past_the_newest_go_back_to_following_it() {
        let (mut tab, app) = (RxLogTab::default(), app_with_packets());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Down);
        assert_eq!(tab.key_hints(), ["↑↓=browse", "Home=newest", "Tab=panel"]);
        assert_eq!(press(&mut tab, &app, KeyCode::Home), KeyResult::Handled);
        assert_eq!(tab.browsed, None);
        assert_eq!(tab.key_hints(), ["↓=browse", "Tab=panel"]);
        // From the fields too, which hand focus back to the packets
        let mut tab = tab_on_channel_message_path(&app);
        press(&mut tab, &app, KeyCode::Home);
        assert_eq!((tab.browsed, &tab.focus, tab.field), (None, &Section::Packets, 0));
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Up);
        assert_eq!(tab.browsed, None);
        assert!(screen_text(&render(&app, &tab, 80, 24)).contains(" Fields · newest "));
    }

    #[test]
    fn browsed_packet_is_highlighted() {
        let (mut tab, app) = (RxLogTab::default(), app_with_packets());
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Down);
        render(&app, &tab, 80, 24);
        assert_eq!(tab.table.borrow().selected(), Some(1));
    }

    #[test]
    fn browsed_packet_shows_its_fields_and_hex() {
        let app = app_with_packets();
        insta::assert_snapshot!(render(&app, &tab_on_channel_message_path(&app), 80, 24));
    }

    #[test]
    fn hex_highlights_the_selected_fields_bytes() {
        let app = app_with_packets();
        let backend = render(&app, &tab_on_channel_message_path(&app), 80, 24);
        let buffer = backend.buffer();
        let row = (0..24).find(|&y| buffer[(1, y)].symbol() == "0" && buffer[(2, y)].symbol() == "0").unwrap();
        // Bytes 2-4 of the first row are the path: border, "0000  ", then 3 columns a byte
        let byte_x = |byte: u16| 1 + 6 + byte * 3;
        let bg = |x: u16| buffer[(x, row)].bg;
        assert_eq!(buffer[(byte_x(2), row)].symbol(), "A");
        assert_eq!(bg(byte_x(2)), Theme::CLASSIC.selected_bg);
        assert_eq!(bg(byte_x(3) - 1), Theme::CLASSIC.selected_bg, "gap inside the field");
        assert_eq!(bg(byte_x(4) + 1), Theme::CLASSIC.selected_bg);
        assert_ne!(bg(byte_x(2) - 1), Theme::CLASSIC.selected_bg, "gap before the field");
        assert_ne!(bg(byte_x(5)), Theme::CLASSIC.selected_bg);
    }

    #[test]
    fn hex_is_left_out_when_the_tab_is_short() {
        let app = app_with_packets();
        let tab = tab_on_channel_message_path(&app);
        assert!(screen_text(&render(&app, &tab, 80, 21)).contains(" Hex "));
        let short = screen_text(&render(&app, &tab, 80, 20));
        assert!(!short.contains(" Hex "));
        assert!(short.contains(" Fields "));
    }

    #[test]
    fn a_push_without_a_packet_says_why_it_has_no_fields() {
        let (mut tab, app) = (RxLogTab::default(), app_with_packets());
        press(&mut tab, &app, KeyCode::Down);
        let text = screen_text(&render(&app, &tab, 80, 24));
        assert!(text.contains("Can't show its bytes"));
        assert!(!text.contains(" Hex "));
    }

    #[test]
    fn tab_moves_to_the_fields_browsing_the_packet_on_show() {
        let (mut tab, app) = (RxLogTab::default(), App::new(0));
        assert_eq!(press(&mut tab, &app, KeyCode::Tab), KeyResult::Ignored);
        let app = app_with_packets();
        assert_eq!(press(&mut tab, &app, KeyCode::Tab), KeyResult::Handled);
        // The newest, which the fields were following
        assert_eq!(tab.browsed, Some(2));
        assert_eq!(tab.key_hints(), ["↑↓=field", "Home=newest", "Tab=panel"]);
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!(tab.focus, Section::Packets);
        // A browsed packet keeps its selected field through Tab and back
        let mut tab = tab_on_channel_message_path(&app);
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Tab);
        assert_eq!((tab.browsed, tab.field), (Some(0), 2));
    }

    #[test]
    fn arrows_pick_fields_within_bounds_and_a_new_packet_starts_at_its_first() {
        let app = app_with_packets();
        let mut tab = tab_on_channel_message_path(&app);
        assert_eq!(tab.field, 2);
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Down);
        }
        // Header, path len, path, channel hash, mac, ciphertext
        assert_eq!(tab.field, 5);
        for _ in 0..5 {
            press(&mut tab, &app, KeyCode::Up);
        }
        assert_eq!(tab.field, 0);
        press(&mut tab, &app, KeyCode::Down);
        press(&mut tab, &app, KeyCode::Tab);
        press(&mut tab, &app, KeyCode::Up);
        assert_eq!(tab.field, 0);
    }

    #[test]
    fn bytes_preview_shows_the_first_three() {
        assert_eq!(bytes_preview(&[0x15]), "15");
        assert_eq!(bytes_preview(&[0xa1, 0x7f, 0x3c]), "A1 7F 3C");
        assert_eq!(bytes_preview(&[1, 2, 3, 4]), "01 02 03 …");
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
        press(&mut tab, &app, KeyCode::Up);
        press(&mut tab, &app, KeyCode::Up);
        assert_eq!(tab.browsed, Some(2));
        // Past the newest is following it, and ↑ there does nothing more
        press(&mut tab, &app, KeyCode::Up);
        assert_eq!(tab.browsed, None);
        assert_eq!(press(&mut tab, &app, KeyCode::Up), KeyResult::Handled);
        assert_eq!(tab.browsed, None);
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
