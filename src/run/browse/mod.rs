mod action;
mod catalog;
mod entries;
mod error;
mod render;
mod result;
mod state;

pub use error::Error;
pub use result::Result;

use std::path::Path;

use ratatui::crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use crate::run::clock::now_millis;
use state::State;

/// Runs an interactive browsing session for the given directory.
pub fn run(dir: &Path, home: &Path) -> Result<()> {
    // rendering closure for the terminal
    let drive = |terminal: &mut DefaultTerminal, state: &mut State| -> Result<()> {
        while !state.done {
            terminal.draw(|frame| render::draw(frame, state, now_millis()))?;
            if let Event::Key(key) = event::read()? {
                // reads a key event from the terminal
                if let Some(command) = state.apply(&key) {
                    // ensure valid command
                    action::execute(state, command, dir, home); // execute the cmd
                }
            }
        }
        Ok(())
    };
    let mut state = State::new(dir.to_path_buf(), catalog::scan(dir)?);
    let mut terminal = ratatui::try_init()?;
    let outcome = drive(&mut terminal, &mut state);
    ratatui::restore();
    outcome
}
