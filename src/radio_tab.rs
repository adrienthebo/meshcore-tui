use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

use crate::app::App;
use crate::theme::Theme;
use crate::ui::panel;
use crate::view::{KeyResult, View};

/// Radio diagnostics: the noise floor over time above the radio's signal,
/// airtime, packet, device, error and settings readings
#[derive(Debug, Default)]
pub struct RadioTab;

impl View for RadioTab {
    fn title(&self) -> &str {
        "Radio"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_radio_tab(f, app, theme, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }
}

/// Three panels side by side, equal width
fn three_columns(area: Rect) -> [Rect; 3] {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1); 3])
        .areas(area)
}

fn draw_radio_tab(f: &mut Frame, _app: &App, theme: &Theme, area: Rect) {
    let [noise, readings, device] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(6), Constraint::Length(6)])
        .areas(area);
    f.render_widget(panel(theme, " Noise floor ", theme.radio_border), noise);

    let [signal, airtime, packets] = three_columns(readings);
    f.render_widget(panel(theme, " Signal ", theme.radio_border), signal);
    f.render_widget(panel(theme, " Airtime ", theme.radio_border), airtime);
    f.render_widget(panel(theme, " Packets ", theme.radio_border), packets);

    let [device, errors, settings] = three_columns(device);
    f.render_widget(panel(theme, " Device ", theme.radio_border), device);
    f.render_widget(panel(theme, " Errors (since boot) ", theme.radio_border), errors);
    f.render_widget(panel(theme, " Settings ", theme.radio_border), settings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(app: &App, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_radio_tab(f, app, &Theme::CLASSIC, f.area())).unwrap();
        terminal.backend().clone()
    }

    #[test]
    fn panels_lay_out_below_the_noise_floor() {
        insta::assert_snapshot!(render(&App::new(0), 80, 19));
    }
}
