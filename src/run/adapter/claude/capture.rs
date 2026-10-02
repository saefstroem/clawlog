use serde_json::{from_slice, Map, Value};

use super::Result;
use crate::run::record::{Detail, Kind, Record, Role};

/// Keys that should not be passed through to the message fields
const NOT_PASSED_THROUGH: [&str; 8] = [
    "session_id",
    "prompt_id",
    "cwd",
    "transcript_path",
    "scratchpad_dir",
    "permission_mode",
    "effort",
    "hook_event_name",
];

/// Keys that are considered as message text fields
const MESSAGE_TEXT_KEYS: [&str; 3] = ["text", "message", "delta"];

/// Captures a record from some given bytes and the current timestamp
pub fn capture(stdin: &[u8], now: u64) -> Result<Option<Record>> {
    // Returns the string value associated with a key, if it exists
    let string =
        |input: &Value, key: &str| input.get(key).and_then(Value::as_str).map(str::to_owned);

    // returns the string representation of the value associated with a key, if it exists
    let compact = |input: &Value, key: &str| {
        input
            .get(key)
            .filter(|value| !value.is_null())
            .map(Value::to_string)
    };

    // Returns a map of key-value pairs excluding the ones in NOT_PASSED_THROUGH
    let passthrough = |input: &Value| -> Map<String, Value> {
        input
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| !NOT_PASSED_THROUGH.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    };

    // Constructs a tool result detail from the input JSON
    let tool_result = |input: &Value| Detail::ToolResult {
        tool: string(input, "tool_name"),
        tool_use_id: string(input, "tool_use_id"),
        input: compact(input, "tool_input"),
        output: compact(input, "error").or_else(|| compact(input, "tool_response")),
    };

    // Parse the input JSON from the provided bytes
    let input: Value = from_slice(stdin)?;

    // Extract commonly used fields from the input JSON
    let event = string(&input, "hook_event_name");
    let prompt = string(&input, "prompt");
    let slash_command = prompt
        .as_deref()
        .is_some_and(|prompt| prompt.trim_start().starts_with('/'));

    // Determine if the current event is related to a skill tool usage
    let skill = string(&input, "tool_name").as_deref() == Some("Skill");

    // Determine the role, kind, and detail of the record based on the event type
    let (role, kind, detail) = match event.as_deref() {
        Some("UserPromptExpansion") => return Ok(None),
        Some("UserPromptSubmit") if slash_command => return Ok(None),
        Some("PreToolUse" | "PostToolUse" | "PostToolUseFailure") if skill => return Ok(None),
        Some("UserPromptSubmit") => (Role::User, Kind::Prompt, Detail::Text { text: prompt }),
        Some("Stop") => (
            Role::Assistant,
            Kind::Response,
            Detail::Text {
                text: string(&input, "last_assistant_message"),
            },
        ),
        Some("MessageDisplay") => (
            Role::Assistant,
            Kind::Message,
            Detail::Message {
                fields: passthrough(&input),
                text: MESSAGE_TEXT_KEYS.iter().find_map(|key| string(&input, key)),
            },
        ),
        Some("PreToolUse") => (
            Role::Tool,
            Kind::ToolCall,
            Detail::ToolCall {
                tool: string(&input, "tool_name"),
                tool_use_id: string(&input, "tool_use_id"),
                input: compact(&input, "tool_input"),
            },
        ),
        Some("PostToolUse") => (Role::Tool, Kind::ToolResult, tool_result(&input)),
        Some("PostToolUseFailure") => (Role::Tool, Kind::ToolError, tool_result(&input)),
        _ => (Role::Other, Kind::Other, Detail::Other),
    };
    // Return the constructed record as an option
    Ok(Some(Record {
        ts: now,
        session_id: string(&input, "session_id"),
        prompt_id: string(&input, "prompt_id"),
        cwd: string(&input, "cwd"),
        role,
        kind,
        event,
        detail,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_value, Value};

    use super::capture;

    fn logged(input: Value) -> Option<Value> {
        let now = 1_790_000_000_123;
        capture(input.to_string().as_bytes(), now)
            .unwrap()
            .map(|record| to_value(record).unwrap())
    }

    fn common(event: Value, role: &str, kind: &str) -> Value {
        json!({
            "ts": 1_790_000_000_123u64,
            "session_id": "s1",
            "prompt_id": "p1",
            "cwd": "/w",
            "role": role,
            "kind": kind,
            "event": event,
        })
    }

    fn with(mut base: Value, extra: Value) -> Value {
        for (key, value) in extra.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn hook(event: &str, extra: Value) -> Value {
        with(
            json!({
                "session_id": "s1",
                "prompt_id": "p1",
                "cwd": "/w",
                "transcript_path": "/t.jsonl",
                "permission_mode": "default",
                "hook_event_name": event,
            }),
            extra,
        )
    }

    #[test]
    fn user_prompt_submit_is_a_user_prompt() {
        assert_eq!(
            logged(hook("UserPromptSubmit", json!({"prompt": "fix the bug"}))),
            Some(with(
                common(json!("UserPromptSubmit"), "user", "prompt"),
                json!({"text": "fix the bug"})
            ))
        );
    }

    #[test]
    fn non_string_prompt_is_null_text() {
        let record = logged(hook("UserPromptSubmit", json!({"prompt": 7}))).unwrap();
        assert_eq!(record["text"], Value::Null);
    }

    #[test]
    fn slash_commands_are_skipped() {
        for prompt in [
            "/help",
            "  /plugin marketplace add DietrichGebert/ponytail",
            "\n/plugin install ponytail@ponytail",
        ] {
            assert_eq!(
                logged(hook("UserPromptSubmit", json!({"prompt": prompt}))),
                None
            );
        }
        assert!(logged(hook("UserPromptSubmit", json!({"prompt": "a /path"}))).is_some());
    }

    #[test]
    fn prompt_expansions_are_skipped() {
        let expansion = hook(
            "UserPromptExpansion",
            json!({"expansion_type": "slash_command", "command_name": "ponytail", "command_args": "", "prompt": "expanded"}),
        );
        assert_eq!(logged(expansion), None);
    }

    #[test]
    fn skill_tool_events_are_skipped() {
        for event in ["PreToolUse", "PostToolUse", "PostToolUseFailure"] {
            let skill = hook(
                event,
                json!({"tool_name": "Skill", "tool_input": {"skill": "ponytail:ponytail"}, "tool_response": "ok"}),
            );
            assert_eq!(logged(skill), None);
        }
    }

    #[test]
    fn stop_is_the_assistant_response() {
        assert_eq!(
            logged(hook("Stop", json!({"last_assistant_message": "done"}))),
            Some(with(
                common(json!("Stop"), "assistant", "response"),
                json!({"text": "done"})
            ))
        );
        assert_eq!(
            logged(hook("Stop", json!({}))).unwrap()["text"],
            Value::Null
        );
    }

    #[test]
    fn message_display_passes_the_streamed_batch_through() {
        let batch = hook(
            "MessageDisplay",
            json!({"scratchpad_dir": "/s", "effort": "high", "turn_id": "t1", "message_id": "m1", "index": 0, "final": false, "delta": "Hello"}),
        );
        assert_eq!(
            logged(batch),
            Some(with(
                common(json!("MessageDisplay"), "assistant", "message"),
                json!({"turn_id": "t1", "message_id": "m1", "index": 0, "final": false, "delta": "Hello", "text": "Hello"})
            ))
        );
    }

    #[test]
    fn message_display_text_prefers_text_then_message_then_delta() {
        let all = hook(
            "MessageDisplay",
            json!({"text": "from text", "message": "from message", "delta": "from delta"}),
        );
        assert_eq!(logged(all).unwrap()["text"], "from text");
        let both = hook(
            "MessageDisplay",
            json!({"message": "from message", "delta": "from delta"}),
        );
        assert_eq!(logged(both).unwrap()["text"], "from message");
        let skip_non_string = hook("MessageDisplay", json!({"message": 5, "delta": "d"}));
        assert_eq!(logged(skip_non_string).unwrap()["text"], "d");
        let no_text = logged(hook("MessageDisplay", json!({"delta": 5}))).unwrap();
        assert!(no_text.get("text").is_none());
        assert_eq!(no_text["delta"], 5);
    }

    #[test]
    fn pre_tool_use_is_a_tool_call_with_compact_input() {
        let call = hook(
            "PreToolUse",
            json!({"tool_name": "Bash", "tool_use_id": "tu1", "tool_input": {"command": "ls -la"}}),
        );
        assert_eq!(
            logged(call),
            Some(with(
                common(json!("PreToolUse"), "tool", "tool_call"),
                json!({"tool": "Bash", "tool_use_id": "tu1", "input": "{\"command\":\"ls -la\"}"})
            ))
        );
    }

    #[test]
    fn post_tool_use_is_a_tool_result() {
        let result = hook(
            "PostToolUse",
            json!({"tool_name": "Edit", "tool_use_id": "tu2", "tool_input": {"file_path": "a"}, "tool_response": {"ok": true}}),
        );
        assert_eq!(
            logged(result),
            Some(with(
                common(json!("PostToolUse"), "tool", "tool_result"),
                json!({"tool": "Edit", "tool_use_id": "tu2", "input": "{\"file_path\":\"a\"}", "output": "{\"ok\":true}"})
            ))
        );
    }

    #[test]
    fn post_tool_use_failure_is_a_tool_error_preferring_the_error_field() {
        let failure = hook(
            "PostToolUseFailure",
            json!({"tool_name": "Bash", "tool_use_id": "tu3", "tool_input": {"command": "false"}, "error": "exit 1", "tool_response": "ignored"}),
        );
        assert_eq!(
            logged(failure),
            Some(with(
                common(json!("PostToolUseFailure"), "tool", "tool_error"),
                json!({"tool": "Bash", "tool_use_id": "tu3", "input": "{\"command\":\"false\"}", "output": "\"exit 1\""})
            ))
        );
        let null_error = hook(
            "PostToolUseFailure",
            json!({"tool_name": "Bash", "error": null, "tool_response": "fallback"}),
        );
        assert_eq!(logged(null_error).unwrap()["output"], "\"fallback\"");
        let bare = logged(hook("PostToolUse", json!({}))).unwrap();
        assert_eq!(bare["tool"], Value::Null);
        assert_eq!(bare["input"], Value::Null);
        assert_eq!(bare["output"], Value::Null);
    }

    #[test]
    fn other_events_carry_only_the_common_keys() {
        assert_eq!(
            logged(hook("SessionStart", json!({"source": "startup"}))),
            Some(common(json!("SessionStart"), "other", "other"))
        );
    }

    #[test]
    fn missing_or_non_string_event_is_other_with_null_ids() {
        let expected = json!({
            "ts": 1_790_000_000_123u64,
            "session_id": null,
            "prompt_id": null,
            "cwd": null,
            "role": "other",
            "kind": "other",
            "event": null,
        });
        assert_eq!(logged(json!({"prompt": "/x"})), Some(expected.clone()));
        assert_eq!(
            logged(json!({"hook_event_name": 3, "session_id": 9})),
            Some(expected)
        );
    }

    #[test]
    fn malformed_input_is_an_error() {
        let now = 0;
        for stdin in ["", "{", "not json"] {
            assert!(capture(stdin.as_bytes(), now).is_err());
        }
    }
}
