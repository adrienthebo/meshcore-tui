use chrono::Utc;
use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Chart, Dataset, GraphType, Paragraph},
    Frame,
};

use crate::app::App;
use crate::stats::{ClockReading, HISTORY_SECS};
use crate::theme::Theme;
use crate::text::{format_duration, sanitize};
use crate::ui::{noise_quality, panel, rssi_quality, snr_quality};
use crate::view::{KeyResult, View};

/// Rows for the two rows of reading panels under the charts
const PANEL_ROWS: u16 = 12;
/// Tallest each chart gets; past this a taller chart shows nothing more
const CHART_MAX: u16 = 14;
/// Shortest chart worth drawing: borders, two axis rows and one row of data
const CHART_MIN: u16 = 5;

/// Radio diagnostics: the noise floor and packet SNR over time above the
/// radio's signal, airtime, packet, device, error and settings readings
#[derive(Debug, Default)]
pub struct RadioTab;

impl View for RadioTab {
    fn title(&self) -> &str {
        "Radio"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_radio_tab(f, app, theme, Utc::now().timestamp(), area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }

    fn shows_diagnostics(&self) -> bool {
        true
    }
}

/// Heights of the noise floor and SNR charts in `rows`; the SNR chart gives up
/// its rows first, and a chart too short to read is left out
fn chart_heights(rows: u16) -> (u16, u16) {
    let noise = rows.min(CHART_MAX);
    let snr = (rows - noise).min(CHART_MAX);
    let readable = |height: u16| if height < CHART_MIN { 0 } else { height };
    (readable(noise), readable(snr))
}

/// Three panels side by side, equal width
fn three_columns(area: Rect) -> [Rect; 3] {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1); 3])
        .areas(area)
}

fn draw_radio_tab(f: &mut Frame, app: &App, theme: &Theme, now: i64, area: Rect) {
    let (noise_height, snr_height) = chart_heights(area.height.saturating_sub(PANEL_ROWS));
    let [noise, snr, readings, device, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(noise_height),
            Constraint::Length(snr_height),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Fill(1),
        ])
        .areas(area);
    draw_noise_chart(f, app, theme, noise);
    draw_snr_chart(f, app, theme, now, snr);

    let [signal, airtime, packets] = three_columns(readings);
    draw_signal(f, app, theme, signal);
    draw_airtime(f, app, theme, airtime);
    draw_packets(f, app, theme, packets);

    let [device, errors, settings] = three_columns(device);
    draw_device(f, app, theme, device);
    draw_errors(f, app, theme, errors);
    draw_settings(f, app, theme, settings);
}

/// Shown in place of a reading the radio hasn't sent yet
const UNKNOWN: &str = "—";

/// Width of the Signal and Device panels' labels, so their values line up
const LABEL_WIDTH: usize = 10;

/// A reading: its label, muted and padded to `width`, then its value
fn reading<'a>(theme: &Theme, label: &'a str, width: usize, value: Span<'a>) -> [Span<'a>; 2] {
    [Span::styled(format!("{:<width$}", label), Style::default().fg(theme.muted)), value]
}

fn value<'a>(theme: &Theme, text: impl Into<std::borrow::Cow<'a, str>>) -> Span<'a> {
    Span::styled(text, Style::default().fg(theme.value))
}

fn draw_readings(f: &mut Frame, theme: &Theme, title: impl Into<Line<'static>>, rows: Vec<Line>, area: Rect) {
    f.render_widget(Paragraph::new(rows).block(panel(theme, title, theme.radio_border)), area);
}

/// The last packet's signal, and how many packets wait to go out
fn draw_signal(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let last = app.radio_stats.packet_signals.back();
    let rssi = match last {
        Some(p) => Span::styled(format!("{} dBm", p.rssi), Style::default().fg(theme.quality(rssi_quality(p.rssi)))),
        None => value(theme, UNKNOWN),
    };
    let snr = match last {
        Some(p) => Span::styled(format!("{:.2} dB", p.snr), Style::default().fg(theme.quality(snr_quality(p.snr)))),
        None => value(theme, UNKNOWN),
    };
    let queue = app.diagnostics.core.map_or(UNKNOWN.to_string(), |core| core.queue_len.to_string());
    let rows = vec![
        Line::from_iter(reading(theme, "Last RSSI", LABEL_WIDTH, rssi)),
        Line::from_iter(reading(theme, "Last SNR", LABEL_WIDTH, snr)),
        Line::from_iter(reading(theme, "TX queue", LABEL_WIDTH, value(theme, queue))),
    ];
    draw_readings(f, theme, " Signal ", rows, area);
}

/// Seconds on air as a share of uptime; None before the radio has been up a second
fn duty_cycle(air_secs: u32, uptime_secs: u32) -> Option<f64> {
    (uptime_secs > 0).then(|| air_secs as f64 / uptime_secs as f64)
}

/// A bar `width` cells long at 100%, in eighths of a cell; anything above zero shows
fn duty_bar(fraction: f64, width: usize) -> String {
    const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let eighths = (fraction.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let eighths = if fraction > 0.0 { eighths.max(1) } else { 0 };
    format!("{}{}", "█".repeat(eighths / 8), EIGHTHS[eighths % 8])
}

/// Time on air since the radio booted, as a share of its uptime and in total
fn draw_airtime(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let uptime = app.diagnostics.core.map(|core| core.uptime_secs);
    let title = match uptime {
        Some(secs) => format!(" Airtime (up {}) ", format_duration(secs)),
        None => " Airtime ".to_string(),
    };
    let Some(stats) = app.radio_stats.latest else {
        return waiting(f, theme, panel(theme, title, theme.radio_border), "waiting for radio stats…", area);
    };
    let share = |air_secs: u32| match uptime.and_then(|up| duty_cycle(air_secs, up)) {
        Some(fraction) => format!("{:>5.1}%  {}", fraction * 100.0, duty_bar(fraction, 10)),
        None => UNKNOWN.to_string(),
    };
    let totals = reading(theme, "TX ", 3, value(theme, format_duration(stats.tx_air_secs)))
        .into_iter()
        .chain(reading(theme, "  RX ", 5, value(theme, format_duration(stats.rx_air_secs))));
    let rows = vec![
        Line::from_iter(reading(theme, "TX", 3, value(theme, share(stats.tx_air_secs)))),
        Line::from_iter(reading(theme, "RX", 3, value(theme, share(stats.rx_air_secs)))),
        Line::from_iter(totals),
    ];
    draw_readings(f, theme, title, rows, area);
}

/// A count with thousands separators, e.g. 1,204
fn with_separators(n: u32) -> String {
    let digits = n.to_string();
    let mut text = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// Packets received and sent since boot, by how they were routed
fn draw_packets(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = panel(theme, " Packets ", theme.radio_border);
    let Some(packets) = app.diagnostics.packets else {
        return waiting(f, theme, block, "waiting for diagnostics…", area);
    };
    let muted = Style::default().fg(theme.muted);
    let row = |label: &'static str, rx: u32, tx: u32| {
        Line::from(vec![
            Span::styled(format!("{:<7}", label), muted),
            value(theme, format!("{:>8}{:>8}", with_separators(rx), with_separators(tx))),
        ])
    };
    let rows = vec![
        Line::styled(format!("{:<7}{:>8}{:>8}", "", "rx", "tx"), muted),
        row("flood", packets.flood_rx, packets.flood_tx),
        row("direct", packets.direct_rx, packets.direct_tx),
        row("total", packets.recv, packets.sent),
    ];
    f.render_widget(Paragraph::new(rows).block(block), area);
}

/// How far the radio's clock is from the host's, e.g. "5s behind"
fn clock_drift(clock: ClockReading) -> String {
    let drift = clock.radio as i64 - clock.host;
    let size = format_duration(drift.unsigned_abs().try_into().unwrap_or(u32::MAX));
    match drift {
        0 => "in sync".to_string(),
        d if d > 0 => format!("{} ahead", size),
        _ => format!("{} behind", size),
    }
}

/// The radio's battery, storage and clock
fn draw_device(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = panel(theme, " Device ", theme.radio_border);
    let (Some(battery), Some(clock)) = (app.diagnostics.battery.as_ref(), app.diagnostics.clock) else {
        return waiting(f, theme, block, "waiting for diagnostics…", area);
    };
    let storage = match (battery.used_kb, battery.total_kb) {
        (Some(used), Some(total)) => format!("{} / {} KB", with_separators(used), with_separators(total)),
        _ => UNKNOWN.to_string(),
    };
    let rows = vec![
        Line::from_iter(reading(theme, "Battery", LABEL_WIDTH, value(theme, format!("{:.2} V", battery.battery_mv as f64 / 1000.0)))),
        Line::from_iter(reading(theme, "Storage", LABEL_WIDTH, value(theme, storage))),
        Line::from_iter(reading(theme, "Clock", LABEL_WIDTH, value(theme, clock_drift(clock)))),
        // The radio reports its MCU temperature only as self telemetry, which meshcore-rs can't request yet
        Line::from_iter(reading(theme, "MCU temp", LABEL_WIDTH, value(theme, UNKNOWN))),
    ];
    f.render_widget(Paragraph::new(rows).block(block), area);
}

/// Width of the Settings panel's labels, short so the firmware build fits
const SETTINGS_LABEL_WIDTH: usize = 6;

/// Where the radio advertises itself, in degrees; None when it has no location set
fn location(lat_microdeg: i32, lon_microdeg: i32) -> Option<String> {
    let degrees = |microdeg: i32| microdeg as f64 / 1_000_000.0;
    ((lat_microdeg, lon_microdeg) != (0, 0))
        .then(|| format!("{:.4}, {:.4}", degrees(lat_microdeg), degrees(lon_microdeg)))
}

/// The radio's TX power, firmware, model and advertised location
fn draw_settings(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = panel(theme, " Settings ", theme.radio_border);
    let Some(info) = &app.self_info else {
        return waiting(f, theme, block, "waiting for device info…", area);
    };
    let device = app.device_info.as_ref();
    let firmware = match device.map(|d| (&d.version, &d.fw_build)) {
        Some((Some(version), Some(build))) => format!("{} {}", version, build),
        Some((Some(version), None)) => version.clone(),
        _ => UNKNOWN.to_string(),
    };
    let model = device.and_then(|d| d.model.as_deref()).map_or(UNKNOWN.to_string(), sanitize);
    let place = location(info.adv_lat, info.adv_lon).unwrap_or("not set".to_string());
    let rows = vec![
        Line::from_iter(reading(theme, "TX", SETTINGS_LABEL_WIDTH, value(theme, format!("{} / {} dBm", info.tx_power, info.max_tx_power)))),
        Line::from_iter(reading(theme, "FW", SETTINGS_LABEL_WIDTH, value(theme, sanitize(&firmware)))),
        Line::from_iter(reading(theme, "Model", SETTINGS_LABEL_WIDTH, value(theme, model))),
        Line::from_iter(reading(theme, "Loc", SETTINGS_LABEL_WIDTH, value(theme, place))),
    ];
    f.render_widget(Paragraph::new(rows).block(block), area);
}

/// The firmware's error flags (`ERR_EVENT_*` in MeshCore's Dispatcher.h), by
/// bit. Each stays set until the radio reboots.
const ERROR_FLAGS: [(u16, &str); 3] = [
    (1 << 0, "Packet pool full"),
    (1 << 1, "CAD busy timeout"),
    (1 << 2, "RX stuck >8s"),
];

/// Width of the Errors panel's labels, so its values line up
const ERROR_LABEL_WIDTH: usize = 18;

/// Each error flag's label, and whether it is set in `errors`
fn error_flags(errors: u16) -> impl Iterator<Item = (&'static str, bool)> {
    ERROR_FLAGS.into_iter().map(move |(bit, label)| (label, errors & bit != 0))
}

/// Packets the radio failed to read, and the error flags it has raised
fn draw_errors(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = panel(theme, " Errors (since boot) ", theme.radio_border);
    let (Some(core), Some(packets)) = (app.diagnostics.core, app.diagnostics.packets) else {
        return waiting(f, theme, block, "waiting for diagnostics…", area);
    };
    let recv_errors = packets.recv_errors.map_or(UNKNOWN.to_string(), with_separators);
    let mut rows = vec![Line::from_iter(reading(theme, "RX errors", ERROR_LABEL_WIDTH, value(theme, recv_errors)))];
    rows.extend(error_flags(core.errors).map(|(label, set)| {
        let state = if set {
            Span::styled("SET", Style::default().fg(theme.bad))
        } else {
            Span::styled("ok", Style::default().fg(theme.good))
        };
        Line::from_iter(reading(theme, label, ERROR_LABEL_WIDTH, state))
    }));
    f.render_widget(Paragraph::new(rows).block(block), area);
}

/// The time axis shared by both charts: the history up to now, in seconds.
/// Labelled at the ends and the middle only, because ratatui spaces the
/// labels between those unevenly.
fn time_axis(theme: &Theme) -> Axis<'static> {
    let minutes = HISTORY_SECS / 60;
    let labels = [format!("-{}m", minutes), format!("-{}m", minutes / 2), "now".to_string()];
    Axis::default()
        .bounds([-(HISTORY_SECS as f64), 0.0])
        .labels(labels)
        .style(Style::default().fg(theme.muted))
}

/// A value axis from `low` to `high`, labelled at both ends and the middle
fn value_axis(theme: &Theme, low: f64, high: f64) -> Axis<'static> {
    let label = |v: f64| format!("{:.0}", v);
    Axis::default()
        .bounds([low, high])
        .labels([label(low), label((low + high) / 2.0), label(high)])
        .style(Style::default().fg(theme.muted))
}

/// Bounds that hold `values` with room to spare, on multiples of `step`
fn padded_bounds(values: impl Iterator<Item = f64>, step: f64) -> Option<(f64, f64)> {
    let (low, high) = values.fold(None, |range: Option<(f64, f64)>, v| match range {
        Some((low, high)) => Some((low.min(v), high.max(v))),
        None => Some((v, v)),
    })?;
    Some(((low / step).floor() * step - step, (high / step).ceil() * step + step))
}

fn waiting(f: &mut Frame, theme: &Theme, block: ratatui::widgets::Block, text: &str, area: Rect) {
    f.render_widget(Paragraph::new(text).style(Style::default().fg(theme.muted)).block(block), area);
}

/// Noise floor readings as runs of (seconds before now, dBm); a failed poll ends a run
fn noise_runs(noise_history: &[Option<i16>]) -> Vec<Vec<(f64, f64)>> {
    let interval = crate::STATS_POLL_INTERVAL.as_secs_f64();
    let newest = noise_history.len().saturating_sub(1);
    let mut runs = vec![Vec::new()];
    for (i, reading) in noise_history.iter().enumerate() {
        match reading {
            Some(dbm) => runs.last_mut().unwrap().push((-((newest - i) as f64) * interval, *dbm as f64)),
            None => runs.push(Vec::new()),
        }
    }
    runs.retain(|run| !run.is_empty());
    runs
}

fn draw_noise_chart(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if area.height == 0 {
        return;
    }
    let stats = &app.radio_stats;
    let history: Vec<Option<i16>> = stats.noise_history.iter().copied().collect();
    let readings = history.iter().flatten().copied();
    let (Some(latest), Some(min), Some(max)) = (stats.latest, readings.clone().min(), readings.max()) else {
        let block = panel(theme, " Noise floor ", theme.radio_border);
        return waiting(f, theme, block, "waiting for radio stats…", area);
    };

    let title = Line::from(vec![
        Span::raw(" Noise floor "),
        Span::styled(
            format!("{} dBm", latest.noise_floor),
            Style::default().fg(theme.quality(noise_quality(latest.noise_floor))),
        ),
        Span::styled(format!("  (min {} dBm / max {} dBm) ", min, max), Style::default().fg(theme.text)),
    ]);
    let (low, high) = padded_bounds([min as f64, max as f64].into_iter(), 10.0).unwrap();
    let runs = noise_runs(&history);
    let datasets = runs
        .iter()
        .map(|run| {
            Dataset::default()
                .marker(Marker::HalfBlock)
                .graph_type(GraphType::Area)
                .fill_to_y(low)
                .style(Style::default().fg(theme.info))
                .data(run)
        })
        .collect();
    let chart = Chart::new(datasets)
        .block(panel(theme, title, theme.radio_border))
        .x_axis(time_axis(theme))
        .y_axis(value_axis(theme, low, high))
        .legend_position(None);
    f.render_widget(chart, area);
}

fn draw_snr_chart(f: &mut Frame, app: &App, theme: &Theme, now: i64, area: Rect) {
    if area.height == 0 {
        return;
    }
    let signals = &app.radio_stats.packet_signals;
    let Some(last) = signals.back() else {
        let block = panel(theme, " SNR ", theme.radio_border);
        return waiting(f, theme, block, "no packets heard yet", area);
    };

    let title = Line::from(vec![
        Span::raw(" SNR "),
        Span::styled(format!("{:.2} dB ", last.snr), Style::default().fg(theme.quality(snr_quality(last.snr)))),
    ]);
    let points: Vec<(f64, f64)> = signals.iter().map(|p| ((p.at - now) as f64, p.snr as f64)).collect();
    let (low, high) = padded_bounds(points.iter().map(|(_, snr)| *snr), 5.0).unwrap();
    let datasets = vec![Dataset::default()
        .marker(Marker::Dot)
        .graph_type(GraphType::Scatter)
        .style(Style::default().fg(theme.value))
        .data(&points)];
    let chart = Chart::new(datasets)
        .block(panel(theme, title, theme.radio_border))
        .x_axis(time_axis(theme))
        .y_axis(value_axis(theme, low, high))
        .legend_position(None);
    f.render_widget(chart, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::PacketSignal;
    use crate::test_support::*;
    use meshcore_rs::events::{DeviceInfoData, SelfInfo};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// 2026-10-11 12:00:00 UTC
    const NOW: i64 = 1_791_720_000;

    fn render(app: &App, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_radio_tab(f, app, &Theme::CLASSIC, NOW, f.area())).unwrap();
        terminal.backend().clone()
    }

    /// Noise readings rising from -118 to -96 dBm with one failed poll, and
    /// packets heard over the last ten minutes, and a radio with a location
    /// set whose channel once stayed busy too long
    fn app_with_history() -> App {
        let mut app = App::new(0);
        for i in 0..60 {
            let reading = if i == 40 { Err("timeout".to_string()) } else { Ok(radio_stats(-118 + (i * 22 / 59) as i16)) };
            app.radio_stats.record(reading);
        }
        for (ago, snr) in [(600, -9.0), (420, -2.5), (300, 4.0), (120, 7.25), (5, 5.25)] {
            app.radio_stats.record_packet(PacketSignal { at: NOW - ago, snr, rssi: -90 });
        }
        let mut reading = diagnostics_reading();
        reading.core.as_mut().unwrap().errors = 1 << 1;
        app.diagnostics.record(reading);
        app.self_info = Some(SelfInfo {
            tx_power: 20,
            max_tx_power: 22,
            adv_lat: 52_008_900,
            adv_lon: -4_353_100,
            ..Default::default()
        });
        app.device_info = Some(DeviceInfoData {
            version: Some("v1.9.1".to_string()),
            fw_build: Some("2 Oct 2026".to_string()),
            model: Some("Heltec V3".to_string()),
            ..Default::default()
        });
        app
    }

    #[test]
    fn waiting_for_readings() {
        insta::assert_snapshot!(render(&App::new(0), 80, 24));
    }

    #[test]
    fn charts_show_the_history_above_the_panels() {
        insta::assert_snapshot!(render(&app_with_history(), 80, 36));
    }

    #[test]
    fn short_area_leaves_out_the_snr_chart() {
        insta::assert_snapshot!(render(&app_with_history(), 80, 24));
    }

    #[test]
    fn charts_stop_growing_at_their_caps() {
        assert_eq!(chart_heights(4), (0, 0));
        assert_eq!(chart_heights(7), (7, 0));
        assert_eq!(chart_heights(18), (14, 0));
        assert_eq!(chart_heights(21), (14, 7));
        assert_eq!(chart_heights(70), (14, 14));
    }

    #[test]
    fn duty_cycle_needs_uptime() {
        assert_eq!(duty_cycle(12, 0), None);
        assert_eq!(duty_cycle(30, 600), Some(0.05));
    }

    #[test]
    fn duty_bars_fill_in_eighths_and_show_any_airtime() {
        assert_eq!(duty_bar(0.0, 10), "");
        assert_eq!(duty_bar(0.0001, 10), "▏");
        assert_eq!(duty_bar(0.15, 10), "█▌");
        assert_eq!(duty_bar(2.0, 2), "██");
    }

    #[test]
    fn counts_group_thousands() {
        assert_eq!(with_separators(0), "0");
        assert_eq!(with_separators(999), "999");
        assert_eq!(with_separators(1_204), "1,204");
        assert_eq!(with_separators(4_294_967_295), "4,294,967,295");
    }

    #[test]
    fn clock_drift_says_which_way() {
        let drift = |radio: u32| clock_drift(ClockReading { radio, host: NOW });
        assert_eq!(drift(NOW as u32), "in sync");
        assert_eq!(drift(NOW as u32 + 90), "1m30s ahead");
        assert_eq!(drift(NOW as u32 - 5), "5s behind");
    }

    #[test]
    fn location_at_zero_is_unset() {
        assert_eq!(location(0, 0), None);
        assert_eq!(location(52_008_900, -4_353_100), Some("52.0089, -4.3531".to_string()));
    }

    #[test]
    fn error_flags_decode_each_bit() {
        let flags: Vec<_> = error_flags(0b101).collect();
        assert_eq!(flags, [("Packet pool full", true), ("CAD busy timeout", false), ("RX stuck >8s", true)]);
        assert!(error_flags(0).all(|(_, set)| !set));
    }

    #[test]
    fn failed_polls_split_the_noise_floor() {
        let runs = noise_runs(&[Some(-110), None, Some(-104), Some(-100)]);
        assert_eq!(runs, [vec![(-15.0, -110.0)], vec![(-5.0, -104.0), (0.0, -100.0)]]);
    }

    #[test]
    fn bounds_pad_to_the_step() {
        assert_eq!(padded_bounds([-118.0, -96.0].into_iter(), 10.0), Some((-130.0, -80.0)));
        assert_eq!(padded_bounds(std::iter::empty(), 5.0), None);
    }
}
