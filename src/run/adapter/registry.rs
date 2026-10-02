use super::claude::Claude;
use super::{Adapter, Error, Result};

/// Available LLM adapters
pub const ADAPTERS: &[&dyn Adapter] = &[&Claude];

/// Finds an adapter by its name, returning an error if it is unknown.
pub fn find(name: &str) -> Result<&'static dyn Adapter> {
    ADAPTERS
        .iter()
        .copied()
        .find(|adapter| adapter.name() == name)
        .ok_or_else(|| Error::Unknown(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::find;

    #[test]
    fn finds_claude_by_name() {
        assert_eq!(find("claude").unwrap().name(), "claude");
    }

    #[test]
    fn rejects_unknown_and_reserved_names() {
        for name in ["nope", "hooks", "install", "", "Claude"] {
            assert_eq!(
                find(name).err().unwrap().to_string(),
                format!("unknown adapter '{name}'")
            );
        }
    }
}
