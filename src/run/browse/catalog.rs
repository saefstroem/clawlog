use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{from_str, Value};

use super::Result;
use crate::run::part_file;

pub struct Conversation {
    pub adapter: String,
    pub id: String,
    pub parts: Vec<PathBuf>,
    pub bytes: u64,
    pub last_ms: u64,
}

/// scans the given directory for conversations and returns a list of them
pub fn scan(dir: &Path) -> Result<Vec<Conversation>> {
    let mut grouped: BTreeMap<_, Vec<(u32, u64, PathBuf)>> = BTreeMap::new();
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
            grouped
                .entry((name.clone(), id.to_owned()))
                .or_default()
                .push((number, part.metadata()?.len(), part.path()));
        }
    }

    // compute the last entry timestamp for a given part file
    let last_entry_ts = |path: &Path| -> Option<u64> {
        let mut file = File::open(path).ok()?;
        let len = file.metadata().ok()?.len();
        let mut window = 8 * 1024u64; // to prevent reading the entire file at once
        loop {
            // compute start position
            let start = len.saturating_sub(window);

            // seek to the start position and read the tail of the file
            file.seek(SeekFrom::Start(start)).ok()?;
            let mut tail = String::new();
            file.read_to_string(&mut tail).ok()?;
            let mut lines = tail.lines().rev();
            // get the first JSON entry from the tail of the file
            let entry = lines.find(|line| line.starts_with('{'));
            if let Some(line) = entry {
                // strip the trailing comma if present
                let line = line.strip_suffix(',').unwrap_or(line);
                if start == 0 || !tail.starts_with(line) {
                    // parse the JSON entry and extract the timestamp
                    let parsed: Value = from_str(line).ok()?;
                    return parsed.get("ts").and_then(Value::as_u64);
                }
            }
            // if no entry was found, increase the window and try again
            if start == 0 || window >= 4 * 1024 * 1024 {
                return None;
            }
            window *= 8;
        }
    };

    // transform the grouped parts into conversations
    let mut conversations: Vec<Conversation> = grouped
        .into_iter()
        .map(|((adapter, id), mut parts)| {
            parts.sort();
            // compute conversation from the raw data
            Conversation {
                adapter,
                id,
                bytes: parts.iter().map(|(_, bytes, _)| bytes).sum(),
                last_ms: parts
                    .last()
                    .and_then(|(_, _, path)| last_entry_ts(path))
                    .unwrap_or(0),
                parts: parts.into_iter().map(|(_, _, path)| path).collect(),
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
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::scan;
    use crate::run::scratch_dir::ScratchDir;

    fn write(root: &Path, relative: &str, bytes: usize) {
        write_text(root, relative, &"x".repeat(bytes));
    }

    fn write_text(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn last_ms_reads_the_newest_entry_ts_and_sorts_newest_first() {
        let root = ScratchDir::new();
        write_text(
            root.path(),
            "claude/clawlog_claude_old_0001.part.json",
            "[\n{\"ts\":100,\"text\":\"a\"},\n{\"ts\":200,\"text\":\"b\"}\n]\n",
        );
        write_text(
            root.path(),
            "claude/clawlog_claude_new_0001.part.json",
            "[\n{\"ts\":50}\n]\n",
        );
        write_text(
            root.path(),
            "claude/clawlog_claude_new_0002.part.json",
            "[\n{\"ts\":900}\n]\n",
        );
        let big_entry = format!("{{\"ts\":300,\"text\":\"{}\"}}", "y".repeat(20 * 1024));
        write_text(
            root.path(),
            "claude/clawlog_claude_big_0001.part.json",
            &format!("[\n{{\"ts\":1}},\n{big_entry}\n]\n"),
        );
        let found = scan(root.path()).unwrap();
        let rows: Vec<(String, u64)> = found
            .iter()
            .map(|conversation| (conversation.id.clone(), conversation.last_ms))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("new".to_owned(), 900),
                ("big".to_owned(), 300),
                ("old".to_owned(), 200),
            ]
        );
    }

    #[test]
    fn unreadable_tails_leave_last_ms_zero() {
        let root = ScratchDir::new();
        write_text(
            root.path(),
            "claude/clawlog_claude_junk_0001.part.json",
            "not json at all",
        );
        write(root.path(), "claude/clawlog_claude_empty_0001.part.json", 0);
        let found = scan(root.path()).unwrap();
        assert!(found.iter().all(|conversation| conversation.last_ms == 0));
        assert_eq!(found.len(), 2);
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
