use std::ffi::OsString;
use std::io::{Read, Write};

use serde_json::to_string;

use super::adapter::{find, Adapter};
use super::clock::now_millis;
use super::{browse, part_file, settings, usage, Result};

/// Runs the program with specified arguments
pub fn run(
    args: &[String],
    env: &dyn Fn(&str) -> Option<OsString>,
    stdin: &mut dyn Read,
    stderr: &mut dyn Write,
) -> Result<()> {
    // Returns the current time in milliseconds since the UNIX epoch

    // Logs the input using the specified adapter
    let log = |adapter: &dyn Adapter,
               env: &dyn Fn(&str) -> Option<OsString>,
               stdin: &mut dyn Read|
     -> Result<()> {
        let dir = settings::dir(env)?;
        let max_bytes = settings::number(env, "CLAWLOG_MAX_BYTES", 524_288_000)?;
        let mut input = Vec::new();
        stdin.read_to_end(&mut input)?;

        // with the specified adapter, extract a record from the input
        let Some(mut record) = adapter.capture(&input, now_millis())? else {
            // we explicitly dont log anything here to prevent poisoning context
            // in the case that the adapter is unable to capture a record
            return Ok(());
        };

        // remove some common fields that are not needed for logging
        record.normalize();

        // store the file
        part_file::append(
            &dir,
            adapter.name(),
            record.session_id.as_deref(),
            max_bytes,
            &to_string(&record)?,
        )
    };

    let args: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    // matches argument and passes to the correct fn
    match args.as_slice() {
        [] => {
            let dir = settings::dir(env)?;
            let home = settings::home(env)?;
            browse::run(&dir, &home)?;
        }
        ["-h", name, ..] => log(find(name)?, env, stdin)?,
        ["-V" | "--version", ..] => writeln!(stderr, "{}", usage::VERSION)?,
        _ => write!(stderr, "{}", usage::help())?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde_json::{from_str, json, Value};

    use super::run;
    use crate::run::scratch_dir::ScratchDir;
    use crate::run::Result;

    struct Outcome {
        result: Result<()>,
        stderr: String,
    }

    fn invoke(args: &[&str], vars: &[(&str, &str)], stdin: &str) -> Outcome {
        let args: Vec<String> = ["clawlog"]
            .iter()
            .chain(args)
            .map(|arg| arg.to_string())
            .collect();
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let mut stderr = Vec::new();
        let result = run(
            &args,
            &|key| vars.get(key).map(OsString::from),
            &mut stdin.as_bytes(),
            &mut stderr,
        );
        Outcome {
            result,
            stderr: String::from_utf8(stderr).unwrap(),
        }
    }

    fn log(home: &Path, stdin: Value) {
        let home = home.to_str().unwrap();
        invoke(
            &["-h", "claude", "--extra"],
            &[("HOME", home)],
            &stdin.to_string(),
        )
        .result
        .unwrap();
    }

    fn part(home: &Path, id: &str, number: u32) -> PathBuf {
        home.join(".clawlog/claude")
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
    fn logs_a_conversation_in_order_and_hides_skill_usage() {
        let home = ScratchDir::new();
        let events = [
            json!({"hook_event_name": "SessionStart", "session_id": "s1", "source": "startup"}),
            json!({"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt": "hi"}),
            json!({"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt": " /plugin install ponytail@ponytail"}),
            json!({"hook_event_name": "UserPromptExpansion", "session_id": "s1", "command_name": "ponytail", "prompt": "x"}),
            json!({"hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Skill", "tool_input": {}}),
            json!({"hook_event_name": "PostToolUse", "session_id": "s1", "tool_name": "Skill", "tool_response": {}}),
            json!({"hook_event_name": "PostToolUseFailure", "session_id": "s1", "tool_name": "Skill", "error": "e"}),
            json!({"hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "ls"}}),
            json!({"hook_event_name": "MessageDisplay", "session_id": "s1", "delta": "Hel", "index": 0, "final": false}),
            json!({"hook_event_name": "Stop", "session_id": "s1", "last_assistant_message": "Hello"}),
            json!({"hook_event_name": "Stop", "session_id": "s2", "last_assistant_message": "other"}),
        ];
        for event in events {
            log(home.path(), event);
        }
        let kinds: Vec<Value> = entries(&part(home.path(), "s1", 1))
            .iter()
            .map(|entry| entry["kind"].clone())
            .collect();
        assert_eq!(
            kinds,
            vec![
                json!("other"),
                json!("prompt"),
                json!("tool_call"),
                json!("message"),
                json!("response")
            ]
        );
        assert_eq!(entries(&part(home.path(), "s2", 1))[0]["text"], "other");
        assert!(!fs::read_to_string(part(home.path(), "s1", 1))
            .unwrap()
            .contains("ponytail"));
    }

    #[test]
    fn applies_env_limits_and_directory() {
        let scratch = ScratchDir::new();
        let dir = scratch.path().join("logs");
        let stdin = json!({"hook_event_name": "PreToolUse", "session_id": "s/1", "tool_name": "Bash", "tool_input": {"command": "echo 日本語"}, "prompt_id": "p"});
        let outcome = invoke(
            &["-h", "claude"],
            &[
                ("CLAWLOG_DIR", dir.to_str().unwrap()),
                ("CLAWLOG_MAX_BYTES", "0"),
            ],
            &stdin.to_string(),
        );
        outcome.result.unwrap();
        let path = dir.join("claude/clawlog_claude_s-1_0001.part.json");
        assert_eq!(entries(&path)[0]["input"], "{\"command\":\"echo 日本語\"}");
        invoke(
            &["-h", "claude"],
            &[
                ("CLAWLOG_DIR", dir.to_str().unwrap()),
                ("CLAWLOG_MAX_BYTES", "0"),
            ],
            &stdin.to_string(),
        )
        .result
        .unwrap();
        assert!(dir
            .join("claude/clawlog_claude_s-1_0002.part.json")
            .exists());
    }

    #[test]
    fn failures_write_nothing() {
        let home = ScratchDir::new();
        let home_str = home.path().to_str().unwrap();
        let prompt =
            json!({"hook_event_name": "UserPromptSubmit", "session_id": "s1", "prompt": "hi"})
                .to_string();
        for (args, vars, stdin, message) in [
            (
                vec!["-h", "nope", "x"],
                vec![("HOME", home_str)],
                prompt.as_str(),
                "unknown adapter 'nope'",
            ),
            (
                vec!["-h", "claude"],
                vec![("HOME", home_str), ("CLAWLOG_MAX_BYTES", "-1")],
                prompt.as_str(),
                "CLAWLOG_MAX_BYTES must be a whole number, got \"-1\"",
            ),
            (
                vec!["-h", "claude"],
                vec![],
                prompt.as_str(),
                "HOME is not set",
            ),
            (
                vec!["-h", "claude"],
                vec![("HOME", home_str)],
                "",
                "invalid JSON: EOF while parsing a value at line 1 column 0",
            ),
        ] {
            let outcome = invoke(&args, &vars, stdin);
            assert_eq!(outcome.result.unwrap_err().to_string(), message);
            assert_eq!(outcome.stderr, "");
        }
        assert!(!home.path().join(".clawlog/claude").exists());
    }

    #[test]
    fn help_and_version_go_to_stderr() {
        for args in [
            &["-h"][..],
            &["--help"],
            &["--help", "claude"],
            &["-x"],
            &["extra", "args"],
        ] {
            let help = invoke(args, &[], "");
            help.result.unwrap();
            assert!(help.stderr.contains("CLAWLOG_MAX_BYTES"), "{args:?}");
        }
        for args in [&["-V"][..], &["--version"], &["-V", "x"]] {
            assert_eq!(
                invoke(args, &[], "").stderr,
                format!("clawlog {}\n", env!("CARGO_PKG_VERSION"))
            );
        }
    }

    #[test]
    fn former_subcommands_and_bare_adapters_show_help_and_act_on_nothing() {
        let home = ScratchDir::new();
        let vars = [("HOME", home.path().to_str().unwrap())];
        for args in [
            &["hooks", "claude"][..],
            &["install", "claude"],
            &["claude"],
        ] {
            let outcome = invoke(args, &vars, "{\"hook_event_name\": \"Stop\"}");
            outcome.result.unwrap();
            assert!(outcome.stderr.contains("Usage:"), "{args:?}");
        }
        assert!(!home.path().join(".claude").exists());
        assert!(!home.path().join(".clawlog").exists());
    }

    #[test]
    fn bare_invocation_resolves_the_environment_before_the_interface() {
        let outcome = invoke(&[], &[], "");
        assert_eq!(outcome.result.unwrap_err().to_string(), "HOME is not set");
        assert_eq!(outcome.stderr, "");
    }
}
