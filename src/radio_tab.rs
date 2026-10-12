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
use crate::stats::HISTORY_SECS;
use crate::theme::Theme;
use crate::ui::{noise_quality, panel, snr_quality};
use crate::view::{KeyResult, View};

/// Rows for the two rows of reading panels under the charts
const PANEL_ROWS: u16 = 12;
/// Tallest the noise floor chart gets; past this a taller chart shows nothing more
const NOISE_CHART_MAX: u16 = 16;
/// Tallest the SNR chart gets
const SNR_CHART_MAX: u16 = 12;
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
}

/// Heights of the noise floor and SNR charts in `rows`; the SNR chart gives up
/// its rows first, and a chart too short to read is left out
fn chart_heights(rows: u16) -> (u16, u16) {
    let noise = rows.min(NOISE_CHART_MAX);
    let snr = (rows - noise).min(SNR_CHART_MAX);
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
    f.render_widget(panel(theme, " Signal ", theme.radio_border), signal);
    f.render_widget(panel(theme, " Airtime ", theme.radio_border), airtime);
    f.render_widget(panel(theme, " Packets ", theme.radio_border), packets);

    let [device, errors, settings] = three_columns(device);
    f.render_widget(panel(theme, " Device ", theme.radio_border), device);
    f.render_widget(panel(theme, " Errors (since boot) ", theme.radio_border), errors);
    f.render_widget(panel(theme, " Settings ", theme.radio_border), settings);
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
        Span::styled(format!("  (min {} / max {}) ", min, max), Style::default().fg(theme.text)),
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
    /// packets heard over the last ten minutes
    fn app_with_history() -> App {
        let mut app = App::new(0);
        for i in 0..60 {
            let reading = if i == 40 { Err("timeout".to_string()) } else { Ok(radio_stats(-118 + (i * 22 / 59) as i16)) };
            app.radio_stats.record(reading);
        }
        for (ago, snr) in [(600, -9.0), (420, -2.5), (300, 4.0), (120, 7.25), (5, 5.25)] {
            app.radio_stats.record_packet(PacketSignal { at: NOW - ago, snr, rssi: -90 });
        }
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
        assert_eq!(chart_heights(20), (16, 0));
        assert_eq!(chart_heights(23), (16, 7));
        assert_eq!(chart_heights(70), (16, 12));
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
