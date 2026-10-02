use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde_json::{from_str, Value};

use super::hooks::commands;
use super::save::save;
use super::{Error, Result};
use crate::run::adapter::Removed;

/// Uninstalls the hooks for claude by removing them from the settings.json
/// file in the specified home directory.
pub fn uninstall(home: &Path, config: &Value) -> Result<Removed> {
    let dir = home.join(".claude");
    let path = dir.join("settings.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(Removed { path, removed: 0 });
        }
        Err(error) => return Err(error.into()),
    };
    let mut settings: Value = from_str(&text)?;
    let root = settings
        .as_object_mut()
        .ok_or(Error::NotObject("the root"))?;

    // Get hooks that we have configured
    let ours: Vec<&Value> = config
        .get("hooks")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .flat_map(|(_, groups)| groups.as_array().into_iter().flatten())
        .flat_map(commands)
        .collect();
    let mut removed = 0;
    if let Some(existing) = root.get_mut("hooks") {
        // try get hooks value in settings
        let events = existing
            .as_object_mut()
            .ok_or(Error::NotObject("\"hooks\""))?;

        // Iterate over each event and its associated groups of hooks
        for (event, groups) in events.iter_mut() {
            let groups = groups
                .as_array_mut()
                .ok_or_else(|| Error::NotArray(event.clone()))?;
            for group in groups.iter_mut() {
                if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    let before = hooks.len();
                    hooks.retain(|hook| {
                        !hook
                            .get("command")
                            .is_some_and(|command| ours.contains(&command))
                    });
                    removed += before - hooks.len();
                }
            }
            groups.retain(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|hooks| !hooks.is_empty())
            });
        }
        events.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
        if events.is_empty() {
            root.remove("hooks");
        }
    }
    if removed > 0 {
        save(&dir, &path, true, &settings)?;
    }
    Ok(Removed { path, removed })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde_json::{from_str, json, Value};

    use super::super::hooks::hooks;
    use super::super::install::install;
    use super::uninstall;
    use crate::run::scratch_dir::ScratchDir;

    fn config() -> Value {
        json!({"hooks": {
            "Stop": [{"hooks": [{"type": "command", "command": "ours"}]}],
            "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "ours"}]}]
        }})
    }

    fn settings_path(home: &Path) -> PathBuf {
        home.join(".claude").join("settings.json")
    }

    fn write_settings(home: &Path, text: &str) {
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(settings_path(home), text).unwrap();
    }

    fn read_settings(home: &Path) -> Value {
        from_str(&fs::read_to_string(settings_path(home)).unwrap()).unwrap()
    }

    #[test]
    fn removes_our_hooks_and_keeps_foreign_ones_in_the_same_event() {
        let home = ScratchDir::new();
        let original = json!({"model": "opus", "hooks": {
            "Stop": [{"hooks": [{"command": "theirs"}, {"type": "command", "command": "ours"}]}],
            "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "ours"}]}],
            "SessionStart": [{"hooks": [{"command": "other"}]}]
        }});
        write_settings(home.path(), &original.to_string());
        let removed = uninstall(home.path(), &config()).unwrap();
        assert_eq!(removed.removed, 2);
        assert_eq!(removed.path, settings_path(home.path()));
        assert_eq!(
            read_settings(home.path()),
            json!({"model": "opus", "hooks": {
                "Stop": [{"hooks": [{"command": "theirs"}]}],
                "SessionStart": [{"hooks": [{"command": "other"}]}]
            }})
        );
        let backup = home.path().join(".claude/settings.json.bak");
        assert_eq!(
            from_str::<Value>(&fs::read_to_string(backup).unwrap()).unwrap(),
            original
        );
    }

    #[test]
    fn round_trips_with_install_and_prunes_the_hooks_object() {
        let home = ScratchDir::new();
        write_settings(home.path(), r#"{"model":"opus"}"#);
        install(home.path(), &hooks()).unwrap();
        let removed = uninstall(home.path(), &hooks()).unwrap();
        assert_eq!(removed.removed, 4);
        assert_eq!(read_settings(home.path()), json!({"model": "opus"}));
    }

    #[test]
    fn second_uninstall_removes_nothing_and_leaves_the_file_alone() {
        let home = ScratchDir::new();
        install(home.path(), &config()).unwrap();
        uninstall(home.path(), &config()).unwrap();
        let after_first = fs::read_to_string(settings_path(home.path())).unwrap();
        let again = uninstall(home.path(), &config()).unwrap();
        assert_eq!(again.removed, 0);
        assert_eq!(
            fs::read_to_string(settings_path(home.path())).unwrap(),
            after_first
        );
        let mut names: Vec<_> = fs::read_dir(home.path().join(".claude"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["settings.json", "settings.json.bak"]);
    }

    #[test]
    fn missing_settings_file_removes_nothing_and_writes_nothing() {
        let home = ScratchDir::new();
        let removed = uninstall(home.path(), &config()).unwrap();
        assert_eq!(removed.removed, 0);
        assert_eq!(removed.path, settings_path(home.path()));
        assert!(!home.path().join(".claude").exists());
    }

    #[test]
    fn refuses_invalid_settings_without_touching_them() {
        for original in [
            "{not json",
            "[]",
            r#"{"hooks":[]}"#,
            r#"{"hooks":{"Stop":{}}}"#,
        ] {
            let home = ScratchDir::new();
            write_settings(home.path(), original);
            assert!(uninstall(home.path(), &config()).is_err(), "{original}");
            assert_eq!(
                fs::read_to_string(settings_path(home.path())).unwrap(),
                original
            );
            assert!(!home.path().join(".claude/settings.json.bak").exists());
        }
    }
}
