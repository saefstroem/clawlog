use std::fs;
use std::path::PathBuf;

use serde_json::{from_str, Value};

use super::Result;

/// loads all entries from the given part files
pub fn load(parts: &[PathBuf]) -> Result<Vec<Value>> {
    let mut entries = Vec::new();
    for part in parts {
        let mut list: Vec<Value> = from_str(&fs::read_to_string(part)?)?;
        entries.append(&mut list);
    }
    Ok(entries)
}

/// renders a single line for the given entry with an index
pub fn line(index: usize, entry: &Value) -> String {
    // helper to generate a preview of the entry's text content
    let preview = |entry: &Value| -> String {
        let raw = match entry.get("text") {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Null) | None => match (
                entry.get("tool").and_then(Value::as_str),
                entry.get("input").and_then(Value::as_str),
            ) {
                (Some(tool), Some(input)) => format!("{tool} {input}"),
                (Some(tool), None) => tool.to_owned(),
                _ => String::new(),
            },
            Some(other) => other.to_string(),
        };
        raw.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(200)
            .collect()
    };
    // helper to extract a specific field from the entry
    let field = |key: &str| match entry.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };

    // render the line for the entry
    format!(
        "{index:>4}  {}  {:<9}  {:<11}  {}",
        field("ts"),
        field("role"),
        field("kind"),
        preview(entry)
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::{json, Value};

    use super::{line, load};
    use crate::run::scratch_dir::ScratchDir;

    fn part(root: &ScratchDir, name: &str, content: &str) -> PathBuf {
        let path = root.path().join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn loads_entries_across_parts_in_order() {
        let root = ScratchDir::new();
        let first = part(&root, "a", "[\n{\"n\":1},\n{\"n\":2}\n]\n");
        let second = part(&root, "b", "[\n{\"n\":3}\n]\n");
        assert_eq!(
            load(&[first, second]).unwrap(),
            vec![json!({"n": 1}), json!({"n": 2}), json!({"n": 3})]
        );
        assert_eq!(load(&[]).unwrap(), Vec::<Value>::new());
    }

    #[test]
    fn corrupt_or_non_array_parts_are_errors() {
        let root = ScratchDir::new();
        let corrupt = part(&root, "a", "[\n{\"n\":1}\n");
        let object = part(&root, "b", "{}");
        let missing = root.path().join("absent");
        assert!(load(&[corrupt])
            .unwrap_err()
            .to_string()
            .starts_with("invalid part file:"));
        assert!(load(&[object]).is_err());
        assert!(load(&[missing]).is_err());
    }

    #[test]
    fn lines_show_index_ts_role_kind_and_text_preview() {
        let entry = json!({"ts": 1_790_000_000_123u64, "role": "user", "kind": "prompt", "text": "hi\nthere"});
        assert_eq!(
            line(3, &entry),
            "   3  1790000000123  user       prompt       hi there"
        );
    }

    #[test]
    fn tool_entries_preview_tool_and_input() {
        let call = json!({"ts": "t", "role": "tool", "kind": "tool_call", "tool": "Bash", "input": "{\"command\":\"ls\"}"});
        assert!(line(0, &call).ends_with("Bash {\"command\":\"ls\"}"));
        let bare = json!({"tool": "Bash", "input": null});
        assert!(line(0, &bare).ends_with("Bash"));
        let null_text = json!({"text": null, "tool": "Read"});
        assert!(line(0, &null_text).ends_with("Read"));
    }

    #[test]
    fn odd_entries_still_render() {
        assert_eq!(line(0, &json!({})).trim_end(), "   0");
        let object_text = json!({"text": {"content": "x"}});
        assert!(line(0, &object_text).ends_with("{\"content\":\"x\"}"));
        let long = json!({"text": "x".repeat(500)});
        assert!(line(0, &long).ends_with(&"x".repeat(200)));
    }
}
