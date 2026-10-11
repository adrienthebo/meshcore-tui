use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, RenderDirection, Sparkline, SparklineBar},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, MsgType, Panel};
use crate::text::{sanitize, truncate_to_width, wrap_to_width};
use crate::theme::Theme;

// ── TUI Rendering ────────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // status
            Constraint::Min(10),   // main
            Constraint::Length(7), // packets
            Constraint::Length(3), // input
        ])
        .split(f.area());

    let diagnostics = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(50)])
        .split(chunks[2]);

    draw_status(f, app, chunks[0]);
    draw_main(f, app, chunks[1]);
    draw_packets(f, app, diagnostics[0]);
    draw_radio(f, app, diagnostics[1]);
    draw_input(f, app, chunks[3]);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let freq_info = app
        .self_info
        .as_ref()
        .map(|i| {
            format!(
                "{:.3} MHz | BW: {:.1} kHz | SF: {} | CR: {}",
                i.radio_freq as f64 / 1e3,
                i.radio_bw as f64 / 1e3,
                i.sf,
                i.cr
            )
        })
        .unwrap_or_default();

    let theme = &app.theme;
    let spans = vec![
        Span::styled("Port: ", Style::default().fg(theme.text)),
        Span::styled(&app.port_info, Style::default().fg(theme.info)),
        Span::styled(" | ", Style::default().fg(theme.muted)),
        Span::styled(
            &freq_info,
            Style::default().fg(theme.value),
        ),
        Span::styled(" | ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("RX: {} | TX: {}", app.rx_count, app.tx_count),
            Style::default().fg(theme.good),
        ),
        Span::styled(" | ", Style::default().fg(theme.muted)),
        Span::styled(
            if app.connected { "● Connected" } else { "○ Disconnected" },
            Style::default().fg(if app.connected {
                theme.good
            } else {
                theme.bad
            }),
        ),
    ];

    let w = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Status ")
            .border_style(Style::default().fg(theme.status_border)),
    );
    f.render_widget(w, area);
}

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(30)])
        .split(area);

    draw_messages(f, app, chunks[0]);
    draw_contacts(f, app, chunks[1]);
}

fn draw_messages(f: &mut Frame, app: &App, area: Rect) {
    let panel_width = (area.width as usize).saturating_sub(2); // border
    let theme = &app.theme;
    let items: Vec<ListItem> = app
        .messages
        .iter()
        .map(|m| {
            let (tag, tag_c, name_c) = match &m.msg_type {
                MsgType::Incoming => ("[MSG]".to_string(), theme.msg_incoming, theme.msg_incoming),
                MsgType::Outgoing => ("[SND]".to_string(), theme.msg_outgoing, theme.msg_outgoing),
                MsgType::Channel(ch) => {
                    let label = app.channel_label(*ch);
                    (format!("[{}]", label), theme.msg_channel, theme.msg_channel)
                }
                MsgType::System => ("[SYS]".to_string(), theme.muted, theme.muted),
                MsgType::Error => ("[ERR]".to_string(), theme.bad, theme.bad),
            };

            // Build prefix: "HH:MM:SS [TAG] sender: "
            let prefix = format!("{} {} {}: ", m.timestamp, tag, m.sender);
            let prefix_len = prefix.width();
            let text_display_width = m.text.width();

            let chunks = if prefix_len + text_display_width <= panel_width || panel_width <= prefix_len {
                // Fits on one line or panel too narrow to wrap
                vec![m.text.clone()]
            } else {
                wrap_to_width(&m.text, panel_width - prefix_len)
            };

            let continuation_indent = " ".repeat(prefix_len);
            let lines: Vec<Line> = chunks
                .into_iter()
                .enumerate()
                .map(|(i, chunk)| {
                    let body = Span::styled(chunk, Style::default().fg(theme.text));
                    if i == 0 {
                        Line::from(vec![
                            Span::styled(&m.timestamp, Style::default().fg(theme.muted)),
                            Span::raw(" "),
                            Span::styled(tag.clone(), Style::default().fg(tag_c).add_modifier(Modifier::BOLD)),
                            Span::raw(" "),
                            Span::styled(
                                &m.sender,
                                Style::default().fg(name_c).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(": ", Style::default().fg(name_c)),
                            body,
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled(
                                continuation_indent.clone(),
                                Style::default().fg(theme.muted),
                            ),
                            body,
                        ])
                    }
                })
                .collect();
            ListItem::new(lines)
        })
        .collect::<Vec<_>>();

    // Take items from the end that fit in visible height
    let visible_height = (area.height as usize).saturating_sub(2);
    let mut total_lines = 0usize;
    let mut start_idx = items.len();
    for (i, item) in items.iter().enumerate().rev() {
        let h = item.height();
        if total_lines + h > visible_height {
            break;
        }
        total_lines += h;
        start_idx = i;
    }
    let visible_items: Vec<ListItem> = items.into_iter().skip(start_idx).collect();

    let bc = if app.panel == Panel::Messages {
        theme.focus
    } else {
        theme.messages_border
    };
    let msg_w = List::new(visible_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} — {} ", app.device_name(), app.target_label()))
            .border_style(Style::default().fg(bc)),
    );
    f.render_widget(msg_w, area);
}

fn draw_contacts(f: &mut Frame, app: &App, area: Rect) {
    let cpanel_width = (area.width as usize).saturating_sub(2); // minus borders
    let selected = app.selected_index();
    let theme = &app.theme;
    let citems: Vec<ListItem> = app
        .contacts
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let sel = selected == Some(i);
            let sty = if sel {
                Style::default()
                    .fg(theme.selected_fg)
                    .bg(theme.selected_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            let name = sanitize(&c.adv_name);
            let hex = c.prefix_hex();
            let hex_short = &hex[..8.min(hex.len())];

            // Build the full line and truncate to exactly cpanel_width
            let marker = if sel { "\u{25b6} " } else { "  " };
            let full = format!("{}{} ({})", marker, name, hex_short);
            let line = truncate_to_width(&full, cpanel_width);

            // Pad with spaces to fill the panel (needed for selection highlight)
            let line_w = UnicodeWidthStr::width(line.as_str());
            let padded = if line_w < cpanel_width {
                format!("{}{}", line, " ".repeat(cpanel_width - line_w))
            } else {
                line
            };

            ListItem::new(Span::styled(padded, sty))
        })
        .collect();

    let cc = if app.panel == Panel::Contacts {
        theme.focus
    } else {
        theme.contacts_border
    };
    let c_w = List::new(citems).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Contacts ({}) ", app.contacts.len()))
            .border_style(Style::default().fg(cc)),
    );
    f.render_widget(c_w, area);
}

fn draw_packets(f: &mut Frame, app: &App, area: Rect) {
    let visible = (area.height as usize).saturating_sub(2);
    let theme = &app.theme;
    let items: Vec<ListItem> = app
        .packets
        .iter()
        .rev()
        .take(visible)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|p| {
            let tc = match p.ptype.as_str() {
                "ADV" => theme.packet_advert,
                "ACK" => theme.packet_ack,
                "SND" => theme.packet_sent,
                "PATH" => theme.packet_path,
                _ => theme.text,
            };
            ListItem::new(Line::from(vec![
                Span::styled(&p.timestamp, Style::default().fg(theme.muted)),
                Span::raw(" "),
                Span::styled(
                    format!("[{}]", p.ptype),
                    Style::default().fg(tc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" H:{}", p.hops),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(
                    if p.rssi != 0 || p.snr != 0.0 {
                        format!(" R:{}dBm S:{:.1}dB", p.rssi, p.snr)
                    } else {
                        String::new()
                    },
                    Style::default().fg(theme.muted),
                ),
                Span::styled(
                    format!(" | {}", p.info),
                    Style::default().fg(theme.text),
                ),
            ]))
        })
        .collect();

    let pc = if app.panel == Panel::Packets {
        theme.focus
    } else {
        theme.packets_border
    };
    let w = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Received Packets ")
            .border_style(Style::default().fg(pc)),
    );
    f.render_widget(w, area);
}

/// Smallest dB range the noise graph spans, so 1 dB of jitter doesn't fill its height
const NOISE_GRAPH_MIN_SPAN_DB: i16 = 10;
/// Bar height of a reading at the bottom of the graph, so it draws the smallest bar
/// in a 3-row graph and stays distinct from a gap
const NOISE_BAR_FLOOR: u64 = 5;
/// Bar height added by a reading at the top of the graph
const NOISE_BAR_SCALE: u64 = 100;

/// The dB range to graph: from the lowest visible reading up, at least NOISE_GRAPH_MIN_SPAN_DB tall
fn noise_graph_range(min: i16, max: i16) -> (i16, i16) {
    (min, max.max(min + NOISE_GRAPH_MIN_SPAN_DB))
}

/// Bar height for a noise floor reading within the graph's range
fn noise_bar(dbm: i16, (low, high): (i16, i16)) -> u64 {
    let above_low = (dbm.clamp(low, high) - low) as u64;
    NOISE_BAR_FLOOR + above_low * NOISE_BAR_SCALE / (high - low) as u64
}

fn draw_radio(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Radio ")
        .border_style(Style::default().fg(theme.radio_border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let stats = &app.radio_stats;
    let Some(latest) = stats.latest else {
        let waiting = Paragraph::new("waiting for radio stats…").style(Style::default().fg(theme.muted));
        f.render_widget(waiting, inner);
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    // Newest first, drawn from the right edge, so the latest reading is rightmost
    let visible: Vec<Option<i16>> = stats.noise_history.iter().rev().take(inner.width as usize).copied().collect();
    let readings = visible.iter().flatten().copied();
    let min_max = readings.clone().min().zip(readings.max());
    let range = min_max
        .map(|(min, max)| format!("  (min {} / max {})", min, max))
        .unwrap_or_default();
    let header = format!("Noise {} dBm{}", latest.noise_floor, range);
    f.render_widget(Paragraph::new(header).style(Style::default().fg(theme.text)), rows[0]);

    let graph = min_max.map(|(min, max)| noise_graph_range(min, max));
    let bars: Vec<SparklineBar> = visible
        .iter()
        .map(|r| match r.zip(graph) {
            Some((dbm, graph)) => {
                let bar = noise_bar(dbm, graph);
                SparklineBar::from(bar).style(Style::default().fg(noise_bar_color(theme, bar)))
            }
            None => SparklineBar::from(None),
        })
        .collect();
    let sparkline = Sparkline::default()
        .data(bars)
        .max(NOISE_BAR_FLOOR + NOISE_BAR_SCALE)
        .direction(RenderDirection::RightToLeft);
    f.render_widget(sparkline, rows[1]);

    let footer = format!(
        "RSSI {} dBm  SNR {:.2} dB   Air TX {}s RX {}s",
        latest.last_rssi, latest.last_snr, latest.tx_air_secs, latest.rx_air_secs
    );
    f.render_widget(Paragraph::new(footer).style(Style::default().fg(theme.muted)), rows[2]);
}

/// Colour for a noise bar by how high it sits in the graph: low, middle or top third
fn noise_bar_color(theme: &Theme, bar: u64) -> Color {
    match (bar - NOISE_BAR_FLOOR) * 3 / NOISE_BAR_SCALE {
        0 => theme.graph_low,
        1 => theme.graph_mid,
        _ => theme.graph_high,
    }
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let w = Paragraph::new(app.input.as_str())
        .style(Style::default().fg(theme.text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " Send to: {} (Enter=send, Tab=panel, ↑↓=contact, F1=deselect, Esc=quit) ",
                    app.target_label()
                ))
                .border_style(Style::default().fg(theme.input_border)),
        );
    f.render_widget(w, area);
    f.set_cursor_position((area.x + app.input.cursor_column() + 1, area.y + 1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::app::{ChatMessage, PacketLogEntry};
    use meshcore_rs::events::SelfInfo;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    // ── Rendering ────────────────────────────────────────────────────────────

    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().to_string()
    }

    fn chat_message(sender: &str, text: &str, msg_type: MsgType) -> ChatMessage {
        ChatMessage {
            timestamp: "12:34:56".to_string(),
            sender: sender.to_string(),
            text: text.to_string(),
            msg_type,
        }
    }

    fn packet(ptype: &str, hops: u8, info: &str) -> PacketLogEntry {
        PacketLogEntry {
            timestamp: "12:34:56".to_string(),
            ptype: ptype.to_string(),
            hops,
            rssi: 0,
            snr: 0.0,
            info: info.to_string(),
        }
    }

    fn connected_app() -> App {
        let mut app = App::new(0);
        app.connected = true;
        app.port_info = "/dev/ttyACM0".to_string();
        app.self_info = Some(SelfInfo {
            name: "Base".to_string(),
            radio_freq: 869_618,
            radio_bw: 62_500,
            sf: 8,
            cr: 5,
            ..Default::default()
        });
        app.channels.insert(0, channel("Public"));
        app.channels.insert(1, channel("Ops"));
        app.contacts.push(contact("Alice", [0xa1; 6]));
        app.contacts.push(contact("Bob the very long named repeater", [0xb2; 6]));
        app.rx_count = 3;
        app.tx_count = 1;
        app.messages = vec![
            chat_message("SYS", "Connected", MsgType::System),
            chat_message("Alice", "hi there", MsgType::Incoming),
            chat_message("\u{2192} Alice", "hello!", MsgType::Outgoing),
            chat_message("Carol", "anyone on ops?", MsgType::Channel(1)),
            chat_message("ERR", "Send failed: timeout", MsgType::Error),
        ];
        app.packets = vec![
            packet("ADV", 0, "Repeater @52.0089,4.3531"),
            packet("ACK", 0, "tag:deadbeef"),
            packet("PATH", 2, "a1a1a1a1a1a1 len=2"),
        ];
        app
    }

    /// Render one panel into a terminal of its own size
    fn render_panel(width: u16, height: u16, draw_panel: impl FnOnce(&mut Frame, Rect)) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_panel(f, f.area())).unwrap();
        terminal.backend().clone()
    }

    fn rows(backend: &TestBackend) -> Vec<String> {
        let buffer = backend.buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    fn rows_containing(rows: &[String], needle: &str) -> usize {
        rows.iter().filter(|row| row.contains(needle)).count()
    }

    /// Full-screen layout check; expect to re-accept this when the layout changes
    #[test]
    fn render_connected_chat() {
        insta::assert_snapshot!(render(&connected_app(), 80, 24));
    }

    #[test]
    fn status_shows_connection_state() {
        let connected = rows(&render_panel(120, 3, |f, area| draw_status(f, &connected_app(), area)));
        assert_eq!(rows_containing(&connected, "● Connected"), 1);
        let offline = rows(&render_panel(120, 3, |f, area| draw_status(f, &App::new(0), area)));
        assert_eq!(rows_containing(&offline, "○ Disconnected"), 1);
    }

    #[test]
    fn status_bar() {
        insta::assert_snapshot!(render_panel(120, 3, |f, area| draw_status(f, &connected_app(), area)));
    }

    #[test]
    fn messages_title_shows_device_and_target() {
        let rows = rows(&render_panel(60, 5, |f, area| draw_messages(f, &connected_app(), area)));
        assert!(rows[0].contains(" Base — Public "));
    }

    #[test]
    fn messages_show_newest_when_overflowing() {
        let mut app = App::new(0);
        app.messages = (0..30)
            .map(|i| chat_message("SYS", &format!("message {}", i), MsgType::System))
            .collect();
        // 8 rows minus borders leaves 6 visible messages: 24..=29
        let rows = rows(&render_panel(60, 8, |f, area| draw_messages(f, &app, area)));
        assert_eq!(rows_containing(&rows, "message 29"), 1);
        assert_eq!(rows_containing(&rows, "message 24"), 1);
        assert_eq!(rows_containing(&rows, "message 23"), 0);
    }

    #[test]
    fn messages_tag_each_type() {
        insta::assert_snapshot!(render_panel(60, 7, |f, area| draw_messages(f, &connected_app(), area)));
    }

    #[test]
    fn messages_wrap_under_the_text() {
        let mut app = App::new(0);
        app.messages = vec![chat_message("Alice", "abcdefghijklmnopqrstuvwxyz0123456789", MsgType::Incoming)];
        // 38 columns inside the border; the 22-column prefix leaves 16 for text
        insta::assert_snapshot!(render_panel(40, 6, |f, area| draw_messages(f, &app, area)));
    }

    #[test]
    fn contacts_title_counts_contacts() {
        let rows = rows(&render_panel(30, 5, |f, area| draw_contacts(f, &connected_app(), area)));
        assert!(rows[0].contains(" Contacts (2) "));
    }

    #[test]
    fn contacts_with_selection_and_truncation() {
        let mut app = connected_app();
        app.select_contact(0);
        insta::assert_snapshot!(render_panel(24, 4, |f, area| draw_contacts(f, &app, area)));
    }

    #[test]
    fn packets_show_newest_when_overflowing() {
        let mut app = App::new(0);
        app.packets = (0..10).map(|i| packet("ACK", 0, &format!("packet {}", i))).collect();
        // 5 rows minus borders leaves 3 visible packets: 7..=9
        let rows = rows(&render_panel(60, 5, |f, area| draw_packets(f, &app, area)));
        assert!(rows[1].contains("packet 7"));
        assert!(rows[3].contains("packet 9"));
        assert_eq!(rows_containing(&rows, "packet 6"), 0);
    }

    #[test]
    fn packets_show_signal_only_when_known() {
        let mut app = App::new(0);
        let mut heard = packet("ADV", 1, "heard");
        heard.rssi = -92;
        heard.snr = 5.25;
        app.packets = vec![packet("ACK", 0, "no signal"), heard];
        insta::assert_snapshot!(render_panel(60, 4, |f, area| draw_packets(f, &app, area)));
    }

    #[test]
    fn input_shows_text_and_send_target() {
        let mut app = connected_app();
        app.select_contact(0);
        for c in "hello".chars() {
            app.input.insert(c);
        }
        insta::assert_snapshot!(render_panel(60, 3, |f, area| draw_input(f, &app, area)));
    }

    // ── Radio panel ──────────────────────────────────────────────────────────

    fn app_with_noise(readings: &[Option<i16>]) -> App {
        let mut app = App::new(0);
        for reading in readings {
            let result = reading.map(radio_stats).ok_or_else(|| "timeout".to_string());
            app.radio_stats.record(result);
        }
        app
    }

    #[test]
    fn radio_waits_for_the_first_reading() {
        let app = app_with_noise(&[None]);
        let rows = rows(&render_panel(50, 7, |f, area| draw_radio(f, &app, area)));
        assert!(rows[1].contains("waiting for radio stats…"));
    }

    #[test]
    fn noise_graph_spans_the_readings_but_at_least_the_minimum() {
        assert_eq!(noise_graph_range(-115, -90), (-115, -90));
        assert_eq!(noise_graph_range(-108, -107), (-108, -98));
    }

    #[test]
    fn noise_bars_scale_within_the_graph_range() {
        let graph = (-108, -98);
        assert_eq!(noise_bar(-108, graph), NOISE_BAR_FLOOR);
        assert_eq!(noise_bar(-103, graph), NOISE_BAR_FLOOR + 50);
        assert_eq!(noise_bar(-98, graph), NOISE_BAR_FLOOR + NOISE_BAR_SCALE);
    }

    #[test]
    fn one_db_of_change_is_visible() {
        let app = app_with_noise(&[Some(-108), Some(-107)]);
        let rows = rows(&render_panel(50, 7, |f, area| draw_radio(f, &app, area)));
        let column = |x: usize| -> String { rows[2..5].iter().map(|r| r.chars().nth(x).unwrap()).collect() };
        assert_ne!(column(47), column(48));
    }

    #[test]
    fn radio_header_shows_range_of_visible_readings() {
        let app = app_with_noise(&[Some(-90), Some(-118), None, Some(-104)]);
        let rows = rows(&render_panel(50, 7, |f, area| draw_radio(f, &app, area)));
        assert!(rows[1].contains("Noise -104 dBm  (min -118 / max -90)"));
    }

    #[test]
    fn radio_panel_graphs_newest_reading_rightmost() {
        let app = app_with_noise(&[Some(-120), Some(-110), Some(-100), None, Some(-90), Some(-80)]);
        insta::assert_snapshot!(render_panel(50, 7, |f, area| draw_radio(f, &app, area)));
    }
}
