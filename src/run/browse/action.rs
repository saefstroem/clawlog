use std::path::Path;

use super::state::{Command, State};
use super::{catalog, entries};
use crate::run::adapter::{self, Adapter, ADAPTERS};
use crate::run::{clear, export};

pub fn execute(state: &mut State, command: Command, dir: &Path, home: &Path) {
    let manage = |action: &dyn Fn(&dyn Adapter) -> adapter::Result<String>| {
        ADAPTERS
            .iter()
            .map(|adapter| match action(*adapter) {
                Ok(text) => text,
                Err(error) => error.to_string(),
            })
            .collect::<Vec<String>>()
            .join("; ")
    };
    // match on the command and execute the corresponding action
    match command {
        Command::Open => {
            // load the conversation parts and update the state accordingly
            let parts = state
                .conversation()
                .map(|conversation| conversation.parts.clone())
                .unwrap_or_default();
            match entries::load(&parts) {
                Ok(list) => state.open(list),
                Err(error) => state.status = error.to_string(),
            }
        }
        Command::Install(names) => {
            // install the specified adapters and update the state status accordingly
            state.status = ADAPTERS
                .iter()
                .filter(|adapter| names.contains(&adapter.name()))
                .map(|adapter| match adapter.install(home) {
                    Ok(installed) => installed.to_string(),
                    Err(error) => error.to_string(),
                })
                .collect::<Vec<String>>()
                .join("; ");
        }
        Command::Uninstall => {
            // uninstall the adapters and update the state status accordingly
            state.status =
                manage(&|adapter| adapter.uninstall(home).map(|removed| removed.to_string()));
        }
        Command::Export(target) => {
            // export the current state to the specified target and update the state status accordingly
            let expanded = target.strip_prefix("~").ok().map(|rest| home.join(rest));
            let target = expanded.unwrap_or(target);
            state.status = match export::export(dir, &target) {
                Ok(count) => format!("exported {count} file(s) to {}", target.display()),
                Err(error) => error.to_string(),
            };
        }
        Command::AskClear => match clear::part_files(dir) {
            // attempt to clear the part files and update the state status accordingly
            Ok(files) => state.offer_clear(files.len()),
            Err(error) => state.status = error.to_string(),
        },
        Command::Delete => {
            // attempt to clear all part files and update the state status accordingly
            state.status = match clear::clear(dir) {
                Ok(count) => format!("deleted {count} file(s)"),
                Err(error) => error.to_string(),
            };
            match catalog::scan(dir) {
                Ok(conversations) => state.reload(conversations),
                Err(error) => state.status = error.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::execute;
    use crate::run::browse::catalog;
    use crate::run::browse::state::{Command, Level, Mode, Pending, State};
    use crate::run::scratch_dir::ScratchDir;

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn state(dir: &Path) -> State {
        State::new(dir.to_path_buf(), catalog::scan(dir).unwrap())
    }

    #[test]
    fn open_on_a_corrupt_part_surfaces_the_error_in_the_status() {
        let scratch = ScratchDir::new();
        let logs = scratch.path().join("logs");
        write(&logs, "claude/clawlog_claude_s1_0001.part.json", "[\n{");
        let mut state = state(&logs);
        execute(&mut state, Command::Open, &logs, scratch.path());
        assert_eq!(state.level, Level::Conversations);
        assert!(state.status.starts_with("invalid part file:"));
    }

    #[test]
    fn install_and_uninstall_report_against_the_passed_home() {
        let scratch = ScratchDir::new();
        let logs = scratch.path().join("logs");
        let home = scratch.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let mut state = state(&logs);
        execute(&mut state, Command::Install(vec!["claude"]), &logs, &home);
        let settings = home.join(".claude/settings.json");
        assert_eq!(
            state.status,
            format!(
                "added 4 hook(s) to {} (0 already present)",
                settings.display()
            )
        );
        execute(&mut state, Command::Uninstall, &logs, &home);
        assert_eq!(
            state.status,
            format!("removed 4 hook(s) from {}", settings.display())
        );
        assert!(!fs::read_to_string(&settings).unwrap().contains("clawlog"));
    }

    #[test]
    fn export_reports_success_errors_and_expands_tilde() {
        let scratch = ScratchDir::new();
        let logs = scratch.path().join("logs");
        let home = scratch.path().join("home");
        fs::create_dir_all(&home).unwrap();
        write(
            &logs,
            "claude/clawlog_claude_s1_0001.part.json",
            "[\n1\n]\n",
        );
        let mut state = state(&logs);
        let target = scratch.path().join("out");
        execute(&mut state, Command::Export(target.clone()), &logs, &home);
        assert_eq!(
            state.status,
            format!("exported 1 file(s) to {}", target.display())
        );
        assert!(target
            .join("claude/clawlog_claude_s1_0001.part.json")
            .is_file());
        let tilde = Command::Export(PathBuf::from("~/x"));
        execute(&mut state, tilde, &logs, &home);
        assert!(home
            .join("x/claude/clawlog_claude_s1_0001.part.json")
            .is_file());
        assert!(state.status.contains(&home.join("x").display().to_string()));
        execute(&mut state, Command::Export(logs.clone()), &logs, &home);
        assert_eq!(state.status, "export target overlaps the log directory");
    }

    #[test]
    fn ask_clear_prompts_when_files_exist_and_reports_when_none_do() {
        let scratch = ScratchDir::new();
        let logs = scratch.path().join("logs");
        let mut state = state(&logs);
        execute(&mut state, Command::AskClear, &logs, scratch.path());
        assert_eq!(state.status, "nothing to delete");
        assert_eq!(state.mode, Mode::Normal);
        write(&logs, "claude/clawlog_claude_s1_0001.part.json", "[]");
        execute(&mut state, Command::AskClear, &logs, scratch.path());
        assert_eq!(
            state.mode,
            Mode::Confirm(
                format!("delete 1 log file(s) under {}? [y/N]", logs.display()),
                Pending::Delete
            )
        );
    }

    #[test]
    fn delete_reports_the_count_and_reloads_an_empty_catalog() {
        let scratch = ScratchDir::new();
        let logs = scratch.path().join("logs");
        write(
            &logs,
            "claude/clawlog_claude_s1_0001.part.json",
            "[\n1\n]\n",
        );
        let mut state = state(&logs);
        assert_eq!(state.conversations.len(), 1);
        execute(&mut state, Command::Delete, &logs, scratch.path());
        assert_eq!(state.status, "deleted 1 file(s)");
        assert!(state.conversations.is_empty());
    }
}
