use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::entries;
use super::state::{Level, Mode, State};
use crate::run::adapter::ADAPTERS;

pub fn draw(frame: &mut Frame, state: &State) {
    let dim = Style::new().fg(Color::DarkGray);
    let role_style =
        |entry: &serde_json::Value| match entry.get("role").and_then(serde_json::Value::as_str) {
            Some("user") => Style::new().fg(Color::Green),
            Some("assistant") => Style::new().fg(Color::Cyan),
            Some("tool") => Style::new().fg(Color::Magenta),
            _ => dim,
        };
    let age = |last_ms: u64| -> String {
        if last_ms == 0 || state.now_ms <= last_ms {
            return "now".to_owned();
        }
        let seconds = (state.now_ms - last_ms) / 1000;
        match seconds {
            0..=59 => "now".to_owned(),
            60..=3599 => format!("{}m", seconds / 60),
            3600..=86_399 => format!("{}h", seconds / 3600),
            86_400..=604_799 => format!("{}d", seconds / 86_400),
            _ => format!("{}w", seconds / 604_800),
        }
    };
    let conversations = || -> Vec<Line> {
        state
            .conversations
            .iter()
            .map(|conversation| {
                Line::from(vec![
                    Span::styled(format!("{:>4}  ", age(conversation.last_ms)), dim),
                    Span::styled(
                        conversation.adapter.clone(),
                        Style::new().fg(Color::Magenta),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        conversation.id.clone(),
                        Style::new().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "  {} part(s)  {} B",
                            conversation.parts.len(),
                            conversation.bytes
                        ),
                        dim,
                    ),
                ])
            })
            .collect()
    };
    let rows = || -> Vec<Line> {
        state
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| Line::styled(entries::line(index, entry), role_style(entry)))
            .collect()
    };
    let footer = || match &state.mode {
        Mode::Input(buffer) => format!("export to: {buffer}"),
        Mode::Confirm(prompt, _) => prompt.clone(),
        Mode::Select { .. } => "Space toggle  Up/Down move  Enter install  Esc cancel".to_owned(),
        Mode::Normal => match state.level {
            Level::Conversations => {
                "Up/Down move  Enter open  i install  u uninstall  e export  c clear  q quit"
            }
            Level::Entries => "Up/Down move  Enter detail  Esc back  q quit",
            Level::Detail => "Up/Down scroll  Esc back  q quit",
        }
        .to_owned(),
    };
    let footer_style = match state.mode {
        Mode::Normal => dim,
        _ => Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    };
    let list = |frame: &mut Frame, area: Rect, rows: Vec<Line>, cursor: usize| {
        let items: Vec<ListItem> = rows.into_iter().map(ListItem::new).collect();
        let widget = List::new(items)
            .highlight_symbol("> ")
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(
            widget,
            area,
            &mut ListState::default().with_selected(Some(cursor)),
        );
    };
    let [title, main, legend, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let heading = if matches!(state.mode, Mode::Select { .. }) {
        "Install hooks"
    } else {
        match state.level {
            Level::Conversations => "Conversations",
            Level::Entries => "Entries",
            Level::Detail => "Entry",
        }
    };
    frame.render_widget(
        Paragraph::new(heading).style(Style::new().add_modifier(Modifier::BOLD)),
        title,
    );
    if let Mode::Select { cursor, chosen } = &state.mode {
        let rows: Vec<Line> = ADAPTERS
            .iter()
            .enumerate()
            .map(|(index, adapter)| {
                let ticked = chosen.get(index).copied().unwrap_or_default();
                let row = format!("[{}] {}", if ticked { "x" } else { " " }, adapter.name());
                if index == *cursor {
                    Line::styled(
                        row,
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    )
                } else {
                    Line::styled(row, dim)
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(rows), main);
        frame.render_widget(Paragraph::new(footer()).style(footer_style), legend);
        frame.render_widget(
            Paragraph::new(state.status.as_str()).style(Style::new().fg(Color::Yellow)),
            status,
        );
        return;
    }
    match state.level {
        Level::Conversations if state.conversations.is_empty() => {
            let empty = "\n\nyou have no logs at the moment \u{2014} they will show up here once a hook is installed\n\npress i, pick the adapters with Space and hit Enter to install their hooks;\nthen start that LLM, interact, and the conversation appears here";
            frame.render_widget(Paragraph::new(empty).alignment(Alignment::Center), main);
        }
        Level::Conversations => list(frame, main, conversations(), state.cursor),
        Level::Entries => list(frame, main, rows(), state.entry_cursor),
        Level::Detail => {
            let scroll = u16::try_from(state.scroll).unwrap_or(u16::MAX);
            frame.render_widget(
                Paragraph::new(state.detail.join("\n")).scroll((scroll, 0)),
                main,
            );
        }
    }
    frame.render_widget(Paragraph::new(footer()).style(footer_style), legend);
    frame.render_widget(
        Paragraph::new(state.status.as_str()).style(Style::new().fg(Color::Yellow)),
        status,
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;
    use ratatui::style::Color;
    use ratatui::Terminal;
    use serde_json::json;

    use super::draw;
    use crate::run::browse::catalog::Conversation;
    use crate::run::browse::state::{Key, State};

    fn state() -> State {
        let conversations = vec![
            Conversation {
                adapter: "claude".to_owned(),
                id: "s1".to_owned(),
                parts: vec![PathBuf::from("p1"), PathBuf::from("p2")],
                bytes: 84,
                last_ms: 1_790_000_000_000,
            },
            Conversation {
                adapter: "claude".to_owned(),
                id: "s2".to_owned(),
                parts: vec![PathBuf::from("p3")],
                bytes: 9,
                last_ms: 1_789_996_400_000,
            },
        ];
        State::new(PathBuf::from("/logs"), conversations)
    }

    fn render(state: &State) -> (String, HashSet<Color>) {
        let mut terminal = Terminal::new(TestBackend::new(90, 16)).unwrap();
        terminal.draw(|frame| draw(frame, state)).unwrap();
        let buffer = terminal.backend().buffer();
        let text = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.cell((x, y)).map(Cell::symbol).unwrap_or_default())
                    .collect::<String>()
            })
            .collect::<Vec<String>>()
            .join("\n");
        let colors = buffer
            .content()
            .iter()
            .filter(|cell| !cell.symbol().trim().is_empty())
            .map(|cell| cell.style().fg.unwrap_or(Color::Reset))
            .collect();
        (text, colors)
    }

    fn screen(state: &State) -> String {
        render(state).0
    }

    #[test]
    fn empty_catalog_explains_how_to_get_data() {
        let empty = State::new(PathBuf::from("/logs"), Vec::new());
        let text = screen(&empty);
        assert!(text.contains("you have no logs at the moment"), "{text}");
        assert!(
            text.contains("they will show up here once a hook is installed"),
            "{text}"
        );
        assert!(text.contains("pick the adapters with Space"), "{text}");
        assert!(
            text.contains("start that LLM, interact, and the conversation appears here"),
            "{text}"
        );
        assert!(text.contains("q quit"), "{text}");
    }

    #[test]
    fn conversations_view_lists_rows_with_selection_legend_and_color() {
        let mut state = state();
        state.tick(1_790_000_300_000);
        state.status = "added 4 hook(s) to /h/.claude/settings.json (0 already present)".to_owned();
        let (screen, colors) = render(&state);
        assert!(screen.starts_with("Conversations"), "{screen}");
        assert!(
            screen.contains(">   5m  claude  s1  2 part(s)  84 B"),
            "{screen}"
        );
        assert!(
            screen.contains("    1h  claude  s2  1 part(s)  9 B"),
            "{screen}"
        );
        assert!(
            screen.contains(
                "Up/Down move  Enter open  i install  u uninstall  e export  c clear  q quit"
            ),
            "{screen}"
        );
        assert!(screen.contains("added 4 hook(s)"), "{screen}");
        assert!(colors.contains(&Color::Magenta), "{colors:?}");
        assert!(colors.contains(&Color::Yellow), "{colors:?}");
    }

    #[test]
    fn entries_view_colors_lines_by_role() {
        let mut state = state();
        state.open(vec![
            json!({"ts": 1_790_000_000_123u64, "role": "user", "kind": "prompt", "text": "hi"}),
            json!({"ts": 1_790_000_001_000u64, "role": "tool", "kind": "tool_call", "tool": "Bash", "input": "ls"}),
        ]);
        state.apply(Key::Down);
        let (screen, colors) = render(&state);
        assert!(
            screen.contains("   0  1790000000123  user       prompt       hi"),
            "{screen}"
        );
        assert!(screen.contains(">    1  1790000001000"), "{screen}");
        assert!(screen.contains("Bash ls"), "{screen}");
        assert!(
            screen.contains("Up/Down move  Enter detail  Esc back  q quit"),
            "{screen}"
        );
        assert!(colors.contains(&Color::Green), "{colors:?}");
        assert!(colors.contains(&Color::Magenta), "{colors:?}");
    }

    #[test]
    fn ages_read_now_minutes_hours_days_and_weeks() {
        let mut state = state();
        let now = 1_790_000_000_000u64;
        state.tick(now);
        let ages = [
            (now - 30_000, "now"),
            (now - 90_000, "1m"),
            (now - 7_200_000, "2h"),
            (now - 172_800_000, "2d"),
            (now - 1_814_400_000, "3w"),
            (0, "now"),
        ];
        for (index, (last_ms, expected)) in ages.into_iter().enumerate() {
            state.conversations[index % 2].last_ms = last_ms;
            let text = screen(&state);
            assert!(
                text.contains(&format!("{expected:>4}  claude")),
                "{expected}: {text}"
            );
        }
    }

    #[test]
    fn detail_view_shows_pretty_json_and_scrolls() {
        let mut state = state();
        state.open(vec![json!({"kind": "prompt", "text": "hi"})]);
        state.apply(Key::Enter);
        let top = screen(&state);
        let mut lines = top.lines();
        assert!(
            lines.next().is_some_and(|line| line.starts_with("Entry")),
            "{top}"
        );
        assert!(
            lines.next().is_some_and(|line| line.starts_with("{")),
            "{top}"
        );
        assert!(top.contains("\"kind\": \"prompt\""), "{top}");
        assert!(top.contains("Up/Down scroll  Esc back  q quit"), "{top}");
        state.apply(Key::Down);
        let scrolled = screen(&state);
        assert!(
            scrolled
                .lines()
                .nth(1)
                .is_some_and(|line| line.starts_with("  \"kind\": \"prompt\"")),
            "{scrolled}"
        );
    }

    #[test]
    fn footer_shows_input_and_confirm_lines() {
        let mut state = state();
        state.apply(Key::Char('e'));
        for c in "/tmp/x".chars() {
            state.apply(Key::Char(c));
        }
        assert!(screen(&state).contains("export to: /tmp/x"));
        state.apply(Key::Esc);
        state.offer_clear(3);
        let screen = screen(&state);
        assert!(
            screen.contains("delete 3 log file(s) under /logs? [y/N]"),
            "{screen}"
        );
    }

    #[test]
    fn install_selection_lists_adapters_with_checkboxes() {
        let mut state = State::new(PathBuf::from("/logs"), Vec::new());
        state.apply(Key::Char('i'));
        let text = screen(&state);
        assert!(text.starts_with("Install hooks"), "{text}");
        assert!(text.contains("[x] claude"), "{text}");
        assert!(
            text.contains("Space toggle  Up/Down move  Enter install  Esc cancel"),
            "{text}"
        );
        state.apply(Key::Char(' '));
        assert!(screen(&state).contains("[ ] claude"));
    }

    #[test]
    fn status_line_shows_action_results() {
        let mut state = state();
        state.status = "exported 3 file(s) to /tmp/out".to_owned();
        assert!(screen(&state).contains("exported 3 file(s) to /tmp/out"));
        state.status = "invalid part file: expected value at line 1 column 1".to_owned();
        assert!(screen(&state).contains("invalid part file:"));
    }
}
