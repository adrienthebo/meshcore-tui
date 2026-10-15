use crossterm::event::{KeyCode, KeyEvent};
use meshcore_rs::PublicKey;
use ratatui::{layout::Rect, Frame};

use crate::app::App;
use crate::input::InputLine;
use crate::theme::Theme;
use crate::ui::{
    draw_channels, draw_favorites, draw_messages, draw_overview, draw_packets, draw_radio,
};
use crate::update::Action;
use crate::view::{KeyResult, View};

/// The Overview panel that has keyboard focus
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    Input,
    Messages,
    Channels,
    Favorites,
    Packets,
}

/// Chat, channels, favorites, packets and the radio, with the input line for sending
#[derive(Debug, Default)]
pub struct Overview {
    pub focus: Panel,
    pub messages: MessageLog,
    pub channels: ChannelList,
    pub favorites: FavoriteList,
    pub packets: PacketLog,
    pub radio: RadioPanel,
    pub input: InputLine,
}

impl View for Overview {
    fn title(&self) -> &str {
        "Overview"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_overview(f, app, self, theme, area);
    }

    /// The panel with focus gets keys first; Overview takes the ones it ignores:
    /// Tab moves focus between panels. Choosing a channel or favorite moves
    /// focus to the input line, ready to type
    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        let result = self.focused_panel_mut().handle_key(key, app);
        if matches!(self.focus, Panel::Channels | Panel::Favorites)
            && matches!(result, KeyResult::Action(_))
        {
            self.focus = Panel::Input;
        }
        if result != KeyResult::Ignored {
            return result;
        }
        match key.code {
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Panel::Input => Panel::Messages,
                    Panel::Messages => Panel::Channels,
                    Panel::Channels => Panel::Favorites,
                    Panel::Favorites => Panel::Packets,
                    Panel::Packets => Panel::Input,
                };
                KeyResult::Handled
            }
            _ => KeyResult::Ignored,
        }
    }

    fn key_hints(&self) -> Vec<&'static str> {
        let mut hints = self.focused_panel().key_hints();
        hints.push("Tab=panel");
        hints
    }
}

impl Overview {
    fn focused_panel(&self) -> &dyn View {
        match self.focus {
            Panel::Input => &self.input,
            Panel::Messages => &self.messages,
            Panel::Channels => &self.channels,
            Panel::Favorites => &self.favorites,
            Panel::Packets => &self.packets,
        }
    }

    fn focused_panel_mut(&mut self) -> &mut dyn View {
        match self.focus {
            Panel::Input => &mut self.input,
            Panel::Messages => &mut self.messages,
            Panel::Channels => &mut self.channels,
            Panel::Favorites => &mut self.favorites,
            Panel::Packets => &mut self.packets,
        }
    }
}

/// Chat and system messages, newest at the bottom
#[derive(Debug, Default)]
pub struct MessageLog;

impl View for MessageLog {
    fn title(&self) -> &str {
        "Messages"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        draw_messages(f, app, theme, focused, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }
}

/// The position ↑ or ↓ moves to from `current` in a list of `len` entries; the
/// first entry when nothing in the list is selected, None for other keys or an empty list
fn arrow_position(key: KeyCode, current: Option<usize>, len: usize) -> Option<usize> {
    let last = len.checked_sub(1)?;
    match key {
        KeyCode::Up => Some(current.map_or(0, |s| s.saturating_sub(1))),
        KeyCode::Down => Some(current.map_or(0, |s| (s + 1).min(last))),
        _ => None,
    }
}

/// The row ↑↓ have moved to in a list of message targets, kept by the row's id
#[derive(Debug, Default)]
struct TargetCursor<T>(Option<T>);

impl<T: Copy + PartialEq> TargetCursor<T> {
    /// The id under the cursor: where ↑↓ moved it, else the target, if `ids` has it
    fn get(&self, ids: &[T], target: Option<T>) -> Option<T> {
        [self.0, target]
            .into_iter()
            .flatten()
            .find(|id| ids.contains(id))
    }

    /// ↑↓ move the cursor; Enter returns `select` for the id under it
    fn handle_key(
        &mut self,
        key: KeyCode,
        ids: &[T],
        target: Option<T>,
        select: impl FnOnce(T) -> Action,
    ) -> KeyResult {
        let current = self.get(ids, target);
        if key == KeyCode::Enter {
            self.0 = None;
            return current.map_or(KeyResult::Ignored, |id| KeyResult::Action(select(id)));
        }
        let row = current.and_then(|id| ids.iter().position(|&i| i == id));
        match arrow_position(key, row, ids.len()) {
            Some(row) => {
                self.0 = Some(ids[row]);
                KeyResult::Handled
            }
            None => KeyResult::Ignored,
        }
    }
}

/// The radio's channels; ↑↓ and Enter choose which one messages go to
#[derive(Debug, Default)]
pub struct ChannelList {
    cursor: TargetCursor<u8>,
}

impl ChannelList {
    /// The channel index under the cursor
    fn cursor(&self, app: &App) -> Option<u8> {
        self.cursor
            .get(&app.channel_indexes(), app.selected_channel())
    }
}

impl View for ChannelList {
    fn title(&self) -> &str {
        "Channels"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        let cursor = focused.then(|| self.cursor(app)).flatten();
        draw_channels(f, app, cursor, theme, focused, area);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        self.cursor.handle_key(
            key.code,
            &app.channel_indexes(),
            app.selected_channel(),
            Action::SelectChannel,
        )
    }

    fn key_hints(&self) -> Vec<&'static str> {
        vec!["↑↓=move", "Enter=select"]
    }
}

/// The radio's favourite contacts; ↑↓ and Enter choose who messages go to
#[derive(Debug, Default)]
pub struct FavoriteList {
    cursor: TargetCursor<PublicKey>,
}

impl FavoriteList {
    /// The public key of the favorite under the cursor
    fn cursor(&self, app: &App) -> Option<PublicKey> {
        self.cursor.get(&favorite_keys(app), app.selected_key)
    }
}

/// The public keys of the favourite contacts, in list order
fn favorite_keys(app: &App) -> Vec<PublicKey> {
    app.favorites().iter().map(|c| c.public_key).collect()
}

impl View for FavoriteList {
    fn title(&self) -> &str {
        "Favorites"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        let cursor = focused.then(|| self.cursor(app)).flatten();
        draw_favorites(f, app, cursor, theme, focused, area);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> KeyResult {
        self.cursor.handle_key(
            key.code,
            &favorite_keys(app),
            app.selected_key,
            Action::SelectContact,
        )
    }

    fn key_hints(&self) -> Vec<&'static str> {
        vec!["↑↓=move", "Enter=select"]
    }
}

/// Radio events: adverts, ACKs, route changes and sent DMs, newest at the bottom
#[derive(Debug, Default)]
pub struct PacketLog;

impl View for PacketLog {
    fn title(&self) -> &str {
        "Activity"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, focused: bool) {
        draw_packets(f, app, theme, focused, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }
}

/// The noise floor graph and latest signal readings
#[derive(Debug, Default)]
pub struct RadioPanel;

impl View for RadioPanel {
    fn title(&self) -> &str {
        "Radio"
    }

    fn draw(&self, f: &mut Frame, area: Rect, app: &App, theme: &Theme, _focused: bool) {
        draw_radio(f, app, theme, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &App) -> KeyResult {
        KeyResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crossterm::event::KeyModifiers;

    fn press(view: &mut impl View, app: &App, code: KeyCode) -> KeyResult {
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE), app)
    }

    // ── Overview ─────────────────────────────────────────────────────────────

    #[test]
    fn typing_reaches_the_input_line_only_while_it_has_focus() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        assert_eq!(overview.focus, Panel::Input);
        press(&mut overview, &app, KeyCode::Char('a'));
        overview.focus = Panel::Favorites;
        assert_eq!(
            press(&mut overview, &app, KeyCode::Char('b')),
            KeyResult::Ignored
        );
        assert_eq!(overview.input.as_str(), "a");
    }

    #[test]
    fn keys_the_focused_panel_ignores_reach_the_overview() {
        let (mut overview, app) = (Overview::default(), app_with_contacts(1));
        // The input line ignores Tab
        assert_eq!(press(&mut overview, &app, KeyCode::Tab), KeyResult::Handled);
        assert_eq!(overview.focus, Panel::Messages);
    }

    #[test]
    fn enter_submits_the_input_line() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        press(&mut overview, &app, KeyCode::Char('x'));
        assert_eq!(
            press(&mut overview, &app, KeyCode::Enter),
            KeyResult::Action(Action::Submit("x".to_string()))
        );
    }

    #[test]
    fn tab_cycles_panels() {
        let (mut overview, app) = (Overview::default(), App::new(0));
        let mut order = Vec::new();
        for _ in 0..5 {
            press(&mut overview, &app, KeyCode::Tab);
            order.push(overview.focus.clone());
        }
        assert_eq!(
            order,
            [
                Panel::Messages,
                Panel::Channels,
                Panel::Favorites,
                Panel::Packets,
                Panel::Input
            ]
        );
    }

    #[test]
    fn arrows_reach_the_favorite_list_only_while_it_has_focus() {
        let (mut overview, app) = (Overview::default(), app_with_favorites(3));
        assert_eq!(
            press(&mut overview, &app, KeyCode::Down),
            KeyResult::Ignored
        );
        overview.focus = Panel::Favorites;
        press(&mut overview, &app, KeyCode::Down);
        assert_eq!(
            press(&mut overview, &app, KeyCode::Enter),
            select_contact(&app, 0)
        );
    }

    #[test]
    fn choosing_a_channel_or_favorite_moves_focus_to_the_input_line() {
        let (mut overview, mut app) = (Overview::default(), app_with_favorites(2));
        app.channels.insert(0, channel("Public"));
        for panel in [Panel::Channels, Panel::Favorites] {
            overview.focus = panel.clone();
            press(&mut overview, &app, KeyCode::Down);
            assert_eq!(overview.focus, panel);
            press(&mut overview, &app, KeyCode::Enter);
            assert_eq!(overview.focus, Panel::Input);
        }
    }

    #[test]
    fn key_hints_follow_focus() {
        let mut overview = Overview::default();
        assert_eq!(overview.key_hints(), ["Enter=send", "Tab=panel"]);
        overview.focus = Panel::Channels;
        assert_eq!(
            overview.key_hints(),
            ["↑↓=move", "Enter=select", "Tab=panel"]
        );
        overview.focus = Panel::Favorites;
        assert_eq!(
            overview.key_hints(),
            ["↑↓=move", "Enter=select", "Tab=panel"]
        );
        overview.focus = Panel::Packets;
        assert_eq!(overview.key_hints(), ["Tab=panel"]);
    }

    // ── arrow_position ───────────────────────────────────────────────────────

    #[test]
    fn arrows_move_within_the_list() {
        assert_eq!(arrow_position(KeyCode::Down, Some(1), 3), Some(2));
        assert_eq!(arrow_position(KeyCode::Down, Some(2), 3), Some(2));
        assert_eq!(arrow_position(KeyCode::Up, Some(1), 3), Some(0));
        assert_eq!(arrow_position(KeyCode::Up, Some(0), 3), Some(0));
    }

    #[test]
    fn arrows_with_nothing_selected_move_to_the_first() {
        assert_eq!(arrow_position(KeyCode::Up, None, 2), Some(0));
        assert_eq!(arrow_position(KeyCode::Down, None, 2), Some(0));
    }

    #[test]
    fn arrows_in_an_empty_list_go_nowhere() {
        assert_eq!(arrow_position(KeyCode::Down, None, 0), None);
    }

    #[test]
    fn other_keys_go_nowhere() {
        assert_eq!(arrow_position(KeyCode::Char('a'), Some(0), 2), None);
    }

    // ── ChannelList ──────────────────────────────────────────────────────────

    fn select_channel(ch: u8) -> KeyResult {
        KeyResult::Action(Action::SelectChannel(ch))
    }

    /// Channels at indexes 0, 1 and 3
    fn app_with_channels() -> App {
        let mut app = app_with_contacts(1);
        for (idx, name) in [(0, "Public"), (1, "Ops"), (3, "Club")] {
            app.channels.insert(idx, channel(name));
        }
        app
    }

    /// Press each of `keys`, then Enter, and return what Enter did
    fn choose(view: &mut impl View, app: &App, keys: &[KeyCode]) -> KeyResult {
        for &key in keys {
            press(view, app, key);
        }
        press(view, app, KeyCode::Enter)
    }

    #[test]
    fn channel_arrows_move_the_cursor_without_selecting() {
        let mut app = app_with_channels();
        app.active_channel = 1;
        let mut list = ChannelList::default();
        assert_eq!(press(&mut list, &app, KeyCode::Down), KeyResult::Handled);
        assert_eq!(list.cursor(&app), Some(3));
        assert_eq!(app.selected_channel(), Some(1));
    }

    #[test]
    fn channel_enter_selects_the_channel_under_the_cursor() {
        let mut app = app_with_channels();
        app.active_channel = 1;
        let mut list = ChannelList::default();
        assert_eq!(choose(&mut list, &app, &[KeyCode::Down]), select_channel(3));
        assert_eq!(
            choose(&mut list, &app, &[KeyCode::Up, KeyCode::Up]),
            select_channel(0)
        );
    }

    #[test]
    fn channel_enter_without_arrows_selects_the_active_channel() {
        let mut app = app_with_channels();
        app.active_channel = 3;
        assert_eq!(
            choose(&mut ChannelList::default(), &app, &[]),
            select_channel(3)
        );
    }

    #[test]
    fn channel_arrows_while_a_contact_is_selected_start_at_the_first_channel() {
        let mut app = app_with_channels();
        app.active_channel = 3;
        app.select_contact(app.contacts[0].public_key);
        let mut list = ChannelList::default();
        assert_eq!(choose(&mut list, &app, &[]), KeyResult::Ignored);
        assert_eq!(choose(&mut list, &app, &[KeyCode::Down]), select_channel(0));
    }

    #[test]
    fn channel_cursor_on_a_removed_channel_falls_back_to_the_active_channel() {
        let mut app = app_with_channels();
        let mut list = ChannelList::default();
        press(&mut list, &app, KeyCode::Down);
        assert_eq!(list.cursor(&app), Some(1));
        app.channels.remove(&1);
        assert_eq!(list.cursor(&app), Some(0));
    }

    #[test]
    fn channel_keys_with_no_channels_select_nothing() {
        let mut list = ChannelList::default();
        let app = App::new(0);
        assert_eq!(press(&mut list, &app, KeyCode::Down), KeyResult::Ignored);
        assert_eq!(press(&mut list, &app, KeyCode::Enter), KeyResult::Ignored);
    }

    // ── FavoriteList ─────────────────────────────────────────────────────────

    /// The action that selects the contact at `idx` in the contact list
    fn select_contact(app: &App, idx: usize) -> KeyResult {
        KeyResult::Action(Action::SelectContact(app.contacts[idx].public_key))
    }

    /// `n` contacts, every other one a favourite
    fn app_with_favorites(n: u8) -> App {
        let mut app = app_with_contacts(n * 2);
        for contact in app.contacts.iter_mut().step_by(2) {
            contact.flags = 0x01;
        }
        app
    }

    #[test]
    fn favorite_arrows_move_the_cursor_without_selecting() {
        let mut app = app_with_favorites(3);
        app.select_contact(app.contacts[2].public_key);
        let mut list = FavoriteList::default();
        assert_eq!(press(&mut list, &app, KeyCode::Down), KeyResult::Handled);
        assert_eq!(list.cursor(&app), Some(app.contacts[4].public_key));
        assert_eq!(app.selected_key, Some(app.contacts[2].public_key));
    }

    #[test]
    fn favorite_enter_skips_contacts_that_are_not_favorites() {
        let mut app = app_with_favorites(3);
        app.select_contact(app.contacts[2].public_key);
        let mut list = FavoriteList::default();
        assert_eq!(
            choose(&mut list, &app, &[KeyCode::Down]),
            select_contact(&app, 4)
        );
        assert_eq!(
            choose(&mut list, &app, &[KeyCode::Up]),
            select_contact(&app, 0)
        );
    }

    #[test]
    fn favorite_arrows_from_a_contact_that_is_not_a_favorite_start_at_the_first() {
        let mut app = app_with_favorites(2);
        app.select_contact(app.contacts[3].public_key);
        assert_eq!(
            choose(&mut FavoriteList::default(), &app, &[KeyCode::Down]),
            select_contact(&app, 0)
        );
    }

    #[test]
    fn favorite_keys_with_no_favorites_select_nothing() {
        let mut list = FavoriteList::default();
        let app = app_with_contacts(2);
        assert_eq!(press(&mut list, &app, KeyCode::Down), KeyResult::Ignored);
        assert_eq!(press(&mut list, &app, KeyCode::Enter), KeyResult::Ignored);
    }
}
