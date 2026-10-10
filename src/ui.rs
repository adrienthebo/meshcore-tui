use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, MsgType, Panel};
use crate::text::{sanitize, truncate_to_width, wrap_to_width};

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

    draw_status(f, app, chunks[0]);
    draw_main(f, app, chunks[1]);
    draw_packets(f, app, chunks[2]);
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

    let spans = vec![
        Span::styled("Port: ", Style::default().fg(Color::White)),
        Span::styled(&app.port_info, Style::default().fg(Color::Cyan)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &freq_info,
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("RX: {} | TX: {}", app.rx_count, app.tx_count),
            Style::default().fg(Color::Green),
        ),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if app.connected { "● Connected" } else { "○ Disconnected" },
            Style::default().fg(if app.connected {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ];

    let w = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Status ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(w, area);
}

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(30)])
        .split(area);

    // Messages
    let panel_width = (chunks[0].width as usize).saturating_sub(2); // border
    let items: Vec<ListItem> = app
        .messages
        .iter()
        .map(|m| {
            let (tag, tag_c, name_c) = match &m.msg_type {
                MsgType::Incoming => ("[MSG]".to_string(), Color::Cyan, Color::Cyan),
                MsgType::Outgoing => ("[SND]".to_string(), Color::Green, Color::Green),
                MsgType::Channel(ch) => {
                    let label = app.channel_label(*ch);
                    (format!("[{}]", label), Color::Yellow, Color::Yellow)
                }
                MsgType::System => ("[SYS]".to_string(), Color::DarkGray, Color::DarkGray),
                MsgType::Error => ("[ERR]".to_string(), Color::Red, Color::Red),
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
                    let body = Span::styled(chunk, Style::default().fg(Color::White));
                    if i == 0 {
                        Line::from(vec![
                            Span::styled(&m.timestamp, Style::default().fg(Color::DarkGray)),
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
                                Style::default().fg(Color::DarkGray),
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
    let visible_height = (chunks[0].height as usize).saturating_sub(2);
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
        Color::Yellow
    } else {
        Color::Cyan
    };
    let msg_w = List::new(visible_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} — {} ", app.device_name(), app.target_label()))
            .border_style(Style::default().fg(bc)),
    );
    f.render_widget(msg_w, chunks[0]);

    // Contacts
    let cpanel_width = (chunks[1].width as usize).saturating_sub(2); // minus borders
    let citems: Vec<ListItem> = app
        .contacts
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let sel = app.selected_contact == Some(i);
            let sty = if sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
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
        Color::Yellow
    } else {
        Color::Green
    };
    let c_w = List::new(citems).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Contacts ({}) ", app.contacts.len()))
            .border_style(Style::default().fg(cc)),
    );
    f.render_widget(c_w, chunks[1]);
}

fn draw_packets(f: &mut Frame, app: &App, area: Rect) {
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
            let tc = match p.ptype.as_str() {
                "ADV" => Color::Magenta,
                "ACK" => Color::Cyan,
                "SND" => Color::Green,
                "PATH" => Color::Blue,
                _ => Color::White,
            };
            ListItem::new(Line::from(vec![
                Span::styled(&p.timestamp, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    format!("[{}]", p.ptype),
                    Style::default().fg(tc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" H:{}", p.hops),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    if p.rssi != 0 || p.snr != 0.0 {
                        format!(" R:{}dBm S:{:.1}dB", p.rssi, p.snr)
                    } else {
                        String::new()
                    },
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" | {}", p.info),
                    Style::default().fg(Color::White),
                ),
            ]))
        })
        .collect();

    let pc = if app.panel == Panel::Packets {
        Color::Yellow
    } else {
        Color::Magenta
    };
    let w = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Received Packets ")
            .border_style(Style::default().fg(pc)),
    );
    f.render_widget(w, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let w = Paragraph::new(app.input.as_str())
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " Send to: {} (Enter=send, Tab=panel, ↑↓=contact, F1=deselect, Esc=quit) ",
                    app.target_label()
                ))
                .border_style(Style::default().fg(Color::Cyan)),
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
    use ratatui::Terminal;

    // ── Rendering ────────────────────────────────────────────────────────────

    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
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

    #[test]
    fn render_offline() {
        insta::assert_snapshot!(render(&App::new(0), 80, 24));
    }

    #[test]
    fn render_connected_chat() {
        insta::assert_snapshot!(render(&connected_app(), 80, 24));
    }

    #[test]
    fn render_selected_contact_with_input() {
        let mut app = connected_app();
        app.panel = Panel::Contacts;
        app.selected_contact = Some(1);
        for c in "typing…".chars() {
            app.input.insert(c);
        }
        insta::assert_snapshot!(render(&app, 80, 24));
    }

    #[test]
    fn render_wraps_long_messages() {
        let mut app = connected_app();
        app.messages.push(chat_message(
            "Alice",
            "this message is long enough that it has to wrap across several lines of the message panel",
            MsgType::Incoming,
        ));
        app.messages.push(chat_message("Bob", "日本語のテキストも折り返す必要があります。日本語のテキスト", MsgType::Incoming));
        insta::assert_snapshot!(render(&app, 80, 24));
    }

    #[test]
    fn render_shows_newest_entries_when_panels_overflow() {
        let mut app = connected_app();
        for i in 0..30 {
            app.messages.push(chat_message("SYS", &format!("message {}", i), MsgType::System));
            app.packets.push(packet("ACK", 0, &format!("packet {}", i)));
        }
        insta::assert_snapshot!(render(&app, 80, 24));
    }

    #[test]
    fn render_narrow_terminal() {
        insta::assert_snapshot!(render(&connected_app(), 60, 24));
    }
}
