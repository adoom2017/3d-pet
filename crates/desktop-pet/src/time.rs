use std::time::Duration;

pub const FIXED_UPDATE_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);
pub const MAX_ACCUMULATED_TIME: Duration = Duration::from_millis(250);
pub const MAX_FIXED_STEPS_PER_TURN: usize = 5;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FixedStepBatch {
    pub steps: usize,
    pub dropped_time: Duration,
}

#[derive(Debug, Default)]
pub(crate) struct FixedStepAccumulator {
    accumulated: Duration,
}

impl FixedStepAccumulator {
    pub fn push(&mut self, elapsed: Duration) -> FixedStepBatch {
        let total = self.accumulated.saturating_add(elapsed);
        let capped = total.min(MAX_ACCUMULATED_TIME);
        let mut dropped_time = total.saturating_sub(capped);
        self.accumulated = capped;

        let available = (self.accumulated.as_nanos() / FIXED_UPDATE_INTERVAL.as_nanos()) as usize;
        let steps = available.min(MAX_FIXED_STEPS_PER_TURN);
        self.accumulated -= FIXED_UPDATE_INTERVAL * steps as u32;

        let skipped_steps = available - steps;
        if skipped_steps > 0 {
            let skipped = FIXED_UPDATE_INTERVAL * skipped_steps as u32;
            self.accumulated -= skipped;
            dropped_time += skipped;
        }
        FixedStepBatch {
            steps,
            dropped_time,
        }
    }

    pub fn until_next_step(&self) -> Duration {
        FIXED_UPDATE_INTERVAL.saturating_sub(self.accumulated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulator_runs_whole_fixed_steps_and_keeps_remainder() {
        let mut accumulator = FixedStepAccumulator::default();
        let batch = accumulator.push(Duration::from_millis(25));
        assert_eq!(batch.steps, 1);
        assert_eq!(batch.dropped_time, Duration::ZERO);
        assert!(accumulator.until_next_step() < FIXED_UPDATE_INTERVAL);
        assert_eq!(accumulator.push(accumulator.until_next_step()).steps, 1);
    }

    #[test]
    fn accumulator_caps_catch_up_work_and_reports_dropped_time() {
        let mut accumulator = FixedStepAccumulator::default();
        let batch = accumulator.push(Duration::from_secs(2));
        assert_eq!(batch.steps, MAX_FIXED_STEPS_PER_TURN);
        assert!(batch.dropped_time > Duration::from_secs(1));
        assert!(accumulator.until_next_step() <= FIXED_UPDATE_INTERVAL);
    }
}
