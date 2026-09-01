use std::time::Duration;

use crate::config::FpsConfig;

pub const FIXED_UPDATE_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);
pub const MAX_ACCUMULATED_TIME: Duration = Duration::from_millis(250);
pub const MAX_FIXED_STEPS_PER_TURN: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrameActivity {
    Active,
    Idle,
    Sleeping,
    Static,
}

impl FrameActivity {
    pub(crate) fn target_fps(self, fps: FpsConfig) -> Option<u16> {
        Some(match self {
            Self::Active => fps.active,
            Self::Idle => fps.idle,
            Self::Sleeping => fps.sleep,
            Self::Static => return None,
        })
    }

    fn interval(self, fps: FpsConfig) -> Option<Duration> {
        let frames_per_second = self.target_fps(fps)?;
        Some(Duration::from_secs_f64(1.0 / f64::from(frames_per_second)))
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Sleeping => "sleeping",
            Self::Static => "static",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameDecision {
    pub request_redraw: bool,
    pub next_wake: Option<Duration>,
}

#[derive(Debug)]
pub(crate) struct FrameScheduler {
    fps: FpsConfig,
    activity: FrameActivity,
    dirty: bool,
    next_frame: Option<Duration>,
    redraw_not_before: Option<Duration>,
}

impl FrameScheduler {
    pub fn new(fps: FpsConfig) -> Self {
        Self {
            fps,
            activity: FrameActivity::Static,
            dirty: true,
            next_frame: None,
            redraw_not_before: None,
        }
    }

    pub fn set_activity(&mut self, activity: FrameActivity, now: Duration) -> bool {
        if activity == self.activity {
            return false;
        }
        self.activity = activity;
        self.next_frame = activity.interval(self.fps).map(|interval| now + interval);
        self.redraw_not_before = None;
        true
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.redraw_not_before = None;
    }

    pub fn defer_redraw(&mut self, now: Duration) {
        self.dirty = true;
        self.redraw_not_before = self
            .activity
            .interval(self.fps)
            .map(|interval| now + interval);
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        self.redraw_not_before = None;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[cfg(test)]
    pub fn frame_due(&self, now: Duration) -> bool {
        self.next_frame.is_some_and(|deadline| now >= deadline)
    }

    pub fn presented(&mut self, now: Duration) {
        self.dirty = false;
        self.redraw_not_before = None;
        self.next_frame = self
            .activity
            .interval(self.fps)
            .map(|interval| now + interval);
    }

    pub fn decision(&self, now: Duration, external_deadline: Option<Duration>) -> FrameDecision {
        let redraw_deadline = self
            .dirty
            .then_some(self.redraw_not_before.or(self.next_frame).unwrap_or(now));
        let request_redraw = redraw_deadline.is_some_and(|deadline| now >= deadline);
        let next_wake = if request_redraw {
            None
        } else {
            [redraw_deadline, self.next_frame, external_deadline]
                .into_iter()
                .flatten()
                .min()
        };
        FrameDecision {
            request_redraw,
            next_wake,
        }
    }
}

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

    #[cfg(test)]
    pub fn until_next_step(&self) -> Duration {
        FIXED_UPDATE_INTERVAL.saturating_sub(self.accumulated)
    }

    pub fn reset(&mut self) {
        self.accumulated = Duration::ZERO;
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

    #[test]
    fn scheduler_uses_configured_deadlines_for_each_activity() {
        let fps = FpsConfig::default();
        let now = Duration::from_secs(10);
        for (activity, expected) in [
            (FrameActivity::Active, Duration::from_secs_f64(1.0 / 60.0)),
            (FrameActivity::Idle, Duration::from_secs_f64(1.0 / 30.0)),
            (FrameActivity::Sleeping, Duration::from_secs_f64(1.0 / 15.0)),
        ] {
            let mut scheduler = FrameScheduler::new(fps);
            scheduler.set_activity(activity, now);
            scheduler.presented(now);
            assert_eq!(
                scheduler.decision(now, None).next_wake,
                Some(now + expected)
            );
            assert!(scheduler.frame_due(now + expected));
        }
    }

    #[test]
    fn static_mode_waits_for_an_event_and_redraws_once() {
        let now = Duration::from_secs(5);
        let mut scheduler = FrameScheduler::new(FpsConfig::default());
        scheduler.presented(now);

        assert_eq!(
            scheduler.decision(now, None),
            FrameDecision {
                request_redraw: false,
                next_wake: None,
            }
        );

        scheduler.mark_dirty();
        assert!(scheduler.decision(now, None).request_redraw);
        scheduler.presented(now);
        assert!(!scheduler.decision(now, None).request_redraw);
    }

    #[test]
    fn animated_dirty_work_is_rate_limited_until_the_frame_deadline() {
        let now = Duration::from_secs(5);
        let deadline = now + Duration::from_secs_f64(1.0 / 30.0);
        let mut scheduler = FrameScheduler::new(FpsConfig::default());
        scheduler.set_activity(FrameActivity::Idle, now);
        scheduler.presented(now);
        scheduler.mark_dirty();

        assert_eq!(scheduler.decision(now, None).next_wake, Some(deadline));
        assert!(!scheduler.decision(now, None).request_redraw);
        assert!(scheduler.decision(deadline, None).request_redraw);
    }

    #[test]
    fn activity_change_recomputes_the_next_deadline() {
        let now = Duration::from_secs(3);
        let switched_at = now + Duration::from_millis(5);
        let mut scheduler = FrameScheduler::new(FpsConfig::default());
        scheduler.set_activity(FrameActivity::Idle, now);
        scheduler.presented(now);

        assert!(scheduler.set_activity(FrameActivity::Active, switched_at));
        assert_eq!(
            scheduler.decision(switched_at, None).next_wake,
            Some(switched_at + Duration::from_secs_f64(1.0 / 60.0))
        );
    }

    #[test]
    fn external_deadline_wins_when_earlier_than_next_frame() {
        let now = Duration::from_secs(2);
        let mut scheduler = FrameScheduler::new(FpsConfig::default());
        scheduler.set_activity(FrameActivity::Idle, now);
        scheduler.presented(now);
        let external = now + Duration::from_millis(10);

        assert_eq!(
            scheduler.decision(now, Some(external)).next_wake,
            Some(external)
        );
    }

    #[test]
    fn deferred_surface_retry_does_not_busy_loop() {
        let now = Duration::from_secs(1);
        let mut scheduler = FrameScheduler::new(FpsConfig::default());
        scheduler.set_activity(FrameActivity::Active, now);
        scheduler.presented(now);
        scheduler.defer_redraw(now);

        let decision = scheduler.decision(now, None);
        assert!(!decision.request_redraw);
        assert_eq!(
            decision.next_wake,
            Some(now + Duration::from_secs_f64(1.0 / 60.0))
        );
    }

    #[test]
    fn fixed_simulation_steps_are_independent_of_render_rate() {
        fn steps_for(frame_count: u32, frame_time: Duration) -> usize {
            let mut accumulator = FixedStepAccumulator::default();
            (0..frame_count)
                .map(|_| accumulator.push(frame_time).steps)
                .sum()
        }

        let active_steps = steps_for(60, FIXED_UPDATE_INTERVAL);
        let idle_steps = steps_for(30, FIXED_UPDATE_INTERVAL * 2);
        let sleeping_steps = steps_for(15, FIXED_UPDATE_INTERVAL * 4);
        assert_eq!(active_steps, 60);
        assert_eq!(idle_steps, active_steps);
        assert_eq!(sleeping_steps, active_steps);
    }
}
