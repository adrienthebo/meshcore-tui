use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

/// Numbered tab titles on a rule, two rows tall. The selected tab is outlined and
/// opens onto the rule, so it reads as owning what's drawn below the bar.
pub struct TabBar<'a> {
    titles: &'a [&'a str],
    selected: usize,
    active: Style,
    rule: Style,
}

impl<'a> TabBar<'a> {
    pub fn new(titles: &'a [&'a str], selected: usize) -> Self {
        Self {
            titles,
            selected,
            active: Style::default(),
            rule: Style::default(),
        }
    }

    /// Style of the selected tab's outline and title; the title is also bold
    pub fn active_style(mut self, style: Style) -> Self {
        self.active = style;
        self
    }

    /// Style of the rule and the other tabs' titles
    pub fn rule_style(mut self, style: Style) -> Self {
        self.rule = style;
        self
    }

    /// Columns the titles take, from the left edge to the rule after the last one
    pub fn titles_width(&self) -> usize {
        1 + self
            .titles
            .iter()
            .enumerate()
            .map(|(i, title)| format!(" {} {} ", i + 1, title).width() + 3)
            .sum::<usize>()
    }
}

impl Widget for TabBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut top = vec![Span::raw(" ")];
        let mut bottom = vec![Span::styled("─", self.rule)];
        for (i, title) in self.titles.iter().enumerate() {
            let label = format!(" {} {} ", i + 1, title);
            let width = label.width();
            if i == self.selected {
                top.push(Span::styled(format!("╭{}╮", "─".repeat(width)), self.active));
                bottom.push(Span::styled("╯", self.active));
                bottom.push(Span::styled(label, self.active.add_modifier(Modifier::BOLD)));
                bottom.push(Span::styled("╰", self.active));
            } else {
                top.push(Span::raw(" ".repeat(width + 2)));
                bottom.push(Span::styled(format!("─{}─", label), self.rule));
            }
            top.push(Span::raw(" "));
            bottom.push(Span::styled("─", self.rule));
        }
        let used: usize = bottom.iter().map(|span| span.width()).sum();
        bottom.push(Span::styled("─".repeat((area.width as usize).saturating_sub(used)), self.rule));

        Paragraph::new(vec![Line::from(top), Line::from(bottom)]).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    const TITLES: [&str; 4] = ["Overview", "Contacts", "Radio", "Raw"];

    fn render(bar: TabBar, width: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 2));
        bar.render(buf.area, &mut buf);
        buf
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn selected_tab_opens_onto_the_rule() {
        let buf = render(TabBar::new(&TITLES, 1), 60);
        assert_eq!(
            rows(&buf),
            [
                "                ╭────────────╮                              ",
                "── 1 Overview ──╯ 2 Contacts ╰── 3 Radio ─── 4 Raw ─────────",
            ]
        );
    }

    #[test]
    fn first_tab_selected() {
        let buf = render(TabBar::new(&TITLES, 0), 60);
        assert_eq!(
            rows(&buf),
            [
                " ╭────────────╮                                             ",
                "─╯ 1 Overview ╰── 2 Contacts ─── 3 Radio ─── 4 Raw ─────────",
            ]
        );
    }

    #[test]
    fn selected_tab_is_bold_in_the_active_style_and_the_rest_in_the_rule_style() {
        let active = Style::default().fg(Color::Magenta);
        let rule = Style::default().fg(Color::DarkGray);
        let bar = TabBar::new(&TITLES, 1).active_style(active).rule_style(rule);
        let buf = render(bar, 60);

        // Outline corner, then the selected title
        assert_eq!(buf[(16, 0)].fg, Color::Magenta);
        assert_eq!(buf[(16, 1)].fg, Color::Magenta);
        assert_eq!(buf[(20, 1)].fg, Color::Magenta);
        assert!(buf[(20, 1)].modifier.contains(Modifier::BOLD));
        // An unselected title, then the rule past the last tab
        assert_eq!(buf[(5, 1)].fg, Color::DarkGray);
        assert!(!buf[(5, 1)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(59, 1)].fg, Color::DarkGray);
    }

    #[test]
    fn titles_width_ends_after_the_last_titles_rule() {
        let bar = TabBar::new(&TITLES, 1);
        let width = bar.titles_width();
        let row = &rows(&render(bar, 60))[1];
        assert_eq!(row.chars().take(width).collect::<String>(), "── 1 Overview ──╯ 2 Contacts ╰── 3 Radio ─── 4 Raw ──");
    }

    #[test]
    fn narrow_area_cuts_the_bar_off() {
        let buf = render(TabBar::new(&TITLES, 1), 20);
        assert_eq!(rows(&buf), ["                ╭───", "── 1 Overview ──╯ 2 "]);
    }
}
