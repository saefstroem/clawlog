use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;

use super::{Error, Result};

/// Returns the home directory as specified by the HOME environment variable.
/// Returns an error if the HOME environment variable is not set or empty.
pub fn home(env: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    lookup(env, "HOME").map(PathBuf::from).ok_or(Error::Home)
}

/// Returns the directory for Clawlog data as specified by the CLAWLOG_DIR environment variable.
/// Falls back to $HOME/.clawlog if CLAWLOG_DIR is not set or empty.
/// Returns an error if the home directory cannot be determined.
pub fn dir(env: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    match lookup(env, "CLAWLOG_DIR") {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(home(env)?.join(".clawlog")),
    }
}

/// Returns the value of the environment variable as a number.
/// Returns the default value if the environment variable is not set or empty.
/// Returns an error if the environment variable cannot be parsed as a number.
pub fn number<T: FromStr>(
    env: &dyn Fn(&str) -> Option<OsString>,
    name: &'static str,
    default: T,
) -> Result<T> {
    match lookup(env, name) {
        Some(value) => value
            .to_str()
            .and_then(|text| text.trim().parse().ok())
            .ok_or_else(|| Error::Number {
                name,
                value: value.to_string_lossy().into_owned(),
            }),
        None => Ok(default),
    }
}

/// Looks up the environment variable and returns it if it is set and not empty.
fn lookup(env: &dyn Fn(&str) -> Option<OsString>, name: &str) -> Option<OsString> {
    env(name).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    use super::{dir, home, number};

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |key| vars.get(key).map(OsString::from)
    }

    #[test]
    fn dir_prefers_clawlog_dir_then_home() {
        assert_eq!(
            dir(&env(&[("CLAWLOG_DIR", "/logs"), ("HOME", "/h")])).unwrap(),
            PathBuf::from("/logs")
        );
        assert_eq!(
            dir(&env(&[("CLAWLOG_DIR", ""), ("HOME", "/h")])).unwrap(),
            PathBuf::from("/h/.clawlog")
        );
    }

    #[test]
    fn missing_or_empty_home_is_an_error() {
        assert_eq!(home(&env(&[])).unwrap_err().to_string(), "HOME is not set");
        assert!(home(&env(&[("HOME", "")])).is_err());
        assert!(dir(&env(&[])).is_err());
        assert_eq!(home(&env(&[("HOME", "/h")])).unwrap(), PathBuf::from("/h"));
    }

    #[test]
    fn numbers_default_when_unset_or_empty_and_are_trimmed() {
        assert_eq!(number(&env(&[]), "N", 7u64).unwrap(), 7);
        assert_eq!(number(&env(&[("N", "")]), "N", 7u64).unwrap(), 7);
        assert_eq!(number(&env(&[("N", " 12\n")]), "N", 7u64).unwrap(), 12);
        assert_eq!(number(&env(&[("N", "0")]), "N", 7usize).unwrap(), 0);
    }

    #[test]
    fn unparsable_numbers_are_errors() {
        for value in ["abc", "-1", "1.5", "  "] {
            assert_eq!(
                number(&env(&[("N", value)]), "N", 7u64)
                    .unwrap_err()
                    .to_string(),
                format!("N must be a whole number, got {value:?}")
            );
        }
    }

    #[test]
    fn non_utf8_values_are_kept_exact_for_paths_and_rejected_as_numbers() {
        let raw = OsString::from_vec(b"/logs-\xff".to_vec());
        let non_utf8 = |_: &str| Some(raw.clone());
        assert_eq!(dir(&non_utf8).unwrap(), PathBuf::from(raw.clone()));
        assert_eq!(home(&non_utf8).unwrap(), PathBuf::from(raw.clone()));
        assert_eq!(
            number(&non_utf8, "N", 7u64).unwrap_err().to_string(),
            "N must be a whole number, got \"/logs-\u{fffd}\""
        );
    }
}
