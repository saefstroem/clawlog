use serde::Serialize;
use serde_json::{Map, Value};

/// Fixed keys that may be part of the LLM and should not appear in the detail fields.
/// we filter away these keys from the detail fields.
/// TODO: in the future we need to make this trait backed rather than a fixed array.
const FIXED_KEYS: [&str; 7] = [
    "ts",
    "session_id",
    "prompt_id",
    "cwd",
    "role",
    "kind",
    "event",
];

/// Represents a single log record.
///
/// Each record contains metadata such as timestamps, session and prompt identifiers, the current working directory,
/// the role and kind of the record, an optional event, and additional detail fields.
#[derive(Serialize)]
pub struct Record {
    pub ts: u64,
    pub session_id: Option<String>,
    pub prompt_id: Option<String>,
    pub cwd: Option<String>,
    pub role: Role,
    pub kind: Kind,
    pub event: Option<String>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
    Other,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Prompt,
    Response,
    Message,
    ToolCall,
    ToolResult,
    ToolError,
    Other,
}

#[derive(Serialize)]
#[serde(untagged)]
/// Represents the detailed content of a log record.
///
/// The detail can be textual content, a message with additional fields, a tool call or result, or other types.
pub enum Detail {
    Text {
        text: Option<String>,
    },
    Message {
        #[serde(flatten)]
        fields: Map<String, Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    ToolCall {
        tool: Option<String>,
        tool_use_id: Option<String>,
        input: Option<String>,
    },
    ToolResult {
        tool: Option<String>,
        tool_use_id: Option<String>,
        input: Option<String>,
        output: Option<String>,
    },
    Other,
}

impl Record {
    /// Normalizes the record by removing fixed keys from the detail fields.
    ///
    /// If the detail is a message, this method removes any keys that are part of the fixed keys list.
    /// Additionally, if the text field is present, it is removed from the fields map.
    pub fn normalize(&mut self) {
        if let Detail::Message { fields, text } = &mut self.detail {
            fields.retain(|key, _| !FIXED_KEYS.contains(&key.as_str()));
            if text.is_some() {
                fields.remove("text");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_string, to_value, Map};

    use super::{Detail, Kind, Record, Role};

    fn record(role: Role, kind: Kind, detail: Detail) -> Record {
        Record {
            ts: 1_790_000_000_123,
            session_id: Some("s1".to_owned()),
            prompt_id: None,
            cwd: Some("/w".to_owned()),
            role,
            kind,
            event: Some("E".to_owned()),
            detail,
        }
    }

    #[test]
    fn serializes_keys_in_schema_order() {
        let prompt = record(
            Role::User,
            Kind::Prompt,
            Detail::Text {
                text: Some("hi".to_owned()),
            },
        );
        assert_eq!(
            to_string(&prompt).unwrap(),
            r#"{"ts":1790000000123,"session_id":"s1","prompt_id":null,"cwd":"/w","role":"user","kind":"prompt","event":"E","text":"hi"}"#
        );
        let result = record(
            Role::Tool,
            Kind::ToolError,
            Detail::ToolResult {
                tool: Some("Bash".to_owned()),
                tool_use_id: None,
                input: Some("{}".to_owned()),
                output: None,
            },
        );
        assert_eq!(
            to_string(&result).unwrap(),
            r#"{"ts":1790000000123,"session_id":"s1","prompt_id":null,"cwd":"/w","role":"tool","kind":"tool_error","event":"E","tool":"Bash","tool_use_id":null,"input":"{}","output":null}"#
        );
    }

    #[test]
    fn other_detail_adds_no_keys() {
        let other = record(Role::Other, Kind::Other, Detail::Other);
        assert_eq!(
            to_string(&other).unwrap(),
            r#"{"ts":1790000000123,"session_id":"s1","prompt_id":null,"cwd":"/w","role":"other","kind":"other","event":"E"}"#
        );
    }

    #[test]
    fn tool_call_has_no_output_key() {
        let call = record(
            Role::Tool,
            Kind::ToolCall,
            Detail::ToolCall {
                tool: None,
                tool_use_id: Some("t".to_owned()),
                input: None,
            },
        );
        assert_eq!(
            to_string(&call).unwrap(),
            r#"{"ts":1790000000123,"session_id":"s1","prompt_id":null,"cwd":"/w","role":"tool","kind":"tool_call","event":"E","tool":null,"tool_use_id":"t","input":null}"#
        );
    }

    #[test]
    fn normalize_drops_colliding_passthrough_fields() {
        let mut fields = Map::new();
        for key in [
            "ts",
            "session_id",
            "prompt_id",
            "cwd",
            "role",
            "kind",
            "event",
            "text",
        ] {
            fields.insert(key.to_owned(), json!("collides"));
        }
        fields.insert("delta".to_owned(), json!("abcdef"));
        let mut message = record(
            Role::Assistant,
            Kind::Message,
            Detail::Message {
                fields,
                text: Some("abcdef".to_owned()),
            },
        );
        message.normalize();
        assert_eq!(
            to_string(&message).unwrap(),
            r#"{"ts":1790000000123,"session_id":"s1","prompt_id":null,"cwd":"/w","role":"assistant","kind":"message","event":"E","delta":"abcdef","text":"abcdef"}"#
        );
    }

    #[test]
    fn message_without_text_has_no_text_key() {
        let message = record(
            Role::Assistant,
            Kind::Message,
            Detail::Message {
                fields: Map::new(),
                text: None,
            },
        );
        assert!(to_value(&message).unwrap().get("text").is_none());
    }

    #[test]
    fn normalize_keeps_passthrough_text_when_no_text_source() {
        let mut fields = Map::new();
        fields.insert("text".to_owned(), json!({"content": "hello"}));
        let mut message = record(
            Role::Assistant,
            Kind::Message,
            Detail::Message { fields, text: None },
        );
        message.normalize();
        assert_eq!(
            to_value(&message).unwrap()["text"],
            json!({"content": "hello"})
        );
    }
}
