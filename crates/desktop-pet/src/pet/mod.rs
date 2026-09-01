//! Pet state, behavior, movement, and physics boundary.

use std::time::Duration;

use thiserror::Error;

use crate::display::DesktopPosition;

pub(crate) const DEFAULT_WALK_SPEED_LOGICAL_PX_PER_S: f64 = 80.0;
pub(crate) const DEFAULT_GRAVITY_LOGICAL_PX_PER_S2: f64 = 1_800.0;
const DEFAULT_TURN_DURATION: Duration = Duration::from_millis(250);
const DEFAULT_INTERACTION_DURATION: Duration = Duration::from_millis(500);
const DEFAULT_LANDING_DURATION: Duration = Duration::from_millis(250);

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
    BeginDrag,
    EndDrag,
    Landed,
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
    pub const PHYSICS: Self = Self {
        priority: TransitionPriority::Physics,
    };
    pub const DRAG: Self = Self {
        priority: TransitionPriority::Drag,
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
    interaction_elapsed: Duration,
    landing_elapsed: Duration,
    pending_walk: Option<HorizontalDirection>,
}

impl Default for BehaviorStateMachine {
    fn default() -> Self {
        Self {
            state: PetState::Idle,
            facing: HorizontalDirection::Right,
            turn_elapsed: Duration::ZERO,
            interaction_elapsed: Duration::ZERO,
            landing_elapsed: Duration::ZERO,
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
        self.interaction_elapsed = Duration::ZERO;
        self.landing_elapsed = Duration::ZERO;
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
                self.interaction_elapsed = Duration::ZERO;
                self.transition_to(PetState::Interacting, None, PetAnimationIntent::Idle)
            }
            PetIntent::BeginDrag if context.priority >= TransitionPriority::Drag => {
                self.pending_walk = None;
                self.turn_elapsed = Duration::ZERO;
                self.interaction_elapsed = Duration::ZERO;
                self.transition_to(PetState::Dragged, None, PetAnimationIntent::Idle)
            }
            PetIntent::EndDrag
                if self.state == PetState::Dragged
                    && context.priority >= TransitionPriority::Drag =>
            {
                self.landing_elapsed = Duration::ZERO;
                self.transition_to(PetState::Falling, None, PetAnimationIntent::Idle)
            }
            PetIntent::Landed
                if self.state == PetState::Falling
                    && context.priority >= TransitionPriority::Physics =>
            {
                self.landing_elapsed = Duration::ZERO;
                self.transition_to(PetState::Landing, None, PetAnimationIntent::Idle)
            }
            PetIntent::Interact
            | PetIntent::LookAt { .. }
            | PetIntent::BeginDrag
            | PetIntent::EndDrag
            | PetIntent::Landed => {
                StateTransition::rejected(self.state, TransitionRejection::UnsupportedIntent)
            }
        }
    }

    fn fixed_update(&mut self, delta: Duration, _context: &TransitionContext) -> StateTransition {
        match self.state {
            PetState::Turning => {
                self.turn_elapsed = self.turn_elapsed.saturating_add(delta);
                if self.turn_elapsed < DEFAULT_TURN_DURATION {
                    return StateTransition::unchanged(self.state);
                }
                let Some(direction) = self.pending_walk.take() else {
                    return StateTransition::rejected(
                        self.state,
                        TransitionRejection::InvalidIntent,
                    );
                };
                self.turn_elapsed = Duration::ZERO;
                self.transition_to(PetState::Walking, Some(direction), PetAnimationIntent::Walk)
            }
            PetState::Interacting => {
                self.interaction_elapsed = self.interaction_elapsed.saturating_add(delta);
                if self.interaction_elapsed < DEFAULT_INTERACTION_DURATION {
                    return StateTransition::unchanged(self.state);
                }
                self.interaction_elapsed = Duration::ZERO;
                self.transition_to(PetState::Idle, None, PetAnimationIntent::Idle)
            }
            PetState::Landing => {
                self.landing_elapsed = self.landing_elapsed.saturating_add(delta);
                if self.landing_elapsed < DEFAULT_LANDING_DURATION {
                    return StateTransition::unchanged(self.state);
                }
                self.landing_elapsed = Duration::ZERO;
                self.transition_to(PetState::Idle, None, PetAnimationIntent::Idle)
            }
            _ => StateTransition::unchanged(self.state),
        }
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
    fn next_deadline(&self) -> Option<Duration>;

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
    fn next_deadline(&self) -> Option<Duration> {
        self.deadline
    }

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

    fn falling_step(self, delta: Duration, ground_y: f64) -> Self {
        let seconds = delta.as_secs_f64();
        if self.position.y > ground_y
            || (self.position.y == ground_y && self.velocity_logical_px_per_s[1] >= 0.0)
        {
            return Self {
                position: DesktopPosition::new(self.position.x, ground_y),
                velocity_logical_px_per_s: [0.0, 0.0],
                gravity_logical_px_per_s2: self.gravity_logical_px_per_s2,
                grounded: true,
            };
        }
        let next_velocity_y =
            self.velocity_logical_px_per_s[1] + self.gravity_logical_px_per_s2 * seconds;
        let proposed = DesktopPosition::new(
            self.position.x + self.velocity_logical_px_per_s[0] * seconds,
            self.position.y
                + self.velocity_logical_px_per_s[1] * seconds
                + 0.5 * self.gravity_logical_px_per_s2 * seconds * seconds,
        );
        if proposed.y >= ground_y {
            let hit_seconds = self.ground_hit_time(ground_y, seconds).unwrap_or(seconds);
            return Self {
                position: DesktopPosition::new(
                    self.position.x + self.velocity_logical_px_per_s[0] * hit_seconds,
                    ground_y,
                ),
                velocity_logical_px_per_s: [0.0, 0.0],
                gravity_logical_px_per_s2: self.gravity_logical_px_per_s2,
                grounded: true,
            };
        }
        Self {
            position: proposed,
            velocity_logical_px_per_s: [self.velocity_logical_px_per_s[0], next_velocity_y],
            gravity_logical_px_per_s2: self.gravity_logical_px_per_s2,
            grounded: false,
        }
    }

    fn ground_hit_time(self, ground_y: f64, maximum_seconds: f64) -> Option<f64> {
        let a = 0.5 * self.gravity_logical_px_per_s2;
        let b = self.velocity_logical_px_per_s[1];
        let c = self.position.y - ground_y;
        if a.abs() <= f64::EPSILON {
            if b.abs() <= f64::EPSILON {
                return None;
            }
            let time = -c / b;
            return (time >= 0.0 && time <= maximum_seconds).then_some(time);
        }
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return None;
        }
        let root = (-b + discriminant.sqrt()) / (2.0 * a);
        (root >= 0.0 && root <= maximum_seconds).then_some(root)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MovementState {
    Idle,
    Walking(HorizontalDirection),
    Falling,
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

    pub fn begin_drag(&mut self) {
        self.state = MovementState::Idle;
        self.body.velocity_logical_px_per_s = [0.0, 0.0];
        self.body.grounded = false;
    }

    pub fn confirm_drag_position(&mut self, position: DesktopPosition) {
        self.confirm_position(position);
    }

    pub fn finish_drag(&mut self, release_velocity: [f64; 2]) {
        self.state = MovementState::Falling;
        self.body.velocity_logical_px_per_s = release_velocity;
        self.body.gravity_logical_px_per_s2 = DEFAULT_GRAVITY_LOGICAL_PX_PER_S2;
        self.body.grounded = false;
    }

    pub fn try_advance_falling<E, T>(
        &mut self,
        delta: Duration,
        ground_y: f64,
        mut apply: impl FnMut(DesktopPosition) -> Result<(DesktopPosition, T), E>,
    ) -> Result<Option<(T, bool)>, E> {
        if self.state != MovementState::Falling {
            return Ok(None);
        }
        let next = self.body.falling_step(delta, ground_y);
        let (confirmed, output) = apply(next.position)?;
        self.body = PhysicsBody {
            position: confirmed,
            ..next
        };
        if next.grounded {
            self.state = MovementState::Idle;
        }
        Ok(Some((output, next.grounded)))
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

    #[cfg(test)]
    fn body(&self) -> PhysicsBody {
        self.body
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
    fn explicit_interaction_returns_to_idle_after_fixed_duration() {
        let mut machine = BehaviorStateMachine::default();
        let interaction = machine.apply(PetIntent::Interact, &TransitionContext::EXPLICIT);
        assert_eq!(interaction.previous, PetState::Idle);
        assert_eq!(interaction.next, PetState::Interacting);
        assert_eq!(interaction.animation, Some(PetAnimationIntent::Idle));
        assert_eq!(
            machine
                .fixed_update(Duration::from_millis(499), &TransitionContext::BRAIN)
                .outcome,
            TransitionOutcome::Unchanged
        );
        let idle = machine.fixed_update(Duration::from_millis(1), &TransitionContext::BRAIN);
        assert_eq!(idle.previous, PetState::Interacting);
        assert_eq!(idle.next, PetState::Idle);
        assert_eq!(idle.animation, Some(PetAnimationIntent::Idle));
    }

    #[test]
    fn drag_priority_suppresses_brain_until_explicit_drag_end() {
        let mut machine = BehaviorStateMachine::default();
        let dragged = machine.apply(PetIntent::BeginDrag, &TransitionContext::DRAG);
        assert_eq!(dragged.previous, PetState::Idle);
        assert_eq!(dragged.next, PetState::Dragged);
        assert_eq!(dragged.animation, Some(PetAnimationIntent::Idle));
        assert_eq!(
            machine
                .apply(PetIntent::StayIdle, &TransitionContext::BRAIN)
                .outcome,
            TransitionOutcome::Rejected(TransitionRejection::SuppressedByPriority)
        );
        let released = machine.apply(PetIntent::EndDrag, &TransitionContext::DRAG);
        assert_eq!(released.previous, PetState::Dragged);
        assert_eq!(released.next, PetState::Falling);
        assert_eq!(released.animation, Some(PetAnimationIntent::Idle));
        assert_eq!(
            machine
                .apply(PetIntent::StayIdle, &TransitionContext::BRAIN)
                .outcome,
            TransitionOutcome::Rejected(TransitionRejection::SuppressedByPriority)
        );
        let landed = machine.apply(PetIntent::Landed, &TransitionContext::PHYSICS);
        assert_eq!(landed.previous, PetState::Falling);
        assert_eq!(landed.next, PetState::Landing);
        assert_eq!(
            machine
                .fixed_update(Duration::from_millis(249), &TransitionContext::PHYSICS)
                .outcome,
            TransitionOutcome::Unchanged
        );
        let idle = machine.fixed_update(Duration::from_millis(1), &TransitionContext::PHYSICS);
        assert_eq!(idle.previous, PetState::Landing);
        assert_eq!(idle.next, PetState::Idle);
    }

    #[test]
    fn movement_drag_position_and_release_velocity_are_explicit() {
        let mut movement = MovementController::new(DesktopPosition::new(-100.0, 50.0));
        movement.start_walking(HorizontalDirection::Right);
        movement.begin_drag();
        movement.confirm_drag_position(DesktopPosition::new(400.0, -20.0));
        assert_eq!(movement.position(), DesktopPosition::new(400.0, -20.0));
        assert_eq!(movement.body().velocity_logical_px_per_s, [0.0, 0.0]);
        assert!(!movement.body().grounded);

        movement.finish_drag([320.0, -180.0]);
        assert_eq!(movement.body().velocity_logical_px_per_s, [320.0, -180.0]);
        assert_eq!(
            movement.body().gravity_logical_px_per_s2,
            DEFAULT_GRAVITY_LOGICAL_PX_PER_S2
        );
        assert_eq!(movement.body().position, DesktopPosition::new(400.0, -20.0));
    }

    #[test]
    fn falling_uses_constant_acceleration_and_preserves_horizontal_velocity() {
        let mut movement = MovementController::new(DesktopPosition::new(10.0, 20.0));
        movement.finish_drag([120.0, -300.0]);
        let advanced = movement
            .try_advance_falling(Duration::from_millis(100), 500.0, |position| {
                Ok::<_, ()>((position, position))
            })
            .expect("fall step")
            .expect("falling movement");
        assert!(!advanced.1);
        assert_position_close(advanced.0, DesktopPosition::new(22.0, -1.0), 1.0e-9);
        assert_position_close(movement.position(), advanced.0, 1.0e-9);
        assert_velocity_close(movement.body().velocity_logical_px_per_s, [120.0, -120.0]);
        assert!(!movement.body().grounded);
    }

    #[test]
    fn falling_clamps_exactly_to_ground_and_stops_once() {
        let mut movement = MovementController::new(DesktopPosition::new(10.0, 0.0));
        movement.finish_drag([100.0, 200.0]);
        let (position, landed) = movement
            .try_advance_falling(Duration::from_secs(2), 400.0, |position| {
                Ok::<_, ()>((position, position))
            })
            .expect("fall step")
            .expect("falling movement");
        assert!(landed);
        assert!((position.y - 400.0).abs() <= 1.0e-9);
        assert!(position.x > 10.0 && position.x < 210.0);
        assert_eq!(movement.body().velocity_logical_px_per_s, [0.0, 0.0]);
        assert!(movement.body().grounded);
        assert_eq!(
            movement
                .try_advance_falling(Duration::from_secs(1), 400.0, |position| {
                    Ok::<_, ()>((position, ()))
                })
                .expect("idle step"),
            None
        );
    }

    #[test]
    fn release_below_ground_is_clamped_without_an_extra_fall() {
        let mut movement = MovementController::new(DesktopPosition::new(-50.0, 420.0));
        movement.finish_drag([300.0, -800.0]);
        let (position, landed) = movement
            .try_advance_falling(Duration::from_millis(16), 400.0, |position| {
                Ok::<_, ()>((position, position))
            })
            .expect("fall step")
            .expect("falling movement");
        assert!(landed);
        assert_eq!(position, DesktopPosition::new(-50.0, 400.0));
    }

    #[test]
    fn upward_release_from_ground_rises_before_landing() {
        let mut movement = MovementController::new(DesktopPosition::new(50.0, 400.0));
        movement.finish_drag([0.0, -600.0]);
        let (position, landed) = movement
            .try_advance_falling(Duration::from_millis(100), 400.0, |position| {
                Ok::<_, ()>((position, position))
            })
            .expect("fall step")
            .expect("falling movement");
        assert!(!landed);
        assert!(position.y < 400.0);

        let mut landed_again = false;
        for _ in 0..100 {
            let Some((_, landed)) = movement
                .try_advance_falling(Duration::from_millis(16), 400.0, |position| {
                    Ok::<_, ()>((position, ()))
                })
                .expect("fall step")
            else {
                break;
            };
            if landed {
                landed_again = true;
                break;
            }
        }
        assert!(landed_again);
        assert_eq!(movement.position(), DesktopPosition::new(50.0, 400.0));
    }

    #[test]
    fn failed_falling_window_move_preserves_body_and_velocity() {
        let mut movement = MovementController::new(DesktopPosition::new(5.0, 7.0));
        movement.finish_drag([80.0, -120.0]);
        let before = movement.body();
        let error = movement
            .try_advance_falling(Duration::from_millis(100), 500.0, |_proposed| {
                Err::<(DesktopPosition, ()), _>("mock failure")
            })
            .expect_err("mock platform must reject the move");
        assert_eq!(error, "mock failure");
        assert_eq!(movement.body(), before);
    }

    #[test]
    fn different_fixed_steps_produce_the_same_ground_position() {
        let fine = simulate_fall(Duration::from_nanos(1_000_000_000 / 120));
        let coarse = simulate_fall(Duration::from_nanos(1_000_000_000 / 30));
        assert_position_close(fine.0, coarse.0, 1.0e-6);
        assert!((fine.1.as_secs_f64() - coarse.1.as_secs_f64()).abs() < 1.0 / 30.0);
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

    fn simulate_fall(delta: Duration) -> (DesktopPosition, Duration) {
        let mut movement = MovementController::new(DesktopPosition::new(20.0, 40.0));
        movement.finish_drag([180.0, -450.0]);
        let mut elapsed = Duration::ZERO;
        for _ in 0..10_000 {
            elapsed += delta;
            let (position, landed) = movement
                .try_advance_falling(delta, 600.0, |position| Ok::<_, ()>((position, position)))
                .expect("fall step")
                .expect("falling movement");
            if landed {
                return (position, elapsed);
            }
        }
        panic!("fall did not reach the ground");
    }

    fn assert_position_close(actual: DesktopPosition, expected: DesktopPosition, epsilon: f64) {
        assert!((actual.x - expected.x).abs() <= epsilon, "x: {actual:?}");
        assert!((actual.y - expected.y).abs() <= epsilon, "y: {actual:?}");
    }

    fn assert_velocity_close(actual: [f64; 2], expected: [f64; 2]) {
        assert!((actual[0] - expected[0]).abs() <= 1.0e-9, "x: {actual:?}");
        assert!((actual[1] - expected[1]).abs() <= 1.0e-9, "y: {actual:?}");
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
