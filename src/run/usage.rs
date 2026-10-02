use super::adapter::ADAPTERS;

pub const VERSION: &str = concat!("clawlog ", env!("CARGO_PKG_VERSION"));

/// Returns the help text for the Clawlog command-line interface.
pub fn help() -> String {
    let adapters: Vec<&str> = ADAPTERS.iter().map(|adapter| adapter.name()).collect();
    format!(
        "{VERSION}: logs coding-agent prompts, tool calls and responses as JSON

Usage:
  clawlog                open the interactive interface: browse, install, uninstall, export, clear
  clawlog -h <adapter>   log one hook event read from stdin (the hook config runs: clawlog -h claude)
  clawlog --help         show this help
  clawlog -V, --version  show the version

Adapters: {adapters}

Environment:
  CLAWLOG_DIR        log directory (default: $HOME/.clawlog)
  CLAWLOG_MAX_BYTES  size at which a new part starts (default: 524288000 = 500 MiB)

Files:
  $CLAWLOG_DIR/<adapter>/clawlog_<adapter>_<conversation id>_<NNNN>.part.json
  Each part is a JSON array holding one entry per line in arrival order.
",
        adapters = adapters.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::{help, VERSION};

    #[test]
    fn help_covers_the_interface_hook_mode_env_vars_and_layout() {
        let text = help();
        for needle in [
            VERSION,
            "interactive interface",
            "clawlog -h <adapter>",
            "clawlog -h claude",
            "--version",
            "Adapters: claude",
            "CLAWLOG_DIR",
            "$HOME/.clawlog",
            "CLAWLOG_MAX_BYTES",
            "524288000",
            ".part.json",
        ] {
            assert!(text.contains(needle), "{needle}");
        }
        for gone in ["clawlog hooks", "clawlog install"] {
            assert!(!text.contains(gone), "{gone}");
        }
        assert_eq!(VERSION, format!("clawlog {}", env!("CARGO_PKG_VERSION")));
    }
}
