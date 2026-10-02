use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process;

use serde_json::{to_string_pretty, Value};

use super::Result;

/// Saves the updated settings JSON to the specified path, optionally creating a backup of the existing file.
pub fn save(dir: &Path, path: &Path, backup: bool, settings: &Value) -> Result<()> {
    fs::create_dir_all(dir)?;
    if backup {
        let bak = dir.join("settings.json.bak");
        let _ = fs::remove_file(&bak);
        fs::copy(path, bak)?;
    }

    // Determine the target path for the settings file, resolving symlinks if necessary
    let target = fs::canonicalize(path)
        .or_else(|_| fs::read_link(path).map(|link| dir.join(link)))
        .unwrap_or_else(|_| path.to_path_buf());

    // Create a temporary file for writing the new settings before replacing the target file
    let temp = target.with_file_name(format!("settings.json.clawlog-{}", process::id()));
    let mut options = OpenOptions::new();
    // overwrite the target file with the new settings
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let text = to_string_pretty(settings)? + "\n";
    let mut file = options.open(&temp)?;
    let replaced = file.write_all(text.as_bytes()).and_then(|()| {
        if backup {
            file.set_permissions(fs::metadata(&target)?.permissions())?;
        }
        fs::rename(&temp, &target)
    });
    if replaced.is_err() {
        let _ = fs::remove_file(&temp);
    }
    Ok(replaced?)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::save;
    use crate::run::scratch_dir::ScratchDir;

    fn paths(home: &Path) -> (PathBuf, PathBuf) {
        let dir = home.join(".claude");
        let path = dir.join("settings.json");
        (dir, path)
    }

    #[test]
    fn creates_the_file_mode_600_with_a_trailing_newline() {
        let home = ScratchDir::new();
        let (dir, path) = paths(home.path());
        save(&dir, &path, false, &json!({"a": 1})).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\n  \"a\": 1\n}\n");
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(!dir.join("settings.json.bak").exists());
    }

    #[test]
    fn backs_up_and_preserves_the_permissions_of_an_existing_file() {
        let home = ScratchDir::new();
        let (dir, path) = paths(home.path());
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o640)).unwrap();
        save(&dir, &path, true, &json!({"a": 1})).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("settings.json.bak")).unwrap(),
            "old"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\n  \"a\": 1\n}\n");
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
}
