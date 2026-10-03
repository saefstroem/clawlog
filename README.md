# clawlog

clawlog records the exchange between a user and a coding agent as JSON:
prompts, assistant messages, tool calls and tool results.

Currently, only claude code is supported through the adapter: `claude`.

## Installation

```sh
cargo install clawlog
```

clawlog requires Rust 1.89 or later.

Run `clawlog`, press `i`, select the adapters with Space and press Enter.
This adds hooks to `~/.claude/settings.json` that run
`$HOME/.cargo/bin/clawlog -h claude`. If the binary is installed at another
path, then the hook command should be changed to that path. After this, all agent sessions started after the installation are recorded.

## How it works

- The agent runs `clawlog -h claude` on each hook event and passes the event
  on standard input. clawlog appends one entry per event to
  `~/.clawlog/<adapter>/clawlog_<adapter>_<session id>_<NNNN>.part.json`.
- Each part file is a JSON array with one entry per line, in the order the
  events arrived. A new part is started when the current one reaches 500 MiB.
  `CLAWLOG_DIR` sets another directory and `CLAWLOG_MAX_BYTES` another size.
- In hook mode clawlog writes nothing to standard output and always exits with status 0, so it cannot add to the model context or block the agent. In addition, should a bug ever occur with an adapter it will not cause an issue for the agent.
- **Slash commands and skill invocations are not recorded.**
- The `clawlog` TUI allows to glance over conversations and their entries, install and removes the hooks as well as  export the files and delete them.

**Important note**: Entries are stored in full, without redaction, and can contain secrets. The
log directory SHOULD be readable only by its owner, for example after
`chmod 700 ~/.clawlog`.

## Hook events

The `claude` adapter subscribes to these Claude Code hook events:

- `UserPromptSubmit`: the prompt. Recorded with `role` `user` and `kind`
  `prompt`.
- `MessageDisplay`: an assistant message. Recorded with `role` `assistant`
  and `kind` `message`. The hook has a timeout of 5 seconds.
- `PreToolUse` with matcher `Bash|Edit|Write`: a tool call. Recorded with
  `role` `tool` and `kind` `tool_call`.
- `PostToolUse` with matcher `Bash|Edit|Write`: a tool result. Recorded with
  `role` `tool` and `kind` `tool_result`.


## License

MIT, see [LICENSE](LICENSE).

[RFC 2119]: https://www.rfc-editor.org/rfc/rfc2119
