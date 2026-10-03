use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
    millis(SystemTime::now())
}

pub fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::{millis, now_millis};

    #[test]
    fn is_after_2026_and_monotonic_enough() {
        let first = now_millis();
        let second = now_millis();
        assert!(first > 1_767_225_600_000);
        assert!(second >= first);
    }

    #[test]
    fn converts_times_and_clamps_pre_epoch_to_zero() {
        assert_eq!(millis(UNIX_EPOCH + Duration::from_millis(1234)), 1234);
        assert_eq!(millis(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }
}
