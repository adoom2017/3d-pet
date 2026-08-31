//! Pet state, behavior, movement, and physics boundary.

use std::time::Duration;

use thiserror::Error;

use crate::display::DesktopPosition;

pub(crate) const DEFAULT_WALK_SPEED_LOGICAL_PX_PER_S: f64 = 80.0;
const DEFAULT_TURN_DURATION: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HorizontalDirection {
    Left,
    Right,
}

impl HorizontalDirection {
    const fn sign(self) -> f64 {
        match self {
            Self::Left => -1.0,
            Self::Right => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum PetState {
    Idle,
    Walking,
    Turning,
    Interacting,
    Dragged,
    Falling,
    Landing,
    Sleeping,
}

impl PetState {
    const fn minimum_priority(self) -> TransitionPriority {
        match self {
            Self::Dragged => TransitionPriority::Drag,
            Self::Falling | Self::Landing => TransitionPriority::Physics,
            Self::Interacting => TransitionPriority::Explicit,
            Self::Idle | Self::Walking | Self::Turning | Self::Sleeping => {
                TransitionPriority::Brain
            }
        }
    }

    const fn accepts_brain_intent(self) -> bool {
        matches!(self, Self::Idle | Self::Walking | Self::Turning)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum PetIntent {
    StayIdle,
    Walk { direction: HorizontalDirection },
    Turn { direction: HorizontalDirection },
    LookAt { desktop_target: DesktopPosition },
    Interact,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum TransitionPriority {
    Brain,
    Explicit,
    Physics,
    Drag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TransitionContext {
    pub priority: TransitionPriority,
}

impl TransitionContext {
    pub const BRAIN: Self = Self {
        priority: TransitionPriority::Brain,
    };
    pub const EXPLICIT: Self = Self {
        priority: TransitionPriority::Explicit,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PetAnimationIntent {
    Idle,
    Walk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransitionRejection {
    SuppressedByPriority,
    InvalidIntent,
    UnsupportedIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransitionOutcome {
    Applied,
    Unchanged,
    Rejected(TransitionRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StateTransition {
    pub previous: PetState,
    pub next: PetState,
    pub outcome: TransitionOutcome,
    pub facing: Option<HorizontalDirection>,
    pub animation: Option<PetAnimationIntent>,
}

impl StateTransition {
    fn unchanged(state: PetState) -> Self {
        Self {
            previous: state,
            next: state,
            outcome: TransitionOutcome::Unchanged,
            facing: None,
            animation: None,
        }
    }

    fn rejected(state: PetState, reason: TransitionRejection) -> Self {
        Self {
            previous: state,
            next: state,
            outcome: TransitionOutcome::Rejected(reason),
            facing: None,
            animation: None,
        }
    }
}

pub(crate) trait PetStateMachine {
    fn state(&self) -> PetState;
    fn facing(&self) -> HorizontalDirection;
    fn apply(&mut self, intent: PetIntent, context: &TransitionContext) -> StateTransition;
    fn fixed_update(&mut self, delta: Duration, context: &TransitionContext) -> StateTransition;
}

pub(crate) struct BehaviorStateMachine {
    state: PetState,
    facing: HorizontalDirection,
    turn_elapsed: Duration,
    pending_walk: Option<HorizontalDirection>,
}

impl Default for BehaviorStateMachine {
    fn default() -> Self {
        Self {
            state: PetState::Idle,
            facing: HorizontalDirection::Right,
            turn_elapsed: Duration::ZERO,
            pending_walk: None,
        }
    }
}

impl BehaviorStateMachine {
    fn transition_to(
        &mut self,
        next: PetState,
        facing: Option<HorizontalDirection>,
        animation: PetAnimationIntent,
    ) -> StateTransition {
        let previous = self.state;
        self.state = next;
        if let Some(direction) = facing {
            self.facing = direction;
        }
        StateTransition {
            previous,
            next,
            outcome: if previous == next {
                TransitionOutcome::Unchanged
            } else {
                TransitionOutcome::Applied
            },
            facing,
            animation: Some(animation),
        }
    }

    #[cfg(test)]
    fn enter_priority_state(&mut self, state: PetState) {
        self.state = state;
        self.pending_walk = None;
        self.turn_elapsed = Duration::ZERO;
    }
}

impl PetStateMachine for BehaviorStateMachine {
    fn state(&self) -> PetState {
        self.state
    }

    fn facing(&self) -> HorizontalDirection {
        self.facing
    }

    fn apply(&mut self, intent: PetIntent, context: &TransitionContext) -> StateTransition {
        if context.priority < self.state.minimum_priority() {
            return StateTransition::rejected(
                self.state,
                TransitionRejection::SuppressedByPriority,
            );
        }
        match intent {
            PetIntent::StayIdle => {
                self.pending_walk = None;
                self.turn_elapsed = Duration::ZERO;
                self.transition_to(PetState::Idle, None, PetAnimationIntent::Idle)
            }
            PetIntent::Walk { direction } => {
                self.pending_walk = None;
                self.turn_elapsed = Duration::ZERO;
                self.transition_to(PetState::Walking, Some(direction), PetAnimationIntent::Walk)
            }
            PetIntent::Turn { direction } if direction == self.facing => {
                StateTransition::rejected(self.state, TransitionRejection::InvalidIntent)
            }
            PetIntent::Turn { direction } => {
                self.pending_walk = Some(direction);
                self.turn_elapsed = Duration::ZERO;
                self.transition_to(PetState::Turning, Some(direction), PetAnimationIntent::Idle)
            }
            PetIntent::Interact if context.priority >= TransitionPriority::Explicit => {
                self.pending_walk = None;
                self.transition_to(PetState::Interacting, None, PetAnimationIntent::Idle)
            }
            PetIntent::Interact | PetIntent::LookAt { .. } => {
                StateTransition::rejected(self.state, TransitionRejection::UnsupportedIntent)
            }
        }
    }

    fn fixed_update(&mut self, delta: Duration, _context: &TransitionContext) -> StateTransition {
        if self.state != PetState::Turning {
            return StateTransition::unchanged(self.state);
        }
        self.turn_elapsed = self.turn_elapsed.saturating_add(delta);
        if self.turn_elapsed < DEFAULT_TURN_DURATION {
            return StateTransition::unchanged(self.state);
        }
        let Some(direction) = self.pending_walk.take() else {
            return StateTransition::rejected(self.state, TransitionRejection::InvalidIntent);
        };
        self.turn_elapsed = Duration::ZERO;
        self.transition_to(PetState::Walking, Some(direction), PetAnimationIntent::Walk)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PetObservation {
    pub state: PetState,
    pub facing: HorizontalDirection,
}

pub(crate) trait RandomSource {
    fn next_unit_f64(&mut self) -> f64;
}

pub(crate) struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub const fn seeded(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl RandomSource for SplitMix64 {
    fn next_unit_f64(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        (value >> 11) as f64 * (1.0 / (1_u64 << 53) as f64)
    }
}

pub(crate) trait MonotonicClock {
    fn now(&self) -> Duration;
}

#[derive(Debug, Default)]
pub(crate) struct SimulationClock {
    now: Duration,
}

impl SimulationClock {
    pub fn advance(&mut self, delta: Duration) {
        self.now = self.now.saturating_add(delta);
    }
}

impl MonotonicClock for SimulationClock {
    fn now(&self) -> Duration {
        self.now
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrainConfig {
    pub idle_min: Duration,
    pub idle_max: Duration,
    pub walk_min: Duration,
    pub walk_max: Duration,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            idle_min: Duration::from_millis(800),
            idle_max: Duration::from_millis(1_400),
            walk_min: Duration::from_millis(1_500),
            walk_max: Duration::from_millis(2_500),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub(crate) enum BrainConfigError {
    #[error("Idle duration range must be non-zero and ordered")]
    InvalidIdleRange,
    #[error("Walk duration range must be non-zero and ordered")]
    InvalidWalkRange,
}

impl BrainConfig {
    pub fn validate(self) -> Result<Self, BrainConfigError> {
        if self.idle_min.is_zero() || self.idle_min > self.idle_max {
            return Err(BrainConfigError::InvalidIdleRange);
        }
        if self.walk_min.is_zero() || self.walk_min > self.walk_max {
            return Err(BrainConfigError::InvalidWalkRange);
        }
        Ok(self)
    }
}

pub(crate) trait PetBrain {
    fn update(
        &mut self,
        observation: &PetObservation,
        now: Duration,
        rng: &mut dyn RandomSource,
    ) -> Option<PetIntent>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrainMode {
    Idle,
    Walking,
}

pub(crate) struct WanderingPetBrain {
    config: BrainConfig,
    mode: BrainMode,
    deadline: Option<Duration>,
}

impl WanderingPetBrain {
    pub fn new(config: BrainConfig) -> Result<Self, BrainConfigError> {
        Ok(Self {
            config: config.validate()?,
            mode: BrainMode::Idle,
            deadline: None,
        })
    }
}

impl PetBrain for WanderingPetBrain {
    fn update(
        &mut self,
        observation: &PetObservation,
        now: Duration,
        rng: &mut dyn RandomSource,
    ) -> Option<PetIntent> {
        if !observation.state.accepts_brain_intent() {
            return None;
        }
        let deadline = *self.deadline.get_or_insert_with(|| {
            now + random_duration(self.config.idle_min, self.config.idle_max, rng)
        });
        if now < deadline {
            return None;
        }
        match self.mode {
            BrainMode::Idle => {
                let direction = if rng.next_unit_f64() < 0.5 {
                    HorizontalDirection::Left
                } else {
                    HorizontalDirection::Right
                };
                self.mode = BrainMode::Walking;
                self.deadline =
                    Some(now + random_duration(self.config.walk_min, self.config.walk_max, rng));
                Some(if direction == observation.facing {
                    PetIntent::Walk { direction }
                } else {
                    PetIntent::Turn { direction }
                })
            }
            BrainMode::Walking => {
                self.mode = BrainMode::Idle;
                self.deadline =
                    Some(now + random_duration(self.config.idle_min, self.config.idle_max, rng));
                Some(PetIntent::StayIdle)
            }
        }
    }
}

fn random_duration(minimum: Duration, maximum: Duration, rng: &mut dyn RandomSource) -> Duration {
    if minimum == maximum {
        return minimum;
    }
    let span = maximum - minimum;
    minimum + span.mul_f64(rng.next_unit_f64())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PhysicsBody {
    pub position: DesktopPosition,
    pub velocity_logical_px_per_s: [f64; 2],
    pub gravity_logical_px_per_s2: f64,
    pub grounded: bool,
}

impl PhysicsBody {
    fn proposed_position(self, delta: Duration) -> DesktopPosition {
        let seconds = delta.as_secs_f64();
        DesktopPosition::new(
            self.position.x + self.velocity_logical_px_per_s[0] * seconds,
            self.position.y + self.velocity_logical_px_per_s[1] * seconds,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MovementState {
    Idle,
    Walking(HorizontalDirection),
}

pub(crate) struct MovementController {
    body: PhysicsBody,
    state: MovementState,
}

impl MovementController {
    pub fn new(position: DesktopPosition) -> Self {
        Self {
            body: PhysicsBody {
                position,
                velocity_logical_px_per_s: [0.0, 0.0],
                gravity_logical_px_per_s2: 0.0,
                grounded: true,
            },
            state: MovementState::Idle,
        }
    }

    pub fn start_walking(&mut self, direction: HorizontalDirection) {
        self.state = MovementState::Walking(direction);
        self.body.velocity_logical_px_per_s[0] =
            direction.sign() * DEFAULT_WALK_SPEED_LOGICAL_PX_PER_S;
    }

    pub fn stop(&mut self) {
        self.state = MovementState::Idle;
        self.body.velocity_logical_px_per_s[0] = 0.0;
    }

    fn proposed_position(&self, delta: Duration) -> DesktopPosition {
        self.body.proposed_position(delta)
    }

    fn confirm_position(&mut self, position: DesktopPosition) {
        self.body.position = position;
    }

    pub fn try_advance<E, T>(
        &mut self,
        delta: Duration,
        mut apply: impl FnMut(DesktopPosition, DesktopPosition) -> Result<(DesktopPosition, T), E>,
    ) -> Result<Option<T>, E> {
        if !matches!(self.state, MovementState::Walking(_)) {
            return Ok(None);
        }
        let current = self.body.position;
        let proposed = self.proposed_position(delta);
        let (confirmed, output) = apply(current, proposed)?;
        self.confirm_position(confirmed);
        Ok(Some(output))
    }

    pub fn position(&self) -> DesktopPosition {
        self.body.position
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::time::{FIXED_UPDATE_INTERVAL, FixedStepAccumulator};

    #[test]
    fn state_machine_turns_then_walks_after_exact_duration() {
        let mut machine = BehaviorStateMachine::default();
        let turning = machine.apply(
            PetIntent::Turn {
                direction: HorizontalDirection::Left,
            },
            &TransitionContext::BRAIN,
        );
        assert_eq!(turning.previous, PetState::Idle);
        assert_eq!(turning.next, PetState::Turning);
        assert_eq!(turning.animation, Some(PetAnimationIntent::Idle));
        assert_eq!(
            machine
                .fixed_update(Duration::from_millis(249), &TransitionContext::BRAIN)
                .outcome,
            TransitionOutcome::Unchanged
        );
        let walking = machine.fixed_update(Duration::from_millis(1), &TransitionContext::BRAIN);
        assert_eq!(walking.next, PetState::Walking);
        assert_eq!(walking.facing, Some(HorizontalDirection::Left));
        assert_eq!(walking.animation, Some(PetAnimationIntent::Walk));
    }

    #[test]
    fn state_machine_rejects_invalid_and_unsupported_intents() {
        let mut machine = BehaviorStateMachine::default();
        assert_eq!(
            machine
                .apply(
                    PetIntent::Turn {
                        direction: HorizontalDirection::Right,
                    },
                    &TransitionContext::BRAIN,
                )
                .outcome,
            TransitionOutcome::Rejected(TransitionRejection::InvalidIntent)
        );
        assert_eq!(
            machine
                .apply(
                    PetIntent::LookAt {
                        desktop_target: DesktopPosition::new(10.0, 20.0),
                    },
                    &TransitionContext::BRAIN,
                )
                .outcome,
            TransitionOutcome::Rejected(TransitionRejection::UnsupportedIntent)
        );
        assert_eq!(
            machine
                .apply(PetIntent::Interact, &TransitionContext::BRAIN)
                .outcome,
            TransitionOutcome::Rejected(TransitionRejection::UnsupportedIntent)
        );
    }

    #[test]
    fn priority_states_suppress_normal_brain_intents() {
        let mut machine = BehaviorStateMachine::default();
        for state in [
            PetState::Interacting,
            PetState::Dragged,
            PetState::Falling,
            PetState::Landing,
        ] {
            machine.enter_priority_state(state);
            assert_eq!(
                machine
                    .apply(PetIntent::StayIdle, &TransitionContext::BRAIN)
                    .outcome,
                TransitionOutcome::Rejected(TransitionRejection::SuppressedByPriority)
            );
        }
        assert!(!PetState::Sleeping.accepts_brain_intent());
        let mut idle_machine = BehaviorStateMachine::default();
        assert_eq!(
            idle_machine
                .apply(PetIntent::Interact, &TransitionContext::EXPLICIT)
                .next,
            PetState::Interacting
        );
    }

    #[test]
    fn brain_config_rejects_invalid_ranges() {
        assert_eq!(
            BrainConfig {
                idle_min: Duration::ZERO,
                ..BrainConfig::default()
            }
            .validate(),
            Err(BrainConfigError::InvalidIdleRange)
        );
        assert_eq!(
            BrainConfig {
                walk_min: Duration::from_secs(2),
                walk_max: Duration::from_secs(1),
                ..BrainConfig::default()
            }
            .validate(),
            Err(BrainConfigError::InvalidWalkRange)
        );
    }

    #[test]
    fn brain_probability_boundaries_select_expected_durations_and_directions() {
        let config = BrainConfig {
            idle_min: Duration::from_millis(10),
            idle_max: Duration::from_millis(20),
            walk_min: Duration::from_millis(30),
            walk_max: Duration::from_millis(50),
        };
        let observation = PetObservation {
            state: PetState::Idle,
            facing: HorizontalDirection::Right,
        };

        let mut minimum = WanderingPetBrain::new(config).expect("valid config");
        let mut minimum_rng = SequenceRandom::new([0.0, 0.0, 0.0]);
        assert_eq!(
            minimum.update(&observation, Duration::ZERO, &mut minimum_rng),
            None
        );
        assert_eq!(
            minimum.update(&observation, Duration::from_millis(10), &mut minimum_rng),
            Some(PetIntent::Turn {
                direction: HorizontalDirection::Left
            })
        );

        let mut maximum = WanderingPetBrain::new(config).expect("valid config");
        let mut maximum_rng = SequenceRandom::new([1.0 - f64::EPSILON, 0.5, 0.0]);
        assert_eq!(
            maximum.update(&observation, Duration::ZERO, &mut maximum_rng),
            None
        );
        assert_eq!(
            maximum.update(&observation, Duration::from_millis(20), &mut maximum_rng),
            Some(PetIntent::Walk {
                direction: HorizontalDirection::Right
            })
        );
    }

    #[test]
    fn fixed_seed_replays_the_complete_decision_sequence() {
        let first = collect_decisions(0x1234_5678);
        let replay = collect_decisions(0x1234_5678);
        assert_eq!(first, replay);
        assert_eq!(
            first,
            vec![
                (
                    200,
                    PetIntent::Turn {
                        direction: HorizontalDirection::Left,
                    },
                ),
                (500, PetIntent::StayIdle),
                (
                    700,
                    PetIntent::Turn {
                        direction: HorizontalDirection::Right,
                    },
                ),
                (1_000, PetIntent::StayIdle),
                (
                    1_200,
                    PetIntent::Turn {
                        direction: HorizontalDirection::Left,
                    },
                ),
                (1_500, PetIntent::StayIdle),
                (
                    1_700,
                    PetIntent::Walk {
                        direction: HorizontalDirection::Left,
                    },
                ),
                (2_000, PetIntent::StayIdle),
            ]
        );
    }

    #[test]
    fn simulation_clock_advances_only_by_injected_time() {
        let mut clock = SimulationClock::default();
        clock.advance(Duration::from_millis(125));
        clock.advance(Duration::from_millis(375));
        assert_eq!(clock.now(), Duration::from_millis(500));
    }

    struct SequenceRandom {
        values: VecDeque<f64>,
    }

    impl SequenceRandom {
        fn new(values: impl IntoIterator<Item = f64>) -> Self {
            Self {
                values: values.into_iter().collect(),
            }
        }
    }

    impl RandomSource for SequenceRandom {
        fn next_unit_f64(&mut self) -> f64 {
            self.values.pop_front().expect("test random value")
        }
    }

    fn collect_decisions(seed: u64) -> Vec<(u128, PetIntent)> {
        let config = BrainConfig {
            idle_min: Duration::from_millis(200),
            idle_max: Duration::from_millis(200),
            walk_min: Duration::from_millis(300),
            walk_max: Duration::from_millis(300),
        };
        let mut brain = WanderingPetBrain::new(config).expect("valid config");
        let mut rng = SplitMix64::seeded(seed);
        let mut machine = BehaviorStateMachine::default();
        let mut decisions = Vec::new();
        for tick in 0..=20_u64 {
            let now = Duration::from_millis(tick * 100);
            let observation = PetObservation {
                state: machine.state(),
                facing: machine.facing(),
            };
            if let Some(intent) = brain.update(&observation, now, &mut rng) {
                decisions.push((now.as_millis(), intent));
                machine.apply(intent, &TransitionContext::BRAIN);
            }
            machine.fixed_update(Duration::from_millis(100), &TransitionContext::BRAIN);
        }
        decisions
    }

    #[test]
    fn walking_uses_logical_pixels_per_second() {
        let mut movement = MovementController::new(DesktopPosition::new(10.0, -20.0));
        movement.start_walking(HorizontalDirection::Right);
        let next = movement.proposed_position(Duration::from_millis(500));
        assert_eq!(next, DesktopPosition::new(50.0, -20.0));
        movement.confirm_position(next);
        movement.start_walking(HorizontalDirection::Left);
        assert_eq!(
            movement.proposed_position(Duration::from_millis(250)),
            DesktopPosition::new(30.0, -20.0)
        );
    }

    #[test]
    fn failed_platform_move_can_preserve_last_confirmed_position() {
        let mut movement = MovementController::new(DesktopPosition::new(5.0, 7.0));
        movement.start_walking(HorizontalDirection::Right);
        let error = movement
            .try_advance(Duration::from_secs(1), |_current, _proposed| {
                Err::<(DesktopPosition, ()), _>("mock failure")
            })
            .expect_err("mock platform must reject the move");
        assert_eq!(error, "mock failure");
        assert_eq!(movement.position(), DesktopPosition::new(5.0, 7.0));
    }

    #[test]
    fn successful_platform_move_commits_the_proposed_position() {
        let mut movement = MovementController::new(DesktopPosition::new(5.0, 7.0));
        movement.start_walking(HorizontalDirection::Right);
        let mut received = None;
        assert_eq!(
            movement.try_advance(Duration::from_secs(1), |current, proposed| {
                assert_eq!(current, DesktopPosition::new(5.0, 7.0));
                received = Some(proposed);
                Ok::<_, ()>((proposed, "moved"))
            }),
            Ok(Some("moved"))
        );
        assert_eq!(received, Some(DesktopPosition::new(85.0, 7.0)));
        assert_eq!(movement.position(), DesktopPosition::new(85.0, 7.0));
    }

    #[test]
    fn successful_platform_move_commits_a_constrained_position() {
        let mut movement = MovementController::new(DesktopPosition::new(5.0, 7.0));
        movement.start_walking(HorizontalDirection::Right);

        let result = movement
            .try_advance(Duration::from_secs(1), |_current, proposed| {
                assert_eq!(proposed, DesktopPosition::new(85.0, 7.0));
                Ok::<_, ()>((DesktopPosition::new(60.0, 7.0), true))
            })
            .expect("mock platform move");

        assert_eq!(result, Some(true));
        assert_eq!(movement.position(), DesktopPosition::new(60.0, 7.0));
    }

    #[test]
    fn fixed_updates_are_independent_of_render_rate() {
        let positions = [15_u32, 30, 60, 120].map(simulate_two_seconds);
        for position in positions {
            assert!((position - 160.0).abs() < 0.001, "position={position}");
        }
    }

    fn simulate_two_seconds(render_fps: u32) -> f64 {
        let mut movement = MovementController::new(DesktopPosition::default());
        movement.start_walking(HorizontalDirection::Right);
        let mut fixed_steps = FixedStepAccumulator::default();
        let frame_count = render_fps * 2;
        for frame in 1..=frame_count {
            let elapsed = Duration::from_secs_f64(frame as f64 / render_fps as f64);
            let previous = Duration::from_secs_f64((frame - 1) as f64 / render_fps as f64);
            for _ in 0..fixed_steps.push(elapsed - previous).steps {
                let next = movement.proposed_position(FIXED_UPDATE_INTERVAL);
                movement.confirm_position(next);
            }
        }
        movement.position().x
    }
}
