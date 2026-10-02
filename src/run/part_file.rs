use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::{Error, Result};

pub fn append(
    root: &Path,
    adapter: &str,
    conversation: Option<&str>,
    max_bytes: u64,
    entry: &str,
) -> Result<()> {
    // sanitize the conversation ID to create a safe filename prefix
    let sanitize = |conversation: Option<&str>| {
        let id: String = conversation
            .unwrap_or_default()
            .chars()
            .take(128)
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        if id.is_empty() {
            "unknown".to_owned()
        } else {
            id
        }
    };

    let current_part = |dir: &Path, prefix: &str| -> Result<u32> {
        let mut highest = 1; // assume 1 is the highest initially
                             // for each file in the dir
        for entry in fs::read_dir(dir)? {
            let name = entry?.file_name();

            // remove prefix and suffix to extract the part number
            let number = name
                .to_str()
                .and_then(|name| name.strip_prefix(prefix))
                .and_then(|rest| rest.strip_suffix(".part.json"))
                .filter(|digits| digits.len() >= 4 && digits.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|digits| digits.parse().ok());
            highest = highest.max(number.unwrap_or(0));
        }
        Ok(highest)
    };

    let append_to = |path: &Path, entry: &str| -> Result<bool> {
        // read file
        let mut file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
        {
            Err(_) if path.symlink_metadata().is_ok() => return Ok(false),
            file => file?,
        };
        if !file.metadata()?.is_file() {
            return Ok(false);
        }

        // acquire lock to the file so that other log attempts dont override
        file.lock()?;
        let len = file.metadata()?.len();

        // if len 0 then this is a new file
        if len == 0 {
            if let Err(error) = file.write_all(format!("[\n{entry}\n]\n").as_bytes()) {
                let _ = file.set_len(0);
                return Err(error.into());
            }
            return Ok(true);
        }

        // we never write less than 3 bytes to the file
        // so if its less than 3 bytes, we consider it invalid
        if len >= max_bytes || len < 3 {
            return Ok(false);
        }

        // as you can see our tail is at least 3 bytes
        let mut tail = [0; 3];
        file.seek(SeekFrom::Start(len - 3))?;
        file.read_exact(&mut tail)?;

        // check if the tail matches the expected ending of a valid part file
        if &tail != b"\n]\n" {
            return Ok(false);
        }

        // append the new entry before the closing bracket
        file.seek(SeekFrom::Start(len - 3))?;

        // write the new entry with a preceding comma and newline
        if let Err(error) = file.write_all(format!(",\n{entry}\n]\n").as_bytes()) {
            let _ = file.set_len(len);
            let _ = file.seek(SeekFrom::Start(len - 3));
            let _ = file.write_all(b"\n]\n");
            return Err(error.into());
        }
        Ok(true)
    };

    // find the dir of the adapter
    let dir = root.join(adapter);

    // upsert the dir
    fs::create_dir_all(&dir)?;
    let prefix = format!("clawlog_{adapter}_{}_", sanitize(conversation));
    let mut part = current_part(&dir, &prefix)?;

    // write the log by ensuring the append succeeds
    while !append_to(&dir.join(format!("{prefix}{part:04}.part.json")), entry)? {
        part = part.checked_add(1).ok_or(Error::PartsExhausted)?;
    }
    Ok(())
}

/// Parses a part file name into its conversation and part number components.
pub fn parse<'n>(adapter: &str, name: &'n str) -> Option<(&'n str, u32)> {
    // strip the prefix and suffix to isolate the conversation and part number
    let rest = name
        .strip_prefix("clawlog_")?
        .strip_prefix(adapter)?
        .strip_prefix('_')?
        .strip_suffix(".part.json")?;
    // split the remaining string into the conversation and part number components
    let (conversation, digits) = rest.rsplit_once('_')?;
    // validate the extracted components
    if adapter.is_empty()
        || conversation.is_empty()
        || digits.len() < 4
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    digits.parse().ok().map(|part| (conversation, part))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::thread;

    use serde_json::{from_str, json, Value};

    use super::{append, parse};
    use crate::run::scratch_dir::ScratchDir;

    fn part(root: &Path, id: &str, number: u32) -> PathBuf {
        root.join("claude")
            .join(format!("clawlog_claude_{id}_{number:04}.part.json"))
    }

    fn entries(path: &Path) -> Vec<Value> {
        from_str::<Value>(&fs::read_to_string(path).unwrap())
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn new_part_is_a_one_entry_array_and_appends_splice_before_the_bracket() {
        let root = ScratchDir::new();
        append(root.path(), "claude", Some("s1"), 1 << 20, r#"{"n":1}"#).unwrap();
        let path = part(root.path(), "s1", 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), "[\n{\"n\":1}\n]\n");
        append(root.path(), "claude", Some("s1"), 1 << 20, r#"{"n":2}"#).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[\n{\"n\":1},\n{\"n\":2}\n]\n"
        );
        assert_eq!(entries(&path), vec![json!({"n": 1}), json!({"n": 2})]);
    }

    #[test]
    fn rotates_to_the_next_part_once_the_cap_is_reached() {
        let root = ScratchDir::new();
        for n in 0..5 {
            append(
                root.path(),
                "claude",
                Some("s1"),
                21,
                &json!({"n": n}).to_string(),
            )
            .unwrap();
        }
        let parts: Vec<Vec<Value>> = (1..=3)
            .map(|n| entries(&part(root.path(), "s1", n)))
            .collect();
        assert_eq!(
            parts,
            vec![
                vec![json!({"n": 0}), json!({"n": 1})],
                vec![json!({"n": 2}), json!({"n": 3})],
                vec![json!({"n": 4})],
            ]
        );
        assert!(!part(root.path(), "s1", 4).exists());
        for n in 0..2 {
            append(
                root.path(),
                "claude",
                Some("s2"),
                13,
                &json!({"n": n}).to_string(),
            )
            .unwrap();
        }
        assert_eq!(
            entries(&part(root.path(), "s2", 1)),
            vec![json!({"n": 0}), json!({"n": 1})]
        );
    }

    #[test]
    fn zero_cap_writes_one_entry_per_part() {
        let root = ScratchDir::new();
        for n in 0..3 {
            append(root.path(), "claude", Some("s1"), 0, &n.to_string()).unwrap();
        }
        for n in 0..3 {
            assert_eq!(entries(&part(root.path(), "s1", n + 1)), vec![json!(n)]);
        }
    }

    #[test]
    fn corrupt_tail_moves_on_and_leaves_the_file_untouched() {
        let root = ScratchDir::new();
        fs::create_dir_all(root.path().join("claude")).unwrap();
        for (id, junk) in [
            ("a", "[\n{\"n\":1}\n"),
            ("b", "x"),
            ("c", "not json at all"),
            ("d", "[1]\n"),
        ] {
            fs::write(part(root.path(), id, 1), junk).unwrap();
            append(root.path(), "claude", Some(id), 1 << 20, "{}").unwrap();
            assert_eq!(fs::read_to_string(part(root.path(), id, 1)).unwrap(), junk);
            assert_eq!(entries(&part(root.path(), id, 2)), vec![json!({})]);
        }
    }

    #[test]
    fn last_part_number_full_is_an_error_not_a_wraparound() {
        let root = ScratchDir::new();
        fs::create_dir_all(root.path().join("claude")).unwrap();
        fs::write(part(root.path(), "s1", u32::MAX), "x").unwrap();
        let error = append(root.path(), "claude", Some("s1"), 1 << 20, "{}").unwrap_err();
        assert_eq!(
            error.to_string(),
            "no part number left for this conversation"
        );
        assert!(!part(root.path(), "s1", 0).exists());
    }

    #[test]
    fn unopenable_part_is_skipped_and_left_untouched() {
        let root = ScratchDir::new();
        fs::create_dir_all(part(root.path(), "s1", 3)).unwrap();
        fs::write(part(root.path(), "s1", 1), "[\n1\n]\n").unwrap();
        append(root.path(), "claude", Some("s1"), 1 << 20, "2").unwrap();
        assert_eq!(entries(&part(root.path(), "s1", 1)), vec![json!(1)]);
        assert_eq!(entries(&part(root.path(), "s1", 4)), vec![json!(2)]);
        let read_only = part(root.path(), "s2", 1);
        fs::write(&read_only, "[\n1\n]\n").unwrap();
        let mut permissions = fs::metadata(&read_only).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&read_only, permissions).unwrap();
        append(root.path(), "claude", Some("s2"), 1 << 20, "2").unwrap();
        assert_eq!(entries(&read_only), vec![json!(1)]);
        assert_eq!(entries(&part(root.path(), "s2", 2)), vec![json!(2)]);
    }

    #[test]
    fn non_regular_part_is_skipped_and_left_untouched() {
        let root = ScratchDir::new();
        fs::create_dir_all(root.path().join("claude")).unwrap();
        symlink("/dev/null", part(root.path(), "s1", 1)).unwrap();
        append(root.path(), "claude", Some("s1"), 1 << 20, "1").unwrap();
        append(root.path(), "claude", Some("s1"), 1 << 20, "2").unwrap();
        assert_eq!(
            entries(&part(root.path(), "s1", 2)),
            vec![json!(1), json!(2)]
        );
    }

    #[test]
    fn failed_write_restores_the_part() {
        let root = ScratchDir::new();
        fs::create_dir_all(root.path().join("claude")).unwrap();
        fs::write(part(root.path(), "s1", 1), "[\n1\n]\n").unwrap();
        let status = Command::new("sh")
            .arg("-c")
            .arg("trap '' XFSZ; ulimit -f 1; exec \"$0\" --exact --ignored run::part_file::tests::write_past_the_file_size_limit")
            .arg(env::current_exe().unwrap())
            .env("CLAWLOG_TEST_ROOT", root.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            fs::read_to_string(part(root.path(), "s1", 1)).unwrap(),
            "[\n1\n]\n"
        );
        assert_eq!(fs::read_to_string(part(root.path(), "s2", 1)).unwrap(), "");
    }

    #[test]
    #[ignore]
    fn write_past_the_file_size_limit() {
        let Some(root) = env::var_os("CLAWLOG_TEST_ROOT").map(PathBuf::from) else {
            return;
        };
        let big = "0".repeat(1 << 16);
        assert!(append(&root, "claude", Some("s1"), 1 << 20, &big).is_err());
        assert!(append(&root, "claude", Some("s2"), 1 << 20, &big).is_err());
    }

    #[test]
    fn unwritable_log_directory_is_an_error() {
        let root = ScratchDir::new();
        let dir = root.path().join("claude");
        fs::create_dir_all(&dir).unwrap();
        let mut permissions = fs::metadata(&dir).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&dir, permissions).unwrap();
        assert!(append(root.path(), "claude", Some("s1"), 1 << 20, "2").is_err());
        assert!(!part(root.path(), "s1", 1).exists());
    }

    #[test]
    fn continues_from_the_highest_existing_part() {
        let root = ScratchDir::new();
        fs::create_dir_all(root.path().join("claude")).unwrap();
        for n in [1, 2, 10, 4, 12, 6] {
            fs::write(part(root.path(), "s1", n), "[\n1\n]\n").unwrap();
        }
        append(root.path(), "claude", Some("s1"), 1 << 20, "2").unwrap();
        assert_eq!(
            entries(&part(root.path(), "s1", 12)),
            vec![json!(1), json!(2)]
        );
    }

    #[test]
    fn part_scan_matches_the_exact_conversation_prefix() {
        let root = ScratchDir::new();
        let dir = root.path().join("claude");
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "clawlog_claude_a_0001_0009.part.json",
            "clawlog_claude_a_009.part.json",
            "clawlog_claude_a_0005.part.json.tmp",
            "clawlog_claude_ab_0008.part.json",
            "clawlog_claude_a_00x7.part.json",
            "clawlog_claude_a_+0012.part.json",
            "clawlog_claude_a_0003.part.json",
        ] {
            fs::write(dir.join(name), "").unwrap();
        }
        append(root.path(), "claude", Some("a"), 1 << 20, "1").unwrap();
        assert_eq!(entries(&part(root.path(), "a", 3)), vec![json!(1)]);
        append(root.path(), "claude", Some("a_0001"), 1 << 20, "2").unwrap();
        assert_eq!(entries(&part(root.path(), "a_0001", 9)), vec![json!(2)]);
        append(root.path(), "claude", Some("zzz"), 1 << 20, "3").unwrap();
        assert_eq!(entries(&part(root.path(), "zzz", 1)), vec![json!(3)]);
    }

    #[test]
    fn parses_part_file_names() {
        assert_eq!(
            parse("claude", "clawlog_claude_s1_0001.part.json"),
            Some(("s1", 1))
        );
        assert_eq!(
            parse("claude", "clawlog_claude_a_0001_0009.part.json"),
            Some(("a_0001", 9))
        );
        assert_eq!(
            parse("claude", "clawlog_claude_日本_00012.part.json"),
            Some(("日本", 12))
        );
        assert_eq!(
            parse("other", "clawlog_other_x-1.y_9999.part.json"),
            Some(("x-1.y", 9999))
        );
        for name in [
            "clawlog_claude_a_009.part.json",
            "clawlog_claude_a_00x7.part.json",
            "clawlog_claude_a_+0012.part.json",
            "clawlog_claude_a_0001.part.json.tmp",
            "clawlog_claude_a_0001.part.jsonx",
            "clawlog_other_a_0001.part.json",
            "clawlog_claude_0001.part.json",
            "clawlog_claude__0001.part.json",
            "x_claude_a_0001.part.json",
            "clawlog_claude_a_99999999999.part.json",
            "README.md",
        ] {
            assert_eq!(parse("claude", name), None, "{name}");
        }
        assert_eq!(parse("", "clawlog__a_0001.part.json"), None);
    }

    #[test]
    fn sanitizes_conversation_ids() {
        let root = ScratchDir::new();
        for (raw, cleaned) in [
            (Some("3f2a-9c.1_e"), "3f2a-9c.1_e".to_owned()),
            (Some("../etc/passwd"), "..-etc-passwd".to_owned()),
            (Some("a\\b c/é"), "a-b-c--".to_owned()),
            (Some("x".repeat(200).as_str()), "x".repeat(128)),
            (Some(""), "unknown".to_owned()),
            (None, "unknown".to_owned()),
        ] {
            append(root.path(), "claude", raw, 1 << 20, "{}").unwrap();
            assert!(part(root.path(), &cleaned, 1).exists(), "{raw:?}");
        }
    }

    #[test]
    fn parallel_appends_keep_one_valid_array() {
        let root = ScratchDir::new();
        let threads: Vec<_> = (0..16)
            .map(|n| {
                let root = root.path().to_path_buf();
                thread::spawn(move || {
                    append(
                        &root,
                        "claude",
                        Some("s1"),
                        1 << 20,
                        &json!({"n": n}).to_string(),
                    )
                    .unwrap()
                })
            })
            .collect();
        for handle in threads {
            handle.join().unwrap();
        }
        let mut seen: Vec<i64> = entries(&part(root.path(), "s1", 1))
            .iter()
            .map(|entry| entry["n"].as_i64().unwrap())
            .collect();
        seen.sort();
        assert_eq!(seen, (0..16).collect::<Vec<i64>>());
    }
}
