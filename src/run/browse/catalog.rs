use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::Result;
use crate::run::{clock, part_file};

pub struct Conversation {
    pub adapter: String,
    pub id: String,
    pub parts: Vec<PathBuf>,
    pub bytes: u64,
    pub last_ms: u64,
}

/// scans the given directory for conversations and returns a list of them
pub fn scan(dir: &Path) -> Result<Vec<Conversation>> {
    let mut grouped: BTreeMap<_, Vec<(u32, u64, u64, PathBuf)>> = BTreeMap::new();
    // read all the adapters configured
    let adapters = match fs::read_dir(dir) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        adapters => adapters?,
    };
    // for all adapters
    for adapter in adapters {
        let adapter = adapter?;
        if !adapter.file_type()?.is_dir() {
            // ensure its dir
            continue;
        }
        let Some(name) = adapter.file_name().to_str().map(str::to_owned) else {
            continue;
        };

        // read all the part files for this adapter
        for part in fs::read_dir(adapter.path())? {
            let part = part?;
            if !part.file_type()?.is_file() {
                continue;
            }
            let Some(file) = part.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some((id, number)) = part_file::parse(&name, &file) else {
                continue;
            };
            let metadata = part.metadata()?;
            let modified = metadata.modified().map_or(0, clock::millis);
            grouped
                .entry((name.clone(), id.to_owned()))
                .or_default()
                .push((number, metadata.len(), modified, part.path()));
        }
    }

    // transform the grouped parts into conversations
    let mut conversations: Vec<Conversation> = grouped
        .into_iter()
        .map(|((adapter, id), mut parts)| {
            parts.sort();
            // compute conversation from the raw data
            Conversation {
                adapter,
                id,
                bytes: parts.iter().map(|(_, bytes, _, _)| bytes).sum(),
                last_ms: parts
                    .iter()
                    .map(|(_, _, modified, _)| *modified)
                    .max()
                    .unwrap_or(0),
                parts: parts.into_iter().map(|(_, _, _, path)| path).collect(),
            }
        })
        .collect();
    conversations.sort_by(|a, b| {
        b.last_ms
            .cmp(&a.last_ms)
            .then_with(|| a.adapter.cmp(&b.adapter))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(conversations)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, UNIX_EPOCH};

    use super::scan;
    use crate::run::scratch_dir::ScratchDir;

    fn write(root: &Path, relative: &str, bytes: usize) {
        write_at(root, relative, bytes, 1);
    }

    fn write_at(root: &Path, relative: &str, bytes: usize, seconds: u64) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "x".repeat(bytes)).unwrap();
        let file = File::options().write(true).open(&path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn last_ms_is_the_newest_part_mtime_and_sorts_newest_first() {
        let root = ScratchDir::new();
        for (part, seconds) in [
            ("old_0001", 200),
            ("new_0001", 50),
            ("new_0002", 900),
            ("mid_0001", 300),
        ] {
            let relative = format!("claude/clawlog_claude_{part}.part.json");
            write_at(root.path(), &relative, 1, seconds);
        }
        let found = scan(root.path()).unwrap();
        let rows: Vec<(String, u64)> = found
            .iter()
            .map(|conversation| (conversation.id.clone(), conversation.last_ms))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("new".to_owned(), 900_000),
                ("mid".to_owned(), 300_000),
                ("old".to_owned(), 200_000),
            ]
        );
    }

    #[test]
    fn missing_dir_is_empty() {
        let root = ScratchDir::new();
        assert!(scan(&root.path().join("absent")).unwrap().is_empty());
        assert!(scan(root.path()).unwrap().is_empty());
    }

    #[test]
    fn groups_by_adapter_and_conversation_with_sizes() {
        let root = ScratchDir::new();
        write(root.path(), "claude/clawlog_claude_s1_0001.part.json", 10);
        write(root.path(), "claude/clawlog_claude_s1_0002.part.json", 5);
        write(root.path(), "claude/clawlog_claude_日本_0001.part.json", 2);
        write(root.path(), "other/clawlog_other_s1_0001.part.json", 7);
        let found = scan(root.path()).unwrap();
        let rows: Vec<(String, String, usize, u64)> = found
            .iter()
            .map(|conversation| {
                (
                    conversation.adapter.clone(),
                    conversation.id.clone(),
                    conversation.parts.len(),
                    conversation.bytes,
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                ("claude".to_owned(), "s1".to_owned(), 2, 15),
                ("claude".to_owned(), "日本".to_owned(), 1, 2),
                ("other".to_owned(), "s1".to_owned(), 1, 7),
            ]
        );
    }

    #[test]
    fn parts_are_ordered_numerically_including_five_digit_parts() {
        let root = ScratchDir::new();
        for part in [
            "clawlog_claude_s1_00010.part.json",
            "clawlog_claude_s1_0002.part.json",
            "clawlog_claude_s1_0001.part.json",
        ] {
            write(root.path(), &format!("claude/{part}"), 1);
        }
        let found = scan(root.path()).unwrap();
        let names: Vec<PathBuf> = found
            .iter()
            .flat_map(|conversation| conversation.parts.clone())
            .map(|path| PathBuf::from(path.file_name().unwrap()))
            .collect();
        assert_eq!(
            names,
            vec![
                PathBuf::from("clawlog_claude_s1_0001.part.json"),
                PathBuf::from("clawlog_claude_s1_0002.part.json"),
                PathBuf::from("clawlog_claude_s1_00010.part.json"),
            ]
        );
    }

    #[test]
    fn ignores_foreign_files_and_non_files() {
        let root = ScratchDir::new();
        write(root.path(), "README.md", 3);
        write(root.path(), "claude/README.md", 3);
        write(root.path(), "claude/clawlog_claude_s1_001.part.json", 3);
        write(root.path(), "claude/clawlog_other_s1_0001.part.json", 3);
        write(root.path(), "claude/.DS_Store", 3);
        fs::create_dir_all(root.path().join("claude/clawlog_claude_dir_0001.part.json")).unwrap();
        write(root.path(), "claude/clawlog_claude_s1_0001.part.json", 3);
        let found = scan(root.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found.first().map(|conversation| conversation.parts.len()),
            Some(1)
        );
    }
}
