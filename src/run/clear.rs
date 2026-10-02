use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::{part_file, Result};

/// Returns a list of all part files under the given log directory.
pub fn part_files(log_dir: &Path) -> Result<Vec<PathBuf>> {
    // read the list of adapter directories under the log directory
    let adapters = match fs::read_dir(log_dir) {
        Ok(adapters) => adapters,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut files = Vec::new();

    // iterate over each adapter directory and collect part files
    for adapter in adapters {
        let adapter = adapter?;
        if !adapter.file_type()?.is_dir() {
            continue;
        }
        let dir_name = adapter.file_name();
        let Some(name) = dir_name.to_str() else {
            continue;
        };

        // read the list of entries in the adapter directory
        for entry in fs::read_dir(adapter.path())? {
            let entry = entry?;
            // check if the entry is a part-shaped file
            let part_shaped = entry
                .file_name()
                .to_str()
                .is_some_and(|file| part_file::parse(name, file).is_some());

            // if the entry is part-shaped and a regular file, include it in the list
            if part_shaped && entry.file_type()?.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Deletes all part files under the given log directory and prunes empty adapter directories.
/// Returns the number of part files deleted.
pub fn clear(log_dir: &Path) -> Result<usize> {
    let files = part_files(log_dir)?;
    for file in &files {
        fs::remove_file(file)?;
    }
    if let Ok(adapters) = fs::read_dir(log_dir) {
        for adapter in adapters.flatten() {
            // attempt to remove the adapter directory if it is empty
            let _ = fs::remove_dir(adapter.path());
        }
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use super::{clear, part_files};
    use crate::run::scratch_dir::ScratchDir;

    fn populate(root: &Path) -> (PathBuf, PathBuf) {
        let claude = root.join("claude");
        let other = root.join("other");
        fs::create_dir_all(&claude).unwrap();
        fs::create_dir_all(&other).unwrap();
        for name in [
            "clawlog_claude_s1_0001.part.json",
            "clawlog_claude_s1_0002.part.json",
        ] {
            fs::write(claude.join(name), "[\n1\n]\n").unwrap();
        }
        fs::write(other.join("clawlog_other_x_0001.part.json"), "[]").unwrap();
        fs::write(claude.join("notes.txt"), "keep").unwrap();
        fs::write(root.join("README.md"), "keep").unwrap();
        (claude, other)
    }

    #[test]
    fn lists_only_part_shaped_regular_files_under_adapter_dirs() {
        let root = ScratchDir::new();
        let (claude, other) = populate(root.path());
        symlink(
            "/dev/null",
            claude.join("clawlog_claude_link_0001.part.json"),
        )
        .unwrap();
        fs::create_dir_all(claude.join("clawlog_claude_dir_0001.part.json")).unwrap();
        assert_eq!(
            part_files(root.path()).unwrap(),
            vec![
                claude.join("clawlog_claude_s1_0001.part.json"),
                claude.join("clawlog_claude_s1_0002.part.json"),
                other.join("clawlog_other_x_0001.part.json"),
            ]
        );
    }

    #[test]
    fn clear_deletes_parts_prunes_empty_adapter_dirs_and_keeps_the_rest() {
        let root = ScratchDir::new();
        let (claude, other) = populate(root.path());
        assert_eq!(clear(root.path()).unwrap(), 3);
        assert!(!other.exists());
        assert!(claude.join("notes.txt").exists());
        assert!(root.path().join("README.md").exists());
        assert!(part_files(root.path()).unwrap().is_empty());
        assert_eq!(clear(root.path()).unwrap(), 0);
        assert!(root.path().exists());
    }

    #[test]
    fn missing_log_dir_is_empty_not_an_error() {
        let root = ScratchDir::new();
        let missing = root.path().join("gone");
        assert!(part_files(&missing).unwrap().is_empty());
        assert_eq!(clear(&missing).unwrap(), 0);
    }
}
