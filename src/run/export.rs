use std::fs;
use std::path::Path;

use super::clear::part_files;
use super::{Error, Result};

/// Exports all part files from the log directory to the target directory.
/// Returns the number of part files copied.
pub fn export(log_dir: &Path, target: &Path) -> Result<usize> {
    let mut copied = 0;
    // iterate over each part file in the log directory and copy it to the target directory
    for file in part_files(log_dir)? {
        // determine the parent directory and file name of the part file
        let Some(parent) = file.parent() else {
            continue;
        };
        let (Some(adapter), Some(name)) = (parent.file_name(), file.file_name()) else {
            continue;
        };
        // create the corresponding directory in the target location
        let dir = target.join(adapter);
        fs::create_dir_all(&dir)?;
        // prevent exporting into the same directory as the log directory
        if fs::canonicalize(&dir)? == fs::canonicalize(parent)? {
            return Err(Error::ExportIntoLogDir);
        }
        // copy the part file to the target directory
        fs::copy(&file, dir.join(name))?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use super::export;
    use crate::run::scratch_dir::ScratchDir;

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn copies_part_files_per_adapter_and_overwrites() {
        let scratch = ScratchDir::new();
        let logs = scratch.path().join("logs");
        let target = scratch.path().join("out");
        write(
            &logs,
            "claude/clawlog_claude_s1_0001.part.json",
            "[\n1\n]\n",
        );
        write(
            &logs,
            "claude/clawlog_claude_s2_0001.part.json",
            "[\n2\n]\n",
        );
        write(&logs, "other/clawlog_other_x_0001.part.json", "[]");
        write(&logs, "claude/notes.txt", "no");
        write(&logs, "README.md", "no");
        write(&target, "claude/clawlog_claude_s1_0001.part.json", "stale");
        assert_eq!(export(&logs, &target).unwrap(), 3);
        assert_eq!(
            fs::read_to_string(target.join("claude/clawlog_claude_s1_0001.part.json")).unwrap(),
            "[\n1\n]\n"
        );
        assert_eq!(
            fs::read_to_string(target.join("claude/clawlog_claude_s2_0001.part.json")).unwrap(),
            "[\n2\n]\n"
        );
        assert_eq!(
            fs::read_to_string(target.join("other/clawlog_other_x_0001.part.json")).unwrap(),
            "[]"
        );
        assert!(!target.join("claude/notes.txt").exists());
        assert!(!target.join("README.md").exists());
        assert_eq!(
            fs::read_to_string(logs.join("claude/clawlog_claude_s1_0001.part.json")).unwrap(),
            "[\n1\n]\n"
        );
    }

    #[test]
    fn export_into_the_log_dir_errors_and_leaves_logs_intact() {
        let scratch = ScratchDir::new();
        let logs = scratch.path().join("logs");
        write(
            &logs,
            "claude/clawlog_claude_s1_0001.part.json",
            "[\n1\n]\n",
        );
        let link = scratch.path().join("link");
        symlink(&logs, &link).unwrap();
        assert!(export(&logs, &logs).is_err());
        assert!(export(&logs, &link).is_err());
        assert_eq!(
            fs::read_to_string(logs.join("claude/clawlog_claude_s1_0001.part.json")).unwrap(),
            "[\n1\n]\n"
        );
    }

    #[test]
    fn missing_log_dir_exports_nothing() {
        let scratch = ScratchDir::new();
        let target = scratch.path().join("out");
        assert_eq!(export(&scratch.path().join("gone"), &target).unwrap(), 0);
        assert!(!target.exists());
    }
}
