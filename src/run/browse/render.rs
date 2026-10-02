use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListState, Paragraph};
use ratatui::Frame;
use serde_json::Value;

use super::entries;
use super::state::{Level, Mode, State};
use crate::run::adapter::ADAPTERS;

pub fn draw(frame: &mut Frame, state: &mut State, now_ms: u64) {
    let dim = Style::new().fg(Color::DarkGray);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let yellow = Style::new().fg(Color::Yellow);
    let age = |last_ms: u64| match now_ms.saturating_sub(last_ms) / 1000 {
        seconds if last_ms == 0 || seconds < 60 => "now".to_owned(),
        seconds if seconds < 3600 => format!("{}m", seconds / 60),
        seconds if seconds < 86_400 => format!("{}h", seconds / 3600),
        seconds if seconds < 604_800 => format!("{}d", seconds / 86_400),
        seconds => format!("{}w", seconds / 604_800),
    };
    let role = |entry: &Value| match entry.get("role").and_then(Value::as_str) {
        Some("user") => Style::new().fg(Color::Green),
        Some("assistant") => Style::new().fg(Color::Cyan),
        Some("tool") => Style::new().fg(Color::Magenta),
        _ => dim,
    };
    let list = |frame: &mut Frame, area: Rect, mut rows: Vec<Line>, cursor: usize| {
        rows.reverse();
        let list = List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        let mut selection = ListState::default().with_selected(Some(cursor));
        frame.render_stateful_widget(list, area, &mut selection);
    };
    let [title, main, legend, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let (mut heading, keys) = match state.level {
        Level::Conversations => (
            "Conversations",
            "Up/Down move  Enter open  i install  u uninstall  e export  c clear  q quit",
        ),
        Level::Entries => ("Entries", "Up/Down move  Enter detail  Esc back  q quit"),
        Level::Detail => ("Entry", "Up/Down scroll  Esc back  q quit"),
    };
    let footer = match &state.mode {
        Mode::Normal => keys.to_owned(),
        Mode::Input(buffer) => format!("export to: {buffer}"),
        Mode::Confirm(prompt, _) => prompt.clone(),
        Mode::Select { .. } => {
            heading = "Install hooks";
            "Space toggle  Up/Down move  Enter install  Esc cancel".to_owned()
        }
    };
    match (&state.mode, state.level) {
        (Mode::Select { cursor, chosen }, _) => {
            let rows = ADAPTERS.iter().zip(chosen).map(|(adapter, chosen)| {
                Line::raw(format!(
                    "[{}] {}",
                    if *chosen { "x" } else { " " },
                    adapter.name()
                ))
            });
            list(frame, main, rows.collect(), *cursor);
        }
        (_, Level::Conversations) if state.conversations.is_empty() => {
            let empty = "\n\nyou have no logs at the moment \u{2014} they will show up here once a hook is installed\n\npress i, pick the adapters with Space and hit Enter to install their hooks;\nthen start that LLM, interact, and the conversation appears here";
            frame.render_widget(Paragraph::new(empty).alignment(Alignment::Center), main);
        }
        (_, Level::Conversations) => {
            let rows = state.conversations.iter().map(|conversation| {
                Line::from(vec![
                    Span::styled(format!("{:>4}  ", age(conversation.last_ms)), dim),
                    Span::styled(conversation.adapter.as_str(), Color::Magenta),
                    Span::raw("  "),
                    Span::styled(conversation.id.as_str(), bold),
                    Span::styled(
                        format!(
                            "  {} part(s)  {} B",
                            conversation.parts.len(),
                            conversation.bytes
                        ),
                        dim,
                    ),
                ])
            });
            list(frame, main, rows.collect(), state.cursor);
        }
        (_, Level::Entries) => {
            let rows = state
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| Line::styled(entries::line(index, entry), role(entry)));
            list(frame, main, rows.collect(), state.entry_cursor);
        }
        (_, Level::Detail) => {
            let width = usize::from(main.width.max(1));
            let lines: Vec<String> = state
                .detail
                .lines()
                .flat_map(|line| {
                    let chars: Vec<char> = line.chars().collect();
                    let chunks = chars.chunks(width).map(String::from_iter);
                    chunks.collect::<Vec<String>>()
                })
                .collect();
            state.scroll = state.scroll.min(lines.len().saturating_sub(1));
            let visible = lines.into_iter().skip(state.scroll);
            let visible: Text = visible.take(usize::from(main.height)).collect();
            frame.render_widget(Paragraph::new(visible), main);
        }
    }
    let footer_style = match state.mode {
        Mode::Normal => dim,
        _ => yellow.add_modifier(Modifier::BOLD),
    };
    frame.render_widget(Paragraph::new(heading).style(bold), title);
    frame.render_widget(Paragraph::new(footer).style(footer_style), legend);
    frame.render_widget(Paragraph::new(state.status.as_str()).style(yellow), status);
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Color;
    use ratatui::Terminal;
    use serde_json::json;

    use super::draw;
    use crate::run::browse::catalog::Conversation;
    use crate::run::browse::state::State;

    const NOW: u64 = 1_790_000_300_000;

    fn state() -> State {
        let conversation = |id: &str, parts: usize, bytes: u64, last_ms: u64| Conversation {
            adapter: "claude".to_owned(),
            id: id.to_owned(),
            parts: vec![PathBuf::new(); parts],
            bytes,
            last_ms,
        };
        let conversations = vec![
            conversation("s1", 2, 84, NOW - 300_000),
            conversation("s2", 1, 9, NOW - 3_900_000),
        ];
        State::new(PathBuf::from("/logs"), conversations)
    }

    fn press(state: &mut State, code: KeyCode) {
        state.apply(&KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn render(state: &mut State, width: u16) -> (String, HashSet<Color>) {
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal.draw(|frame| draw(frame, state, NOW)).unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(Cell::symbol).collect())
            .collect();
        let colors = buffer
            .content()
            .iter()
            .filter(|cell| !cell.symbol().trim().is_empty())
            .filter_map(|cell| cell.style().fg)
            .collect();
        (rows.join("\n"), colors)
    }

    fn screen(state: &mut State) -> String {
        render(state, 90).0
    }

    #[test]
    fn conversations_show_age_adapter_id_parts_size_legend_and_status() {
        let mut state = state();
        state.status = "added 4 hook(s)".to_owned();
        let (text, colors) = render(&mut state, 90);
        assert!(text.starts_with("Conversations"), "{text}");
        assert!(
            text.contains(">   5m  claude  s1  2 part(s)  84 B"),
            "{text}"
        );
        assert!(
            text.contains("    1h  claude  s2  1 part(s)  9 B"),
            "{text}"
        );
        assert!(text.contains("e export  c clear  q quit"), "{text}");
        assert!(text.contains("added 4 hook(s)"), "{text}");
        assert!(colors.is_superset(&HashSet::from([Color::Magenta, Color::Yellow])));
    }

    #[test]
    fn ages_read_now_minutes_hours_days_and_weeks() {
        let mut state = state();
        for (last_ms, expected) in [
            (NOW - 30_000, "now"),
            (NOW - 7_200_000, "2h"),
            (NOW - 172_800_000, "2d"),
            (NOW - 1_814_400_000, "3w"),
            (0, "now"),
        ] {
            state.conversations[1].last_ms = last_ms;
            let text = screen(&mut state);
            assert!(
                text.contains(&format!("{expected:>4}  claude  s2")),
                "{text}"
            );
        }
    }

    #[test]
    fn empty_catalog_explains_how_to_get_data() {
        let text = screen(&mut State::new(PathBuf::from("/logs"), Vec::new()));
        for needle in [
            "you have no logs at the moment",
            "pick the adapters with Space",
            "start that LLM, interact, and the conversation appears here",
            "q quit",
        ] {
            assert!(text.contains(needle), "{needle}: {text}");
        }
    }

    #[test]
    fn entries_are_colored_by_role() {
        let mut state = state();
        state.open(vec![
            json!({"ts": 1, "role": "user", "kind": "prompt", "text": "hi"}),
            json!({"ts": 2, "role": "tool", "kind": "tool_call", "tool": "Bash", "input": "ls"}),
        ]);
        press(&mut state, KeyCode::Down);
        let (text, colors) = render(&mut state, 90);
        assert!(
            text.contains("   0  1  user       prompt       hi"),
            "{text}"
        );
        assert!(
            text.contains(">    1  2  tool       tool_call    Bash ls"),
            "{text}"
        );
        assert!(text.contains("Enter detail  Esc back"), "{text}");
        assert!(colors.is_superset(&HashSet::from([Color::Green, Color::Magenta])));
    }

    #[test]
    fn detail_wraps_to_the_width_and_clamps_the_scroll() {
        let mut state = state();
        state.open(vec![json!({"output": "x".repeat(100)})]);
        press(&mut state, KeyCode::Enter);
        let text = render(&mut state, 40).0;
        assert!(text
            .lines()
            .nth(1)
            .is_some_and(|line| line == format!("{:<40}", "{")));
        assert!(text.contains(&"x".repeat(40)), "{text}");
        for _ in 0..3 {
            press(&mut state, KeyCode::PageDown);
        }
        let text = render(&mut state, 40).0;
        assert_eq!(state.scroll, 4);
        assert!(
            text.lines()
                .nth(1)
                .is_some_and(|line| line.starts_with('}')),
            "{text}"
        );
        press(&mut state, KeyCode::Up);
        assert_eq!(state.scroll, 3);
    }

    #[test]
    fn modes_replace_the_legend() {
        let mut state = state();
        for c in "e/tmp/x".chars() {
            press(&mut state, KeyCode::Char(c));
        }
        assert!(screen(&mut state).contains("export to: /tmp/x"));
        press(&mut state, KeyCode::Esc);
        state.offer_clear(3);
        assert!(screen(&mut state).contains("delete 3 log file(s) under /logs? [y/N]"));
        press(&mut state, KeyCode::Esc);
        press(&mut state, KeyCode::Char('i'));
        let text = screen(&mut state);
        assert!(text.starts_with("Install hooks"), "{text}");
        assert!(text.contains("> [x] claude"), "{text}");
        assert!(
            text.contains("Space toggle  Up/Down move  Enter install"),
            "{text}"
        );
        press(&mut state, KeyCode::Char(' '));
        assert!(screen(&mut state).contains("[ ] claude"));
    }
}
