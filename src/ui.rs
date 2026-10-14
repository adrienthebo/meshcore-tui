use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, List, ListItem, Paragraph, RenderDirection, Sparkline,
        SparklineBar,
    },
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, MsgType, PacketKind, Route};
use crate::input::InputLine;
use crate::overview::{Overview, Panel};
use crate::tab_bar::TabBar;
use crate::text::{format_duration, sanitize, truncate_to_width, wrap_to_width};
use crate::theme::Theme;
use crate::view::{Ui, View};

// ── TUI Rendering ────────────────────────────────────────────────────────────

/// Draw the status bar and tab bar, which every tab shares, above the tab on screen
pub fn draw(f: &mut Frame, app: &App, ui: &Ui) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // status
            Constraint::Length(2), // tab bar
            Constraint::Min(0),    // tab
        ])
        .split(f.area());

    draw_status(f, app, &ui.theme, chunks[0]);
    draw_tab_bar(f, ui, chunks[1]);
    ui.active_tab().draw(f, chunks[2], app, &ui.theme, true);
}

/// Each tab's title, numbered with the F-key that shows it, and the
/// keys that work right now at the right end of the rule
fn draw_tab_bar(f: &mut Frame, ui: &Ui, area: Rect) {
    let titles: Vec<&str> = ui.tabs.iter().map(|tab| tab.title()).collect();
    let bar = TabBar::new(&titles, ui.active)
        .active_style(Style::default().fg(ui.theme.focus))
        .rule_style(Style::default().fg(ui.theme.muted));
    let room = (area.width as usize).saturating_sub(bar.titles_width());
    f.render_widget(bar, area);

    let hints = fit_hints(&ui.key_hints(), room);
    let rule = Rect {
        y: area.y + 1,
        height: 1,
        ..area
    };
    // Style the span, not the line: a line's style covers the whole row and would recolour the rule
    let hints = Line::from(Span::styled(hints, Style::default().fg(ui.theme.text))).right_aligned();
    f.render_widget(hints, rule);
}

/// As many hints as fit in `width` columns, padded by a space each side; the
/// first hints are the focused panel's, so the last ones are left out first
fn fit_hints(hints: &[&str], width: usize) -> String {
    (1..=hints.len())
        .rev()
        .map(|shown| format!(" {} ", hints[..shown].join("  ")))
        .find(|text| text.width() <= width)
        .unwrap_or_default()
}

/// Lay out the Overview's panels and draw each, highlighting the one with focus
pub fn draw_overview(f: &mut Frame, app: &App, overview: &Overview, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // main
            Constraint::Length(7), // packets
            Constraint::Length(3), // input
        ])
        .split(area);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(30)])
        .split(chunks[0]);

    let diagnostics = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(50)])
        .split(chunks[1]);

    let focused = |panel: Panel| overview.focus == panel;
    overview
        .messages
        .draw(f, main[0], app, theme, focused(Panel::Messages));
    overview
        .contacts
        .draw(f, main[1], app, theme, focused(Panel::Contacts));
    overview
        .packets
        .draw(f, diagnostics[0], app, theme, focused(Panel::Packets));
    overview.radio.draw(f, diagnostics[1], app, theme, false);
    overview
        .input
        .draw(f, chunks[2], app, theme, focused(Panel::Input));
}

fn draw_status(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
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

    let spans = vec![
        Span::styled("Port: ", Style::default().fg(theme.text)),
        Span::styled(&app.port_info, Style::default().fg(theme.info)),
        Span::styled(" | ", Style::default().fg(theme.muted)),
        Span::styled(&freq_info, Style::default().fg(theme.value)),
        Span::styled(" | ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("RX: {} | TX: {}", app.rx_count, app.tx_count),
            Style::default().fg(theme.good),
        ),
        Span::styled(" | ", Style::default().fg(theme.muted)),
        Span::styled(
            if app.connected {
                "● Connected"
            } else {
                "○ Disconnected"
            },
            Style::default().fg(if app.connected {
                theme.connected
            } else {
                theme.bad
            }),
        ),
    ];

    let w = Paragraph::new(Line::from(spans)).block(panel(theme, " Status ", theme.status_border));
    f.render_widget(w, area);
}

pub fn draw_messages(f: &mut Frame, app: &App, theme: &Theme, focused: bool, area: Rect) {
    let panel_width = (area.width as usize).saturating_sub(2); // border
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

            let chunks =
                if prefix_len + text_display_width <= panel_width || panel_width <= prefix_len {
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
                            Span::styled(
                                tag.clone(),
                                Style::default().fg(tag_c).add_modifier(Modifier::BOLD),
                            ),
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

    let bc = if focused {
        theme.focus
    } else {
        theme.messages_border
    };
    let title = format!(" {} — {} ", app.device_name(), app.target_label());
    let msg_w = List::new(visible_items).block(panel(theme, title, bc));
    f.render_widget(msg_w, area);
}

pub fn draw_contacts(f: &mut Frame, app: &App, theme: &Theme, focused: bool, area: Rect) {
    let cpanel_width = (area.width as usize).saturating_sub(2); // minus borders
    let selected = app.selected_index();
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

    let cc = if focused {
        theme.focus
    } else {
        theme.contacts_border
    };
    let c_w = List::new(citems).block(panel(
        theme,
        format!(" Contacts ({}) ", app.contacts.len()),
        cc,
    ));
    f.render_widget(c_w, area);
}

pub fn draw_packets(f: &mut Frame, app: &App, theme: &Theme, focused: bool, area: Rect) {
    let visible = (area.height as usize).saturating_sub(2);
    let items: Vec<ListItem> = app
        .packets
        .iter()
        .rev()
        .take(visible)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|p| {
            let tc = match p.kind {
                PacketKind::Advert => theme.packet_advert,
                PacketKind::Ack => theme.packet_ack,
                PacketKind::Sent => theme.packet_sent,
                PacketKind::Path => theme.packet_path,
            };
            ListItem::new(Line::from(vec![
                Span::styled(&p.timestamp, Style::default().fg(theme.muted)),
                Span::raw(" "),
                Span::styled(
                    // Six-letter kinds plus brackets, so the columns after line up
                    format!("{:<8}", format!("[{}]", p.kind.label())),
                    Style::default().fg(tc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    match p.route {
                        Some(Route::Flood) => " \u{2192}flood".to_string(),
                        Some(Route::Hops { count, .. }) => format!(" \u{2192}{}h", count),
                        None => String::new(),
                    },
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
                Span::styled(format!(" | {}", p.info), Style::default().fg(theme.text)),
            ]))
        })
        .collect();

    let pc = if focused {
        theme.focus
    } else {
        theme.packets_border
    };
    let w = List::new(items).block(panel(theme, " Activity ", pc));
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

pub fn draw_radio(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = panel(theme, " Radio ", theme.radio_border);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let stats = &app.radio_stats;
    let Some(latest) = stats.latest else {
        let waiting =
            Paragraph::new("waiting for radio stats…").style(Style::default().fg(theme.muted));
        f.render_widget(waiting, inner);
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    // Newest first, drawn from the right edge, so the latest reading is rightmost
    let visible: Vec<Option<i16>> = stats
        .noise_history
        .iter()
        .rev()
        .take(inner.width as usize)
        .copied()
        .collect();
    let readings = visible.iter().flatten().copied();
    let min_max = readings.clone().min().zip(readings.max());
    let range = min_max
        .map(|(min, max)| format!("  (min {} dBm / max {} dBm)", min, max))
        .unwrap_or_default();
    let noise_colour = |dbm: i16| theme.quality(noise_quality(dbm));
    let header = Line::from(vec![
        Span::styled("Noise ", Style::default().fg(theme.text)),
        quality_span(
            format!("{} dBm", latest.noise_floor),
            noise_quality(latest.noise_floor),
            theme,
        ),
        Span::styled(range, Style::default().fg(theme.text)),
    ]);
    f.render_widget(Paragraph::new(header), rows[0]);

    let graph = min_max.map(|(min, max)| noise_graph_range(min, max));
    let bars: Vec<SparklineBar> = visible
        .iter()
        .map(|r| match r.zip(graph) {
            Some((dbm, graph)) => SparklineBar::from(noise_bar(dbm, graph))
                .style(Style::default().fg(noise_colour(dbm))),
            None => SparklineBar::from(None),
        })
        .collect();
    let sparkline = Sparkline::default()
        .data(bars)
        .max(NOISE_BAR_FLOOR + NOISE_BAR_SCALE)
        .direction(RenderDirection::RightToLeft);
    f.render_widget(sparkline, rows[1]);

    let muted = Style::default().fg(theme.muted);
    let footer = Line::from(vec![
        Span::styled("RSSI ", muted),
        quality_span(
            format!("{} dBm", latest.last_rssi),
            rssi_quality(latest.last_rssi.into()),
            theme,
        ),
        Span::styled("  SNR ", muted),
        quality_span(
            format!("{:.2} dB", latest.last_snr),
            snr_quality(latest.last_snr),
            theme,
        ),
        Span::styled(
            format!(
                "   Air TX {} RX {}",
                format_duration(latest.tx_air_secs, 3),
                format_duration(latest.rx_air_secs, 3)
            ),
            muted,
        ),
    ]);
    f.render_widget(Paragraph::new(footer), rows[2]);
}

/// How good a reading is, from 0.0 at `poor` to 1.0 at `good`, linear between and
/// clamped outside; `poor` may be above `good` for readings where lower is better
fn quality(value: f32, poor: f32, good: f32) -> f32 {
    ((value - poor) / (good - poor)).clamp(0.0, 1.0)
}

/// Packet length, where shorter is better since airtime grows with length, up
/// to the longest packet the RX log shows
pub fn length_quality(bytes: usize) -> f32 {
    quality(bytes as f32, crate::rx_log::LONGEST_LOGGED as f32, 0.0)
}

/// RSSI from the edge of LoRa reception up to a strong local signal
pub fn rssi_quality(dbm: i16) -> f32 {
    quality(dbm as f32, -120.0, -60.0)
}

/// SNR from below what SF7 can decode (about -7.5 dB) up to a clean link
pub fn snr_quality(db: f32) -> f32 {
    quality(db, -10.0, 10.0)
}

/// Noise floor, where lower is better, down to the firmware's lowest reading of -120 dBm
pub fn noise_quality(dbm: i16) -> f32 {
    quality(dbm as f32, -90.0, -120.0)
}

/// `text` in the colour for a reading's quality, from 0.0 (poor) to 1.0 (good)
pub fn quality_span(text: String, quality: f32, theme: &Theme) -> Span<'static> {
    Span::styled(text, Style::default().fg(theme.quality(quality)))
}

/// The input title counts the bytes left once this few remain
const BYTES_LEFT_SHOWN: usize = 20;

/// The input line, with the terminal cursor in it while it has focus
pub fn draw_input(
    f: &mut Frame,
    app: &App,
    input: &InputLine,
    theme: &Theme,
    focused: bool,
    area: Rect,
) {
    let bytes_left = app.text_limit().saturating_sub(input.as_str().len());
    let title = if bytes_left <= BYTES_LEFT_SHOWN {
        format!(
            " Send to: {} · {} bytes left ",
            app.target_label(),
            bytes_left
        )
    } else {
        format!(" Send to: {} ", app.target_label())
    };
    let border = if focused {
        theme.focus
    } else {
        theme.input_border
    };
    let w = Paragraph::new(input.as_str())
        .style(Style::default().fg(theme.text))
        .block(panel(theme, title, border));
    f.render_widget(w, area);
    if focused {
        f.set_cursor_position((area.x + input.cursor_column() + 1, area.y + 1));
    }
}

/// A panel with a rounded border and a bold title
pub fn panel<'a>(theme: &Theme, title: impl Into<Line<'a>>, border: Color) -> Block<'a> {
    let title_style = Style::default()
        .fg(theme.title.unwrap_or(border))
        .add_modifier(Modifier::BOLD);
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(title.into().style(title_style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ChatMessage, PacketLogEntry};
    use crate::test_support::*;
    use meshcore_rs::events::SelfInfo;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    // ── Rendering ────────────────────────────────────────────────────────────

    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app, &test_ui())).unwrap();
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

    fn packet(kind: PacketKind, route: Option<Route>, info: &str) -> PacketLogEntry {
        PacketLogEntry {
            timestamp: "12:34:56".to_string(),
            kind,
            route,
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
        app.contacts
            .push(contact("Bob the very long named repeater", [0xb2; 6]));
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
            packet(
                PacketKind::Advert,
                Some(Route::Hops {
                    count: 2,
                    hash_size: 1,
                }),
                "Repeater @52.0089,4.3531",
            ),
            packet(PacketKind::Ack, None, "tag:deadbeef"),
            packet(PacketKind::Path, None, "a1a1a1a1a1a1 len=2"),
        ];
        app
    }

    /// Render one panel into a terminal of its own size
    fn render_panel(
        width: u16,
        height: u16,
        draw_panel: impl FnOnce(&mut Frame, Rect),
    ) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_panel(f, f.area())).unwrap();
        terminal.backend().clone()
    }

    fn rows(backend: &TestBackend) -> Vec<String> {
        let buffer = backend.buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
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
        let connected = rows(&render_panel(120, 3, |f, area| {
            draw_status(f, &connected_app(), &Theme::CLASSIC, area)
        }));
        assert_eq!(rows_containing(&connected, "● Connected"), 1);
        let offline = rows(&render_panel(120, 3, |f, area| {
            draw_status(f, &App::new(0), &Theme::CLASSIC, area)
        }));
        assert_eq!(rows_containing(&offline, "○ Disconnected"), 1);
    }

    #[test]
    fn status_bar() {
        insta::assert_snapshot!(render_panel(120, 3, |f, area| draw_status(
            f,
            &connected_app(),
            &Theme::CLASSIC,
            area
        )));
    }

    #[test]
    fn messages_title_shows_device_and_target() {
        let rows = rows(&render_panel(60, 5, |f, area| {
            draw_messages(f, &connected_app(), &Theme::CLASSIC, false, area)
        }));
        assert!(rows[0].contains(" Base — Public "));
    }

    #[test]
    fn messages_show_newest_when_overflowing() {
        let mut app = App::new(0);
        app.messages = (0..30)
            .map(|i| chat_message("SYS", &format!("message {}", i), MsgType::System))
            .collect();
        // 8 rows minus borders leaves 6 visible messages: 24..=29
        let rows = rows(&render_panel(60, 8, |f, area| {
            draw_messages(f, &app, &Theme::CLASSIC, false, area)
        }));
        assert_eq!(rows_containing(&rows, "message 29"), 1);
        assert_eq!(rows_containing(&rows, "message 24"), 1);
        assert_eq!(rows_containing(&rows, "message 23"), 0);
    }

    #[test]
    fn messages_tag_each_type() {
        insta::assert_snapshot!(render_panel(60, 7, |f, area| draw_messages(
            f,
            &connected_app(),
            &Theme::CLASSIC,
            false,
            area
        )));
    }

    #[test]
    fn messages_wrap_under_the_text() {
        let mut app = App::new(0);
        app.messages = vec![chat_message(
            "Alice",
            "abcdefghijklmnopqrstuvwxyz0123456789",
            MsgType::Incoming,
        )];
        // 38 columns inside the border; the 22-column prefix leaves 16 for text
        insta::assert_snapshot!(render_panel(40, 6, |f, area| draw_messages(
            f,
            &app,
            &Theme::CLASSIC,
            false,
            area
        )));
    }

    #[test]
    fn contacts_title_counts_contacts() {
        let rows = rows(&render_panel(30, 5, |f, area| {
            draw_contacts(f, &connected_app(), &Theme::CLASSIC, false, area)
        }));
        assert!(rows[0].contains(" Contacts (2) "));
    }

    #[test]
    fn contacts_with_selection_and_truncation() {
        let mut app = connected_app();
        app.select_contact(app.contacts[0].public_key);
        insta::assert_snapshot!(render_panel(24, 4, |f, area| draw_contacts(
            f,
            &app,
            &Theme::CLASSIC,
            false,
            area
        )));
    }

    #[test]
    fn packets_show_newest_when_overflowing() {
        let mut app = App::new(0);
        app.packets = (0..10)
            .map(|i| packet(PacketKind::Ack, None, &format!("packet {}", i)))
            .collect();
        // 5 rows minus borders leaves 3 visible packets: 7..=9
        let rows = rows(&render_panel(60, 5, |f, area| {
            draw_packets(f, &app, &Theme::CLASSIC, false, area)
        }));
        assert!(rows[1].contains("packet 7"));
        assert!(rows[3].contains("packet 9"));
        assert_eq!(rows_containing(&rows, "packet 6"), 0);
    }

    #[test]
    fn packets_show_signal_only_when_known() {
        let mut app = App::new(0);
        let mut heard = packet(PacketKind::Advert, Some(Route::Flood), "heard");
        heard.rssi = -92;
        heard.snr = 5.25;
        app.packets = vec![packet(PacketKind::Ack, None, "no signal"), heard];
        insta::assert_snapshot!(render_panel(60, 4, |f, area| draw_packets(
            f,
            &app,
            &Theme::CLASSIC,
            false,
            area
        )));
    }

    #[test]
    fn input_shows_text_and_send_target() {
        let mut app = connected_app();
        app.select_contact(app.contacts[0].public_key);
        let mut input = InputLine::default();
        for c in "hello".chars() {
            input.insert(c);
        }
        insta::assert_snapshot!(render_panel(60, 3, |f, area| draw_input(
            f,
            &app,
            &input,
            &Theme::CLASSIC,
            true,
            area
        )));
    }

    #[test]
    fn input_title_counts_bytes_left_near_the_limit() {
        let mut app = connected_app();
        app.select_contact(app.contacts[0].public_key);
        let mut input = InputLine::default();
        for c in "x".repeat(140).chars() {
            input.insert(c);
        }
        let title = |input: &InputLine| {
            let backend = render_panel(60, 3, |f, area| {
                draw_input(f, &app, input, &Theme::CLASSIC, true, area)
            });
            let buffer = backend.buffer();
            (0..60)
                .map(|x| buffer[(x, 0)].symbol().to_string())
                .collect::<String>()
        };
        assert!(
            title(&input).contains("Send to: Alice · 20 bytes left"),
            "{}",
            title(&input)
        );
        input.backspace();
        assert!(!title(&input).contains("bytes left"), "{}", title(&input));
    }

    // ── Radio panel ──────────────────────────────────────────────────────────

    fn app_with_noise(readings: &[Option<i16>]) -> App {
        let mut app = App::new(0);
        for reading in readings {
            let result = reading
                .map(radio_stats)
                .ok_or_else(|| "timeout".to_string());
            app.radio_stats.record(result);
        }
        app
    }

    #[test]
    fn radio_waits_for_the_first_reading() {
        let app = app_with_noise(&[None]);
        let rows = rows(&render_panel(50, 7, |f, area| {
            draw_radio(f, &app, &Theme::CLASSIC, area)
        }));
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
    fn signal_quality_spans_poor_to_good() {
        assert_eq!(rssi_quality(-120), 0.0);
        assert_eq!(rssi_quality(-90), 0.5);
        assert_eq!(rssi_quality(-27), 1.0);
        assert_eq!(snr_quality(-12.0), 0.0);
        assert_eq!(snr_quality(0.0), 0.5);
        assert_eq!(snr_quality(10.0), 1.0);
    }

    #[test]
    fn a_shorter_packet_is_better() {
        assert_eq!(length_quality(0), 1.0);
        assert_eq!(length_quality(173), 0.0);
        assert!(length_quality(55) > length_quality(117));
    }

    #[test]
    fn a_lower_noise_floor_is_better() {
        assert_eq!(noise_quality(-80), 0.0);
        assert_eq!(noise_quality(-105), 0.5);
        assert_eq!(noise_quality(-120), 1.0);
    }

    #[test]
    fn radio_values_are_coloured_by_quality() {
        let app = app_with_noise(&[Some(-120), Some(-90)]);
        let theme = Theme::VAPORWAVE;
        let backend = render_panel(50, 7, |f, area| draw_radio(f, &app, &theme, area));
        let buffer = backend.buffer();
        let colour_of = |row: u16, text: &str| {
            let line: String = (0..50)
                .map(|x| buffer[(x, row)].symbol().to_string())
                .collect();
            let x = line
                .find(text)
                .unwrap_or_else(|| panic!("{text} not on row {row}: {line}"));
            buffer[(line[..x].chars().count() as u16, row)].fg
        };
        // Current noise -90 dBm is poor; the test RSSI of -90 dBm is fair
        assert_eq!(colour_of(1, "-90 dBm"), theme.quality_poor);
        assert_eq!(colour_of(5, "-90 dBm"), theme.quality_fair);
        // Bars, newest rightmost: -90 is poor, -120 is good
        assert_eq!(buffer[(48, 4)].fg, theme.quality_poor);
        assert_eq!(buffer[(47, 4)].fg, theme.quality_good);
    }

    #[test]
    fn panels_draw_with_the_uis_theme() {
        let app = connected_app();
        let mut ui = Ui::new(Theme::VAPORWAVE);
        ui.tabs[0] = Box::new(Overview {
            focus: Panel::Contacts,
            ..Overview::default()
        });
        let backend = render_panel(80, 24, |f, _| draw(f, &app, &ui));
        let buffer = backend.buffer();
        // Status border, the current tab's outline over the muted rule and the key
        // hints at its end, then the focused contacts border and the unfocused
        // messages border
        assert_eq!(buffer[(0, 0)].fg, ui.theme.status_border);
        assert_eq!(buffer[(1, 3)].fg, ui.theme.focus);
        assert_eq!(buffer[(1, 4)].fg, ui.theme.focus);
        assert_eq!(buffer[(20, 4)].fg, ui.theme.muted);
        assert_eq!(buffer[(78, 4)].fg, ui.theme.text);
        assert_eq!(buffer[(79, 5)].fg, ui.theme.focus);
        assert_eq!(buffer[(0, 5)].fg, ui.theme.messages_border);
    }

    #[test]
    fn hints_that_dont_fit_are_left_out_from_the_end() {
        let hints = ["Enter=send", "Tab=panel", "Esc=quit"];
        assert_eq!(fit_hints(&hints, 80), " Enter=send  Tab=panel  Esc=quit ");
        assert_eq!(fit_hints(&hints, 24), " Enter=send  Tab=panel ");
        assert_eq!(fit_hints(&hints, 11), "");
    }

    #[test]
    fn input_border_shows_focus() {
        let (app, input, theme) = (App::new(0), InputLine::default(), Theme::VAPORWAVE);
        let border = |focused| {
            render_panel(60, 3, |f, area| {
                draw_input(f, &app, &input, &theme, focused, area)
            })
            .buffer()[(0, 0)]
                .fg
        };
        assert_eq!(border(true), theme.focus);
        assert_eq!(border(false), theme.input_border);
    }

    #[test]
    fn connection_indicator_uses_the_connected_colour() {
        let app = connected_app();
        let backend = render_panel(120, 3, |f, area| {
            draw_status(f, &app, &Theme::VAPORWAVE, area)
        });
        let indicator = backend
            .buffer()
            .content()
            .iter()
            .find(|c| c.symbol() == "●")
            .unwrap();
        assert_eq!(indicator.fg, Color::Rgb(0x26, 0xff, 0x4e));
    }

    #[test]
    fn panel_titles_are_bold_in_the_title_colour() {
        let app = connected_app();
        for theme in [Theme::VAPORWAVE, Theme::CLASSIC] {
            let backend = render_panel(60, 3, |f, area| draw_status(f, &app, &theme, area));
            // The "S" of " Status "
            let cell = &backend.buffer()[(2, 0)];
            assert_eq!(cell.symbol(), "S");
            assert_eq!(cell.fg, theme.title.unwrap_or(theme.status_border));
            assert!(cell.modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn one_db_of_change_is_visible() {
        let app = app_with_noise(&[Some(-108), Some(-107)]);
        let rows = rows(&render_panel(50, 7, |f, area| {
            draw_radio(f, &app, &Theme::CLASSIC, area)
        }));
        let column = |x: usize| -> String {
            rows[2..5]
                .iter()
                .map(|r| r.chars().nth(x).unwrap())
                .collect()
        };
        assert_ne!(column(47), column(48));
    }

    #[test]
    fn radio_header_shows_range_of_visible_readings() {
        let app = app_with_noise(&[Some(-90), Some(-118), None, Some(-104)]);
        let rows = rows(&render_panel(50, 7, |f, area| {
            draw_radio(f, &app, &Theme::CLASSIC, area)
        }));
        assert!(rows[1].contains("Noise -104 dBm  (min -118 dBm / max -90 dBm)"));
    }

    #[test]
    fn radio_panel_graphs_newest_reading_rightmost() {
        let app = app_with_noise(&[
            Some(-120),
            Some(-110),
            Some(-100),
            None,
            Some(-90),
            Some(-80),
        ]);
        insta::assert_snapshot!(render_panel(50, 7, |f, area| draw_radio(
            f,
            &app,
            &Theme::CLASSIC,
            area
        )));
    }
}
