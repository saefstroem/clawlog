use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::{Command, Key, State};

pub fn apply(state: &mut State, event: &KeyEvent) -> Option<Command> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let key = match event.code {
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Char(c) => {
            if !event.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
                return None;
            }
            Key::Char(c)
        }
        _ => return None,
    };
    state.apply(key)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use serde_json::json;

    use super::apply;
    use crate::run::browse::catalog::Conversation;
    use crate::run::browse::state::{Command, Level, Mode, State};

    fn state() -> State {
        let conversations = (0..3)
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
        apply(state, &KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn maps_navigation_keys() {
        let mut state = state();
        press(&mut state, KeyCode::Down);
        press(&mut state, KeyCode::Char('j'));
        assert_eq!(state.cursor, 2);
        press(&mut state, KeyCode::Up);
        press(&mut state, KeyCode::Char('k'));
        assert_eq!(state.cursor, 0);
        press(&mut state, KeyCode::PageDown);
        assert_eq!(state.cursor, 2);
        press(&mut state, KeyCode::PageUp);
        assert_eq!(state.cursor, 0);
        assert_eq!(press(&mut state, KeyCode::Enter), Some(Command::Open));
        state.open(vec![json!(1)]);
        press(&mut state, KeyCode::Esc);
        assert_eq!(state.level, Level::Conversations);
        press(&mut state, KeyCode::Char('q'));
        assert!(state.done);
    }

    #[test]
    fn maps_management_and_input_keys() {
        let mut state = state();
        assert_eq!(press(&mut state, KeyCode::Char('i')), None);
        press(&mut state, KeyCode::Char(' '));
        press(&mut state, KeyCode::Char(' '));
        assert_eq!(
            press(&mut state, KeyCode::Enter),
            Some(Command::Install(vec!["claude"]))
        );
        assert_eq!(press(&mut state, KeyCode::Char('u')), None);
        assert_eq!(
            press(&mut state, KeyCode::Char('y')),
            Some(Command::Uninstall)
        );
        assert_eq!(
            press(&mut state, KeyCode::Char('c')),
            Some(Command::AskClear)
        );
        press(&mut state, KeyCode::Char('e'));
        press(&mut state, KeyCode::Char('a'));
        press(&mut state, KeyCode::Char('b'));
        press(&mut state, KeyCode::Backspace);
        assert_eq!(state.mode, Mode::Input("a".to_owned()));
        assert_eq!(
            press(&mut state, KeyCode::Enter),
            Some(Command::Export(PathBuf::from("a")))
        );
    }

    #[test]
    fn modified_chars_are_ignored_and_shift_still_types() {
        let mut state = state();
        for c in ['c', 'u', 'i', 'e', 'q'] {
            let event = KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
            assert_eq!(apply(&mut state, &event), None, "{c}");
        }
        assert_eq!(state.mode, Mode::Normal);
        assert!(!state.done);
        press(&mut state, KeyCode::Char('e'));
        let ctrl = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(apply(&mut state, &ctrl), None);
        let alt = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT);
        assert_eq!(apply(&mut state, &alt), None);
        assert_eq!(state.mode, Mode::Input(String::new()));
        let shifted = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(apply(&mut state, &shifted), None);
        assert_eq!(state.mode, Mode::Input("A".to_owned()));
    }

    #[test]
    fn ignores_release_events_and_unmapped_keys() {
        let mut state = state();
        let release =
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(apply(&mut state, &release), None);
        assert_eq!(state.cursor, 0);
        for code in [KeyCode::Tab, KeyCode::F(1), KeyCode::Home, KeyCode::Left] {
            assert_eq!(press(&mut state, code), None);
        }
        assert_eq!(state.cursor, 0);
        assert!(!state.done);
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);
        apply(&mut state, &repeat);
        assert_eq!(state.cursor, 1);
    }
}
