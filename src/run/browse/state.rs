use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{to_string_pretty, Value};

use super::catalog::Conversation;
use crate::run::adapter::ADAPTERS;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Level {
    #[default]
    Conversations,
    Entries,
    Detail,
}

#[derive(Debug, Default, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Input(String),
    Confirm(String, Command),
    Select {
        cursor: usize,
        chosen: Vec<bool>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Open,
    Install(Vec<&'static str>),
    Uninstall,
    Export(PathBuf),
    AskClear,
    Delete,
}

#[derive(Default)]
pub struct State {
    pub dir: PathBuf,
    pub conversations: Vec<Conversation>,
    pub cursor: usize,
    pub level: Level,
    pub entries: Vec<Value>,
    pub entry_cursor: usize,
    pub detail: String,
    pub scroll: usize,
    pub mode: Mode,
    pub status: String,
    pub done: bool,
}

impl State {
    pub fn new(dir: PathBuf, conversations: Vec<Conversation>) -> State {
        State {
            dir,
            conversations,
            ..State::default()
        }
    }

    pub fn apply(&mut self, event: &KeyEvent) -> Option<Command> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            self.done = true;
        }
        if event.kind == KeyEventKind::Release
            || !event.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            return None;
        }
        match (&mut self.mode, event.code) {
            (Mode::Input(buffer), KeyCode::Char(c)) => buffer.push(c),
            (Mode::Input(buffer), KeyCode::Backspace) => {
                buffer.pop();
            }
            (Mode::Input(buffer), KeyCode::Enter) => {
                let target = PathBuf::from(buffer.as_str());
                self.mode = Mode::Normal;
                return (!target.as_os_str().is_empty()).then_some(Command::Export(target));
            }
            (Mode::Input(_), KeyCode::Esc) => self.mode = Mode::Normal,
            (Mode::Confirm(_, command), KeyCode::Char('y' | 'Y')) => {
                let command = command.clone();
                self.mode = Mode::Normal;
                return Some(command);
            }
            (Mode::Confirm(..), _) | (Mode::Select { .. }, KeyCode::Esc) => {
                self.mode = Mode::Normal;
                self.status = "aborted".to_owned();
            }
            (Mode::Select { cursor, .. }, KeyCode::Up | KeyCode::Char('k')) => {
                *cursor = cursor.saturating_sub(1);
            }
            (Mode::Select { cursor, chosen }, KeyCode::Down | KeyCode::Char('j')) => {
                *cursor = (*cursor + 1).min(chosen.len().saturating_sub(1));
            }
            (Mode::Select { cursor, chosen }, KeyCode::Char(' ')) => {
                if let Some(flag) = chosen.get_mut(*cursor) {
                    *flag = !*flag;
                }
            }
            (Mode::Select { chosen, .. }, KeyCode::Enter) => {
                let names: Vec<&'static str> = ADAPTERS
                    .iter()
                    .zip(chosen.iter())
                    .filter(|(_, chosen)| **chosen)
                    .map(|(adapter, _)| adapter.name())
                    .collect();
                self.mode = Mode::Normal;
                if !names.is_empty() {
                    return Some(Command::Install(names));
                }
                self.status = "nothing selected".to_owned();
            }
            (Mode::Normal, code) => return self.navigate(code),
            _ => {}
        }
        None
    }

    fn navigate(&mut self, code: KeyCode) -> Option<Command> {
        let top = self.level == Level::Conversations;
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Esc => match self.level {
                Level::Detail => self.level = Level::Entries,
                Level::Entries => self.level = Level::Conversations,
                Level::Conversations => self.done = true,
            },
            KeyCode::Enter if top && !self.conversations.is_empty() => return Some(Command::Open),
            KeyCode::Enter if self.level == Level::Entries => {
                if let Some(entry) = self.entries.get(self.entry_cursor) {
                    self.detail = to_string_pretty(entry).unwrap_or_default();
                    self.scroll = 0;
                    self.level = Level::Detail;
                }
            }
            KeyCode::Char('i') if top => {
                let chosen = vec![true; ADAPTERS.len()];
                self.mode = Mode::Select { cursor: 0, chosen };
            }
            KeyCode::Char('u') if top => {
                let prompt = "remove clawlog hooks from ~/.claude/settings.json? [y/N]";
                self.mode = Mode::Confirm(prompt.to_owned(), Command::Uninstall);
            }
            KeyCode::Char('e') if top => self.mode = Mode::Input(String::new()),
            KeyCode::Char('c') if top => return Some(Command::AskClear),
            _ => {}
        }
        None
    }

    fn move_by(&mut self, delta: isize) {
        let (cursor, len) = match self.level {
            Level::Conversations => (&mut self.cursor, self.conversations.len()),
            Level::Entries => (&mut self.entry_cursor, self.entries.len()),
            Level::Detail => (&mut self.scroll, usize::MAX),
        };
        *cursor = cursor
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }

    pub fn conversation(&self) -> Option<&Conversation> {
        self.conversations.get(self.cursor)
    }

    pub fn open(&mut self, entries: Vec<Value>) {
        self.entries = entries;
        self.entry_cursor = 0;
        self.level = Level::Entries;
    }

    pub fn offer_clear(&mut self, count: usize) {
        if count == 0 {
            self.status = "nothing to delete".to_owned();
            return;
        }
        let prompt = format!(
            "delete {count} log file(s) under {}? [y/N]",
            self.dir.display()
        );
        self.mode = Mode::Confirm(prompt, Command::Delete);
    }

    pub fn reload(&mut self, conversations: Vec<Conversation>) {
        self.conversations = conversations;
        self.cursor = self.cursor.min(self.conversations.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use serde_json::json;

    use super::{Command, Level, Mode, State};
    use crate::run::browse::catalog::Conversation;

    fn state(count: usize) -> State {
        let conversations = (0..count)
            .map(|n| Conversation {
                adapter: "claude".to_owned(),
                id: format!("s{n}"),
                parts: Vec::new(),
                bytes: 0,
                last_ms: 0,
            })
            .collect();
        State::new(PathBuf::from("/logs"), conversations)
    }

    fn press(state: &mut State, code: KeyCode) -> Option<Command> {
        state.apply(&KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn keys(state: &mut State, text: &str) {
        for c in text.chars() {
            press(state, KeyCode::Char(c));
        }
    }

    #[test]
    fn moves_clamps_descends_and_backs_out() {
        let mut state = state(3);
        press(&mut state, KeyCode::Down);
        keys(&mut state, "jj");
        assert_eq!(state.cursor, 2);
        press(&mut state, KeyCode::PageUp);
        assert_eq!(state.cursor, 0);
        press(&mut state, KeyCode::PageDown);
        assert_eq!(state.conversation().map(|c| c.id.as_str()), Some("s2"));
        assert_eq!(press(&mut state, KeyCode::Enter), Some(Command::Open));
        state.open(vec![json!({"n": 1}), json!({"n": 2})]);
        keys(&mut state, "jjk");
        assert_eq!((state.level, state.entry_cursor), (Level::Entries, 0));
        assert_eq!(press(&mut state, KeyCode::Enter), None);
        assert_eq!(state.level, Level::Detail);
        assert_eq!(state.detail, "{\n  \"n\": 1\n}");
        press(&mut state, KeyCode::Esc);
        press(&mut state, KeyCode::Esc);
        assert_eq!(state.level, Level::Conversations);
        assert!(!state.done);
        press(&mut state, KeyCode::Esc);
        assert!(state.done);
    }

    #[test]
    fn empty_lists_stay_put() {
        let mut state = state(0);
        keys(&mut state, "jk");
        assert_eq!(press(&mut state, KeyCode::Enter), None);
        assert_eq!((state.level, state.cursor), (Level::Conversations, 0));
        state.open(Vec::new());
        assert_eq!(press(&mut state, KeyCode::Enter), None);
        assert_eq!(state.level, Level::Entries);
    }

    #[test]
    fn install_picker_toggles_installs_and_cancels() {
        let mut state = state(1);
        keys(&mut state, "i ");
        assert_eq!(press(&mut state, KeyCode::Enter), None);
        assert_eq!(state.status, "nothing selected");
        keys(&mut state, "ijk");
        assert_eq!(
            press(&mut state, KeyCode::Enter),
            Some(Command::Install(vec!["claude"]))
        );
        keys(&mut state, "i");
        press(&mut state, KeyCode::Esc);
        assert_eq!(
            (&state.mode, state.status.as_str()),
            (&Mode::Normal, "aborted")
        );
    }

    #[test]
    fn confirmations_fire_on_y_and_abort_on_anything_else() {
        let mut state = state(1);
        keys(&mut state, "u");
        assert_eq!(
            press(&mut state, KeyCode::Char('Y')),
            Some(Command::Uninstall)
        );
        assert_eq!(
            press(&mut state, KeyCode::Char('c')),
            Some(Command::AskClear)
        );
        state.offer_clear(0);
        assert_eq!(
            (&state.mode, state.status.as_str()),
            (&Mode::Normal, "nothing to delete")
        );
        state.offer_clear(3);
        assert_eq!(
            state.mode,
            Mode::Confirm(
                "delete 3 log file(s) under /logs? [y/N]".to_owned(),
                Command::Delete
            )
        );
        assert_eq!(press(&mut state, KeyCode::Char('y')), Some(Command::Delete));
        for code in [KeyCode::Char('n'), KeyCode::Char('q'), KeyCode::Enter] {
            state.offer_clear(1);
            assert_eq!(press(&mut state, code), None);
            assert_eq!(
                (&state.mode, state.status.as_str()),
                (&Mode::Normal, "aborted")
            );
            assert!(!state.done);
        }
    }

    #[test]
    fn export_input_edits_submits_and_cancels() {
        let mut state = state(1);
        keys(&mut state, "e/tmq");
        press(&mut state, KeyCode::Backspace);
        keys(&mut state, "p");
        press(&mut state, KeyCode::Up);
        assert_eq!(state.mode, Mode::Input("/tmp".to_owned()));
        assert_eq!(
            press(&mut state, KeyCode::Enter),
            Some(Command::Export(PathBuf::from("/tmp")))
        );
        keys(&mut state, "ex");
        press(&mut state, KeyCode::Esc);
        keys(&mut state, "e");
        assert_eq!(press(&mut state, KeyCode::Enter), None);
        assert_eq!(state.mode, Mode::Normal);
    }

    #[test]
    fn management_keys_only_act_on_the_conversations_level() {
        let mut state = state(1);
        state.open(vec![json!(1)]);
        for c in ['i', 'u', 'c', 'e'] {
            assert_eq!(press(&mut state, KeyCode::Char(c)), None);
        }
        assert_eq!(state.mode, Mode::Normal);
    }

    #[test]
    fn ctrl_c_quits_anywhere_while_q_releases_and_other_chords_do_nothing() {
        let mut state = state(3);
        for event in [
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Release),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        ] {
            assert_eq!(state.apply(&event), None);
        }
        assert_eq!(
            (state.cursor, &state.mode, state.done),
            (0, &Mode::Normal, false)
        );
        keys(&mut state, "e");
        state.apply(&KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        assert_eq!(state.mode, Mode::Input("A".to_owned()));
        state.apply(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(state.done);
    }

    #[test]
    fn reload_clamps_the_cursor() {
        let mut state = state(3);
        press(&mut state, KeyCode::PageDown);
        state.reload(Vec::new());
        assert_eq!(state.cursor, 0);
    }
}
