use std::io::Write;
use std::panic::{self, AssertUnwindSafe};

use crate::Result;

pub fn guard(body: impl FnOnce() -> Result<()>, stderr: &mut dyn Write) {
    // execute a guarded body and handle panics by writing to stderr
    // to avoid poisoning LLM context
    let message = match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error.to_string(),
        Err(_) => "internal error (panic)".to_owned(),
    };
    let _ = writeln!(stderr, "clawlog: {message}");
}

#[cfg(test)]
mod tests {
    use crate::{guard::guard, run, Result};

    fn reported(body: impl FnOnce() -> Result<()>) -> String {
        let mut stderr = Vec::new();
        guard(body, &mut stderr);
        String::from_utf8(stderr).unwrap()
    }

    #[test]
    fn success_is_silent() {
        assert_eq!(reported(|| Ok(())), "");
    }

    #[test]
    fn errors_are_prefixed() {
        assert_eq!(
            reported(|| Err(run::Error::Home)),
            "clawlog: HOME is not set\n"
        );
    }

    #[test]
    fn panics_are_reported_not_propagated() {
        assert_eq!(
            reported(|| panic!("boom")),
            "clawlog: internal error (panic)\n"
        );
    }
}
