use std::time::{Duration, SystemTime};

/// Current wall-clock time, so expiry and playtime logic is testable.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

/// Seconds since the Unix epoch; times before it clamp to zero.
pub fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed: Duration| elapsed.as_secs())
}
