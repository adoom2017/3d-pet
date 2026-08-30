use std::time::Duration;

pub const FIXED_UPDATE_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);
pub const MAX_ACCUMULATED_TIME: Duration = Duration::from_millis(250);
pub const MAX_FIXED_STEPS_PER_TURN: usize = 5;
