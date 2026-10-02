use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde_json::{from_str, Map, Value};

use super::hooks::commands;
use super::save::save;
use super::{Error, Result};
use crate::run::adapter::Installed;

/// Installs the hooks for claude by updating the settings.json file in the specified home directory.
pub fn install(home: &Path, config: &Value) -> Result<Installed> {
    let dir = home.join(".claude");
    let path = dir.join("settings.json");
    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let mut settings: Value = match &original {
        Some(text) => from_str(text)?,
        None => Value::Object(Map::new()),
    };

    // Ensure the hooks section exists in the settings JSON
    let hooks = settings
        .as_object_mut()
        .ok_or(Error::NotObject("the root"))?
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(Error::NotObject("\"hooks\""))?;
    let mut installed = Installed {
        path: path.clone(),
        added: 0,
        present: 0,
    };

    // Iterate over each hook group in the configuration and install it if not already present
    for (event, groups) in config
        .get("hooks")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        // Get or create the array of existing hooks for this event
        let existing = hooks
            .entry(event.as_str())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| Error::NotArray(event.clone()))?;
        for group in groups.as_array().into_iter().flatten() {
            // Check if the current group of hooks is already present in the existing hooks
            let present = commands(group).any(|ours| {
                existing
                    .iter()
                    .flat_map(commands)
                    .any(|theirs| theirs == ours)
            });
            if present {
                installed.present += 1;
            } else {
                existing.push(group.clone());
                installed.added += 1;
            }
        }
    }

    // Save the updated settings JSON if any new hooks were added
    if installed.added > 0 {
        save(&dir, &path, original.is_some(), &settings)?;
    }
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};
    #[cfg(target_os = "macos")]
    use std::process::Command;

    use serde_json::{from_str, json, Value};

    use super::install;
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
    fn creates_settings_when_missing_without_backup() {
        let home = ScratchDir::new();
        let installed = install(home.path(), &config()).unwrap();
        assert_eq!((installed.added, installed.present), (2, 0));
        assert_eq!(installed.path, settings_path(home.path()));
        assert_eq!(read_settings(home.path()), config());
        assert!(!home.path().join(".claude/settings.json.bak").exists());
        assert!(fs::read_to_string(settings_path(home.path()))
            .unwrap()
            .ends_with("}\n"));
        let metadata = fs::metadata(settings_path(home.path())).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn second_install_adds_nothing_and_leaves_the_file_alone() {
        let home = ScratchDir::new();
        install(home.path(), &config()).unwrap();
        let first = fs::read_to_string(settings_path(home.path())).unwrap();
        let again = install(home.path(), &config()).unwrap();
        assert_eq!((again.added, again.present), (0, 2));
        assert_eq!(
            fs::read_to_string(settings_path(home.path())).unwrap(),
            first
        );
        assert!(!home.path().join(".claude/settings.json.bak").exists());
    }

    #[test]
    fn merges_into_existing_settings_and_writes_a_backup() {
        let home = ScratchDir::new();
        let original = r#"{"model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"theirs"}]}],"PreToolUse":[{"hooks":[{"type":"command","command":"ours","timeout":30}]}]}}"#;
        write_settings(home.path(), original);
        fs::set_permissions(settings_path(home.path()), Permissions::from_mode(0o600)).unwrap();
        let installed = install(home.path(), &config()).unwrap();
        assert_eq!((installed.added, installed.present), (1, 1));
        for name in ["settings.json", "settings.json.bak"] {
            let metadata = fs::metadata(home.path().join(".claude").join(name)).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600, "{name}");
        }
        assert_eq!(
            read_settings(home.path()),
            json!({"model": "opus", "hooks": {
                "Stop": [
                    {"hooks": [{"type": "command", "command": "theirs"}]},
                    {"hooks": [{"type": "command", "command": "ours"}]}
                ],
                "PreToolUse": [{"hooks": [{"type": "command", "command": "ours", "timeout": 30}]}]
            }})
        );
        assert_eq!(
            fs::read_to_string(home.path().join(".claude/settings.json.bak")).unwrap(),
            original
        );
        let mut names: Vec<_> = fs::read_dir(home.path().join(".claude"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["settings.json", "settings.json.bak"]);
    }

    #[test]
    fn reinstalls_over_a_read_only_settings_file_and_its_backup() {
        let home = ScratchDir::new();
        let read_only = || {
            fs::set_permissions(settings_path(home.path()), Permissions::from_mode(0o444)).unwrap()
        };
        write_settings(home.path(), r#"{"model":"opus"}"#);
        read_only();
        install(home.path(), &config()).unwrap();
        let mut settings = read_settings(home.path());
        settings["hooks"].as_object_mut().unwrap().remove("Stop");
        fs::set_permissions(settings_path(home.path()), Permissions::from_mode(0o644)).unwrap();
        fs::write(settings_path(home.path()), settings.to_string()).unwrap();
        read_only();
        let again = install(home.path(), &config()).unwrap();
        assert_eq!((again.added, again.present), (1, 1));
        assert_eq!(read_settings(home.path())["hooks"], config()["hooks"]);
        let backup = fs::read_to_string(home.path().join(".claude/settings.json.bak")).unwrap();
        assert_eq!(from_str::<Value>(&backup).unwrap(), settings);
        let metadata = fs::metadata(settings_path(home.path())).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o444);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn removes_the_temp_file_when_the_rename_fails() {
        let home = ScratchDir::new();
        let original = r#"{"model":"opus"}"#;
        write_settings(home.path(), original);
        let chflags = |flags: &str, path: &Path| {
            let status = Command::new("chflags")
                .args(["-R", flags])
                .arg(path)
                .status();
            assert!(status.unwrap().success());
        };
        chflags("uchg", &settings_path(home.path()));
        let result = install(home.path(), &config());
        chflags("nouchg", &home.path().join(".claude"));
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(settings_path(home.path())).unwrap(),
            original
        );
        let mut names: Vec<_> = fs::read_dir(home.path().join(".claude"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["settings.json", "settings.json.bak"]);
    }

    #[test]
    fn writes_through_a_symlinked_settings_file() {
        let home = ScratchDir::new();
        let target = home.path().join("dotfiles.json");
        fs::write(&target, r#"{"model":"opus"}"#).unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        symlink(&target, settings_path(home.path())).unwrap();
        install(home.path(), &config()).unwrap();
        let link = fs::symlink_metadata(settings_path(home.path())).unwrap();
        assert!(link.file_type().is_symlink());
        let merged: Value = from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(merged["model"], "opus");
        assert_eq!(merged["hooks"], config()["hooks"]);
    }

    #[test]
    fn creates_the_missing_target_of_a_dangling_symlink() {
        let home = ScratchDir::new();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::create_dir_all(home.path().join("dotfiles")).unwrap();
        symlink("../dotfiles/settings.json", settings_path(home.path())).unwrap();
        install(home.path(), &config()).unwrap();
        let link = fs::symlink_metadata(settings_path(home.path())).unwrap();
        assert!(link.file_type().is_symlink());
        let created = home.path().join("dotfiles/settings.json");
        assert_eq!(
            from_str::<Value>(&fs::read_to_string(created).unwrap()).unwrap(),
            config()
        );
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
            assert!(install(home.path(), &config()).is_err(), "{original}");
            assert_eq!(
                fs::read_to_string(settings_path(home.path())).unwrap(),
                original
            );
            assert!(!home.path().join(".claude/settings.json.bak").exists());
        }
    }
}
