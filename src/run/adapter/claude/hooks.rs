use serde_json::{json, Value};

/// The command used for the hooks for claude
const COMMAND: &str = "\"$HOME\"/.cargo/bin/clawlog -h claude";

/// For which hooks we want to log the action
pub fn hooks() -> Value {
    json!({"hooks": {
        "UserPromptSubmit": [{"hooks": [{"type": "command", "command": COMMAND}]}],
        "MessageDisplay": [{"hooks": [{"type": "command", "command": COMMAND, "timeout": 5}]}],
        "PreToolUse": [{"matcher": "Bash|Edit|Write", "hooks": [{"type": "command", "command": COMMAND}]}],
        "PostToolUse": [{"matcher": "Bash|Edit|Write", "hooks": [{"type": "command", "command": COMMAND}]}]
    }})
}

/// Returns an iterator over the commands of each hook in a given group
/// used for enumerating which hooks are configured
pub fn commands(group: &Value) -> impl Iterator<Item = &Value> {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hook| hook.get("command"))
}

#[cfg(test)]
mod tests {
    use serde_json::{from_str, json, Value};

    use super::{commands, hooks};

    const USER_BLOCK: &str = r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"\"$HOME\"/.cargo/bin/llmlog"}]}],"MessageDisplay":[{"hooks":[{"type":"command","command":"\"$HOME\"/.cargo/bin/llmlog","timeout":5}]}],"Stop":[{"hooks":[{"type":"command","command":"\"$HOME\"/.cargo/bin/llmlog"}]}],"PreToolUse":[{"matcher":"Bash|Edit|Write","hooks":[{"type":"command","command":"\"$HOME\"/.cargo/bin/llmlog"}]}],"PostToolUse":[{"matcher":"Bash|Edit|Write","hooks":[{"type":"command","command":"\"$HOME\"/.cargo/bin/llmlog"}]}]}}"#;

    #[test]
    fn equals_the_user_block_minus_stop_with_the_hook_mode_command() {
        let mut expected: Value = from_str(
            &USER_BLOCK.replace("/.cargo/bin/llmlog\"", "/.cargo/bin/clawlog -h claude\""),
        )
        .unwrap();
        expected["hooks"].as_object_mut().unwrap().remove("Stop");
        assert_eq!(hooks(), expected);
    }

    #[test]
    fn commands_lists_the_command_of_each_hook_in_a_group() {
        let group = json!({"hooks": [{"command": "a"}, {"type": "command"}, {"command": "b"}]});
        let listed: Vec<&Value> = commands(&group).collect();
        assert_eq!(listed, vec![&json!("a"), &json!("b")]);
        assert_eq!(commands(&json!({})).count(), 0);
        assert_eq!(commands(&json!({"hooks": "x"})).count(), 0);
    }
}
