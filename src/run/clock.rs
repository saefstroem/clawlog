use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::now_millis;

    #[test]
    fn is_after_2026_and_monotonic_enough() {
        let first = now_millis();
        let second = now_millis();
        assert!(first > 1_767_225_600_000);
        assert!(second >= first);
    }
}
