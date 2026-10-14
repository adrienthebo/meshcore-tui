use ratatui::{
    layout::{Constraint, Rect},
    text::Line,
    widgets::Clear,
    Frame,
};

use crate::theme::Theme;
use crate::ui::panel;

/// A bordered box over the middle of a view, hiding what's beneath it; the
/// caller draws the content inside
pub struct Popover<'a> {
    title: Line<'a>,
    /// Columns inside the border
    width: u16,
    /// Rows inside the border
    height: u16,
}

impl<'a> Popover<'a> {
    /// A popover whose content takes `width` columns and `height` rows; the
    /// box widens to fit the title
    pub fn new(title: impl Into<Line<'a>>, width: u16, height: u16) -> Self {
        let title = title.into();
        let width = width.max(title.width().try_into().unwrap_or(u16::MAX));
        Self {
            title,
            width,
            height,
        }
    }

    /// Draw the box centered in `area`, shrunk to fit it, and return the
    /// space inside its border
    pub fn render(self, f: &mut Frame, area: Rect, theme: &Theme) -> Rect {
        let outer = area.centered(
            Constraint::Length(self.width.saturating_add(2)),
            Constraint::Length(self.height.saturating_add(2)),
        );
        let block = panel(theme, self.title, theme.focus);
        let inner = block.inner(outer);
        f.render_widget(Clear, outer);
        f.render_widget(block, outer);
        inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    /// A popover over a screen of dots, with "body" written inside it
    fn render(popover: Popover, width: u16, height: u16) -> (TestBackend, Rect) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut inner = Rect::default();
        terminal
            .draw(|f| {
                let dots = ".".repeat(width as usize * height as usize);
                f.render_widget(Paragraph::new(dots).wrap(Default::default()), f.area());
                inner = popover.render(f, f.area(), &Theme::CLASSIC);
                f.render_widget(Paragraph::new("body"), inner);
            })
            .unwrap();
        (terminal.backend().clone(), inner)
    }

    #[test]
    fn popover_clears_a_centered_box_and_returns_its_inside() {
        let (backend, inner) = render(Popover::new(" Hi ", 6, 2), 20, 8);
        insta::assert_snapshot!(backend);
        assert_eq!(inner, Rect::new(7, 3, 6, 2));
    }

    #[test]
    fn popover_widens_to_fit_its_title() {
        let (_, inner) = render(Popover::new(" A long title ", 4, 1), 30, 5);
        assert_eq!(inner.width, 14);
    }

    #[test]
    fn popover_shrinks_to_fit_a_small_area() {
        let (_, inner) = render(Popover::new(" Hi ", 40, 10), 20, 6);
        assert_eq!(inner, Rect::new(1, 1, 18, 4));
    }
}
