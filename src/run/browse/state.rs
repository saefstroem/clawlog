use std::mem;
use std::path::PathBuf;

use serde_json::{to_string_pretty, Value};

use super::catalog::Conversation;
use crate::run::adapter::ADAPTERS;

const PAGE: isize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Conversations,
    Entries,
    Detail,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pending {
    Uninstall,
    Delete,
}

#[derive(Debug, PartialEq)]
pub enum Mode {
    Normal,
    Input(String),
    Confirm(String, Pending),
    Select { cursor: usize, chosen: Vec<bool> },
}

#[derive(Debug, PartialEq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Enter,
    Esc,
    Backspace,
    Char(char),
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Open,
    Install(Vec<&'static str>),
    Uninstall,
    Export(PathBuf),
    AskClear,
    Delete,
}

pub struct State {
    pub dir: PathBuf,
    pub conversations: Vec<Conversation>,
    pub cursor: usize,
    pub level: Level,
    pub entries: Vec<Value>,
    pub entry_cursor: usize,
    pub detail_source: Vec<String>,
    pub detail: Vec<String>,
    pub scroll: usize,
    pub width: u16,
    pub mode: Mode,
    pub status: String,
    pub now_ms: u64,
    pub done: bool,
}

impl State {
    pub fn new(dir: PathBuf, conversations: Vec<Conversation>) -> State {
        State {
            dir,
            conversations,
            cursor: 0,
            level: Level::Conversations,
            entries: Vec::new(),
            entry_cursor: 0,
            detail_source: Vec::new(),
            detail: Vec::new(),
            scroll: 0,
            width: 80,
            mode: Mode::Normal,
            status: String::new(),
            now_ms: 0,
            done: false,
        }
    }

    pub fn apply(&mut self, key: Key) -> Option<Command> {
        match (&mut self.mode, key) {
            (Mode::Input(buffer), Key::Char(c)) => buffer.push(c),
            (Mode::Input(buffer), Key::Backspace) => {
                buffer.pop();
            }
            (Mode::Input(buffer), Key::Enter) => {
                let target = mem::take(buffer);
                self.mode = Mode::Normal;
                if !target.is_empty() {
                    return Some(Command::Export(PathBuf::from(target)));
                }
            }
            (Mode::Input(_), Key::Esc) => self.mode = Mode::Normal,
            (Mode::Input(_), _) => {}
            (Mode::Confirm(_, pending), Key::Char('y' | 'Y')) => {
                let command = match *pending {
                    Pending::Uninstall => Command::Uninstall,
                    Pending::Delete => Command::Delete,
                };
                self.mode = Mode::Normal;
                return Some(command);
            }
            (Mode::Confirm(..), _) => {
                self.mode = Mode::Normal;
                self.status = "aborted".to_owned();
            }
            (Mode::Select { cursor, .. }, Key::Up | Key::Char('k')) => {
                *cursor = cursor.saturating_sub(1);
            }
            (Mode::Select { cursor, .. }, Key::Down | Key::Char('j')) => {
                *cursor = (*cursor + 1).min(ADAPTERS.len().saturating_sub(1));
            }
            (Mode::Select { cursor, chosen }, Key::Char(' ')) => {
                if let Some(flag) = chosen.get_mut(*cursor) {
                    *flag = !*flag;
                }
            }
            (Mode::Select { chosen, .. }, Key::Enter) => {
                let names: Vec<&'static str> = ADAPTERS
                    .iter()
                    .zip(chosen.iter())
                    .filter(|(_, chosen)| **chosen)
                    .map(|(adapter, _)| adapter.name())
                    .collect();
                self.mode = Mode::Normal;
                if names.is_empty() {
                    self.status = "nothing selected".to_owned();
                } else {
                    return Some(Command::Install(names));
                }
            }
            (Mode::Select { .. }, Key::Esc) => {
                self.mode = Mode::Normal;
                self.status = "aborted".to_owned();
            }
            (Mode::Select { .. }, _) => {}
            (Mode::Normal, key) => return self.navigate(key),
        }
        None
    }

    fn navigate(&mut self, key: Key) -> Option<Command> {
        match key {
            Key::Char('q') => self.done = true,
            Key::Up | Key::Char('k') => self.move_by(-1),
            Key::Down | Key::Char('j') => self.move_by(1),
            Key::PageUp => self.move_by(-PAGE),
            Key::PageDown => self.move_by(PAGE),
            Key::Esc => self.back(),
            Key::Enter => return self.enter(),
            Key::Char('i') if self.level == Level::Conversations => self.offer_install(),
            Key::Char('u') if self.level == Level::Conversations => self.offer_uninstall(),
            Key::Char('e') if self.level == Level::Conversations => {
                self.mode = Mode::Input(String::new());
            }
            Key::Char('c') if self.level == Level::Conversations => return Some(Command::AskClear),
            _ => {}
        }
        None
    }

    fn enter(&mut self) -> Option<Command> {
        match self.level {
            Level::Conversations if !self.conversations.is_empty() => return Some(Command::Open),
            Level::Entries => {
                if let Some(entry) = self.entries.get(self.entry_cursor) {
                    self.detail_source = to_string_pretty(entry)
                        .unwrap_or_default()
                        .lines()
                        .map(str::to_owned)
                        .collect();
                    self.scroll = 0;
                    self.rewrap();
                    self.level = Level::Detail;
                }
            }
            _ => {}
        }
        None
    }

    fn back(&mut self) {
        match self.level {
            Level::Detail => self.level = Level::Entries,
            Level::Entries => self.level = Level::Conversations,
            Level::Conversations => self.done = true,
        }
    }

    fn move_by(&mut self, delta: isize) {
        let (cursor, len) = match self.level {
            Level::Conversations => (&mut self.cursor, self.conversations.len()),
            Level::Entries => (&mut self.entry_cursor, self.entries.len()),
            Level::Detail => (&mut self.scroll, self.detail.len()),
        };
        *cursor = cursor
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }

    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    pub fn viewport(&mut self, width: u16) {
        if width != self.width {
            self.width = width;
            self.rewrap();
        }
    }

    fn rewrap(&mut self) {
        let width = usize::from(self.width.max(1));
        self.detail = self
            .detail_source
            .iter()
            .flat_map(|line| {
                let chunks: Vec<String> = line
                    .chars()
                    .collect::<Vec<char>>()
                    .chunks(width)
                    .map(|chunk| chunk.iter().collect())
                    .collect();
                if chunks.is_empty() {
                    vec![String::new()]
                } else {
                    chunks
                }
            })
            .collect();
        self.scroll = self.scroll.min(self.detail.len().saturating_sub(1));
    }

    pub fn conversation(&self) -> Option<&Conversation> {
        self.conversations.get(self.cursor)
    }

    pub fn open(&mut self, entries: Vec<Value>) {
        self.entries = entries;
        self.entry_cursor = 0;
        self.level = Level::Entries;
    }

    pub fn offer_install(&mut self) {
        self.mode = Mode::Select {
            cursor: 0,
            chosen: vec![true; ADAPTERS.len()],
        };
    }

    pub fn offer_uninstall(&mut self) {
        self.mode = Mode::Confirm(
            "remove clawlog hooks from ~/.claude/settings.json? [y/N]".to_owned(),
            Pending::Uninstall,
        );
    }

    pub fn offer_clear(&mut self, count: usize) {
        if count == 0 {
            self.status = "nothing to delete".to_owned();
        } else {
            self.mode = Mode::Confirm(
                format!(
                    "delete {count} log file(s) under {}? [y/N]",
                    self.dir.display()
                ),
                Pending::Delete,
            );
        }
    }

    pub fn reload(&mut self, conversations: Vec<Conversation>) {
        self.conversations = conversations;
        self.cursor = self.cursor.min(self.conversations.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{Command, Key, Level, Mode, Pending, State};
    use crate::run::browse::catalog::Conversation;

    fn conversation(id: &str) -> Conversation {
        Conversation {
            adapter: "claude".to_owned(),
            id: id.to_owned(),
            parts: vec![PathBuf::from(format!(
                "/logs/claude/clawlog_claude_{id}_0001.part.json"
            ))],
            bytes: 10,
            last_ms: 0,
        }
    }

    fn browsing(count: usize) -> State {
        let conversations = (0..count).map(|n| conversation(&format!("s{n}"))).collect();
        State::new(PathBuf::from("/logs"), conversations)
    }

    fn opened(count: usize) -> State {
        let mut state = browsing(1);
        state.open((0..count).map(|n| json!({"n": n})).collect());
        state
    }

    #[test]
    fn starts_on_the_conversations_and_esc_quits_from_them() {
        let mut state = State::new(PathBuf::from("/logs"), Vec::new());
        assert_eq!(state.level, Level::Conversations);
        state.apply(Key::Esc);
        assert!(state.done);
    }

    #[test]
    fn install_opens_a_selection_that_toggles_installs_and_cancels() {
        let mut state = State::new(PathBuf::from("/logs"), Vec::new());
        state.offer_install();
        assert_eq!(
            state.mode,
            Mode::Select {
                cursor: 0,
                chosen: vec![true]
            }
        );
        state.apply(Key::Char(' '));
        assert_eq!(
            state.mode,
            Mode::Select {
                cursor: 0,
                chosen: vec![false]
            }
        );
        assert_eq!(state.apply(Key::Enter), None);
        assert_eq!(state.status, "nothing selected");
        assert_eq!(state.mode, Mode::Normal);
        state.offer_install();
        state.apply(Key::Down);
        state.apply(Key::Up);
        assert_eq!(
            state.apply(Key::Enter),
            Some(Command::Install(vec!["claude"]))
        );
        assert_eq!(state.mode, Mode::Normal);
        state.offer_install();
        assert_eq!(state.apply(Key::Esc), None);
        assert_eq!(state.status, "aborted");
        assert_eq!(state.mode, Mode::Normal);
    }

    #[test]
    fn uninstall_confirmation_aborts_on_anything_else() {
        let mut state = State::new(PathBuf::from("/logs"), Vec::new());
        state.offer_uninstall();
        assert_eq!(state.apply(Key::Esc), None);
        assert_eq!(state.status, "aborted");
        assert_eq!(state.mode, Mode::Normal);
        assert!(!state.done);
    }

    #[test]
    fn empty_catalog_never_opens() {
        let mut state = browsing(0);
        assert_eq!(state.apply(Key::Enter), None);
        assert_eq!(state.level, Level::Conversations);
        state.apply(Key::Down);
        state.apply(Key::Up);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn selection_moves_and_clamps() {
        let mut state = browsing(3);
        state.apply(Key::Down);
        state.apply(Key::Char('j'));
        assert_eq!(state.cursor, 2);
        state.apply(Key::Down);
        assert_eq!(state.cursor, 2);
        state.apply(Key::Char('k'));
        state.apply(Key::Up);
        state.apply(Key::Up);
        assert_eq!(state.cursor, 0);
        state.apply(Key::PageDown);
        assert_eq!(state.cursor, 2);
        state.apply(Key::PageUp);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn enter_descends_and_esc_backs_out_level_by_level() {
        let mut state = opened(2);
        assert_eq!(state.level, Level::Entries);
        assert_eq!(state.apply(Key::Enter), None);
        assert_eq!(state.level, Level::Detail);
        assert_eq!(state.detail.first().map(String::as_str), Some("{"));
        state.apply(Key::Esc);
        assert_eq!(state.level, Level::Entries);
        state.apply(Key::Esc);
        assert_eq!(state.level, Level::Conversations);
        assert!(!state.done);
        state.apply(Key::Esc);
        assert!(state.done);
    }

    #[test]
    fn q_quits_from_every_level() {
        let builds: [fn() -> State; 3] = [
            || browsing(1),
            || opened(1),
            || {
                let mut state = opened(1);
                state.apply(Key::Enter);
                state
            },
        ];
        for build in builds {
            let mut state = build();
            state.apply(Key::Char('q'));
            assert!(state.done);
        }
    }

    #[test]
    fn conversations_enter_requests_open_and_open_lands_on_entries() {
        let mut state = browsing(2);
        assert_eq!(state.apply(Key::Enter), Some(Command::Open));
        state.entry_cursor = 5;
        state.open(vec![json!(1)]);
        assert_eq!(state.level, Level::Entries);
        assert_eq!(state.entry_cursor, 0);
    }

    #[test]
    fn detail_scroll_clamps_to_line_count() {
        let mut state = opened(1);
        state.apply(Key::Enter);
        let lines = state.detail.len();
        assert_eq!(lines, 3);
        state.apply(Key::Down);
        state.apply(Key::Down);
        state.apply(Key::Down);
        assert_eq!(state.scroll, lines - 1);
        state.apply(Key::PageDown);
        assert_eq!(state.scroll, lines - 1);
        state.apply(Key::Up);
        state.apply(Key::PageUp);
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn detail_wraps_long_lines_to_the_viewport_width() {
        let mut state = browsing(1);
        state.open(vec![json!({"output": "x".repeat(100)})]);
        state.width = 40;
        state.apply(Key::Enter);
        assert!(state.detail.len() > state.detail_source.len());
        assert!(state.detail.iter().all(|line| line.chars().count() <= 40));
        let rejoined: String = state.detail.concat();
        assert!(rejoined.contains(&"x".repeat(100)));
    }

    #[test]
    fn viewport_change_rewraps_and_clamps_the_scroll() {
        let mut state = browsing(1);
        state.open(vec![json!({"output": "x".repeat(300)})]);
        state.width = 20;
        state.apply(Key::Enter);
        let narrow = state.detail.len();
        state.apply(Key::PageDown);
        state.apply(Key::PageDown);
        let scrolled = state.scroll;
        assert!(scrolled > 0);
        state.viewport(120);
        assert!(state.detail.len() < narrow);
        assert!(state.scroll < state.detail.len());
        state.viewport(120);
        assert!(state.detail.iter().all(|line| line.chars().count() <= 120));
    }

    #[test]
    fn empty_entries_enter_stays_put() {
        let mut state = opened(0);
        assert_eq!(state.apply(Key::Enter), None);
        assert_eq!(state.level, Level::Entries);
    }

    #[test]
    fn management_keys_confirm_and_fire_only_on_conversations_level() {
        let mut state = browsing(1);
        assert_eq!(state.apply(Key::Char('i')), None);
        assert!(matches!(state.mode, Mode::Select { .. }));
        assert_eq!(
            state.apply(Key::Enter),
            Some(Command::Install(vec!["claude"]))
        );
        assert_eq!(state.apply(Key::Char('u')), None);
        assert!(matches!(state.mode, Mode::Confirm(_, Pending::Uninstall)));
        assert_eq!(state.apply(Key::Char('y')), Some(Command::Uninstall));
        assert_eq!(state.apply(Key::Char('c')), Some(Command::AskClear));
        let mut entries = opened(1);
        for key in ['i', 'u', 'c', 'e'] {
            assert_eq!(entries.apply(Key::Char(key)), None);
        }
        assert_eq!(entries.mode, Mode::Normal);
    }

    #[test]
    fn export_input_edits_submits_and_cancels() {
        let mut state = browsing(1);
        state.apply(Key::Char('e'));
        assert_eq!(state.mode, Mode::Input(String::new()));
        for c in "/tmq".chars() {
            state.apply(Key::Char(c));
        }
        assert!(!state.done);
        state.apply(Key::Backspace);
        state.apply(Key::Char('p'));
        state.apply(Key::Up);
        assert_eq!(state.mode, Mode::Input("/tmp".to_owned()));
        assert_eq!(
            state.apply(Key::Enter),
            Some(Command::Export(PathBuf::from("/tmp")))
        );
        assert_eq!(state.mode, Mode::Normal);
        state.apply(Key::Char('e'));
        state.apply(Key::Char('x'));
        state.apply(Key::Esc);
        assert_eq!(state.mode, Mode::Normal);
        state.apply(Key::Char('e'));
        assert_eq!(state.apply(Key::Enter), None);
        assert_eq!(state.mode, Mode::Normal);
        state.apply(Key::Backspace);
        assert_eq!(state.mode, Mode::Normal);
    }

    #[test]
    fn clear_confirms_on_y_and_aborts_on_anything_else() {
        let mut state = browsing(1);
        state.offer_clear(0);
        assert_eq!(state.status, "nothing to delete");
        assert_eq!(state.mode, Mode::Normal);
        state.offer_clear(3);
        assert_eq!(
            state.mode,
            Mode::Confirm(
                "delete 3 log file(s) under /logs? [y/N]".to_owned(),
                Pending::Delete
            )
        );
        assert_eq!(state.apply(Key::Char('y')), Some(Command::Delete));
        assert_eq!(state.mode, Mode::Normal);
        state.offer_clear(1);
        assert_eq!(state.apply(Key::Char('Y')), Some(Command::Delete));
        for key in [Key::Char('n'), Key::Char('q'), Key::Up, Key::Enter] {
            state.status.clear();
            state.offer_clear(1);
            assert_eq!(state.apply(key), None);
            assert_eq!(state.mode, Mode::Normal);
            assert_eq!(state.status, "aborted");
            assert!(!state.done);
        }
    }

    #[test]
    fn reload_clamps_the_cursor() {
        let mut state = browsing(3);
        state.apply(Key::PageDown);
        assert_eq!(state.cursor, 2);
        state.reload(vec![conversation("s0")]);
        assert_eq!(state.cursor, 0);
        state.reload(Vec::new());
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn conversation_returns_the_selected_row() {
        let mut state = browsing(2);
        state.apply(Key::Down);
        assert_eq!(
            state
                .conversation()
                .map(|conversation| conversation.id.clone()),
            Some("s1".to_owned())
        );
        assert!(State::new(PathBuf::from("/logs"), Vec::new())
            .conversation()
            .is_none());
    }
}
