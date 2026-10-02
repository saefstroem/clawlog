use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use super::Result;
use crate::run::record::Record;

/// An LLM adapter for capturing prompts for a specific type of LLM harness
pub trait Adapter {
    fn name(&self) -> &'static str;
    fn capture(&self, stdin: &[u8], now: u64) -> Result<Option<Record>>;
    fn install(&self, home: &Path) -> Result<Installed>;
    fn uninstall(&self, home: &Path) -> Result<Removed>;
}

/// Represents the result of installing hooks for an LLM harness
pub struct Installed {
    pub path: PathBuf,
    pub added: usize,
    pub present: usize,
}

impl Display for Installed {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "added {} hook(s) to {} ({} already present)",
            self.added,
            self.path.display(),
            self.present
        )
    }
}

/// Represents the result of removing hooks for an LLM harness
pub struct Removed {
    pub path: PathBuf,
    pub removed: usize,
}

impl Display for Removed {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "removed {} hook(s) from {}",
            self.removed,
            self.path.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Installed, Removed};

    #[test]
    fn reports_added_and_present_counts() {
        let installed = Installed {
            path: PathBuf::from("/h/.claude/settings.json"),
            added: 2,
            present: 3,
        };
        assert_eq!(
            installed.to_string(),
            "added 2 hook(s) to /h/.claude/settings.json (3 already present)"
        );
    }

    #[test]
    fn reports_removed_count_and_path() {
        let removed = Removed {
            path: PathBuf::from("/h/.claude/settings.json"),
            removed: 4,
        };
        assert_eq!(
            removed.to_string(),
            "removed 4 hook(s) from /h/.claude/settings.json"
        );
    }
}
